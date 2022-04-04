use std::{collections::HashMap, sync::Arc};

use bincode::Options;
use sha2::{Digest as _, Sha256};
use tracing::{debug, info, warn};

use crate::{
    common::{
        deserialize, serialize, ClientId, Config, Digest, OpNumber, ReplicaId, RequestNumber,
        SignedMessage, ViewNumber,
    },
    facade::{App, Receiver, Transport, TxAgent},
    protocol::zyzzyva::message::{self, ToClient, ToReplica},
    stage::{Handle, State, StatefulContext, StatelessContext},
};

pub struct Replica<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    app: Box<dyn App + Send>,
    batch_size: usize,
    address: T::Address,

    view_number: ViewNumber,
    op_number: OpNumber, // last op number used by primary for ordering (known locally)
    // it is also the last op number that has been speculative executed
    commit_number: OpNumber, // last stable checkpoint up to
    history: Vec<LogItem>,
    client_table: HashMap<ClientId, (RequestNumber, ToClient)>,

    request_buffer: Vec<message::Request>,
    reorder_history: HashMap<OpNumber, LogItem>,
    route_table: HashMap<ClientId, T::Address>,

    shared: Arc<Shared<T>>,
}

struct LogItem {
    view_number: ViewNumber,
    op_number: OpNumber,
    batch: Vec<message::Request>,
    history_digest: Digest, // hash(batch, previous history digest), used in checkpoint
}

pub struct Shared<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    address: T::Address,
}

impl<T: Transport> State for Replica<T> {
    type Shared = Arc<Shared<T>>;
    fn shared(&self) -> Self::Shared {
        self.shared.clone()
    }
}

impl<T: Transport> Receiver<T> for StatefulContext<'_, Replica<T>> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Receiver<T> for StatelessContext<Replica<T>> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Replica<T> {
    pub fn register_new(
        config: Config<T>,
        transport: &mut T,
        replica_id: ReplicaId,
        app: impl App + Send + 'static,
        batch_size: usize,
    ) -> Handle<Self> {
        assert!(config.replica(..).len() > 1);

        let replica = Handle::from(Self {
            config: config.clone(),
            transport: transport.tx_agent(),
            id: replica_id,
            app: Box::new(app),
            batch_size,
            address: config.replica(replica_id).clone(),
            view_number: 0,
            op_number: 0,
            commit_number: 0,
            history: Vec::new(),
            client_table: HashMap::new(),
            request_buffer: Vec::new(),
            reorder_history: HashMap::new(),
            route_table: HashMap::new(),
            shared: Arc::new(Shared {
                transport: transport.tx_agent(),
                address: config.replica(replica_id).clone(),
                config,
            }),
        });

        replica.with_stateful(|state| {
            let submit = state.submit.clone();
            transport.register(state, move |remote, buffer| {
                submit.stateless(|shared| shared.receive_buffer(remote, buffer))
            });
        });

        replica
    }
}

impl<T: Transport> StatelessContext<Replica<T>> {
    fn receive_buffer(&self, remote: T::Address, buffer: T::RxBuffer) {
        match deserialize(buffer.as_ref()) {
            Ok(ToReplica::Request(request)) => {
                self.submit
                    .stateful(move |state| state.handle_request(remote, request));
            }
            _ => {}
        }
        warn!("fail to handle received buffer");
    }
}
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    fn handle_request(&mut self, remote: T::Address, message: message::Request) {
        self.route_table.insert(message.client_id, remote.clone());

        if let Some((request_number, to_client)) = self.client_table.get(&message.client_id) {
            if *request_number > message.request_number {
                return;
            }
            if *request_number == message.request_number {
                self.transport
                    .send_message(self, &remote, serialize(to_client));
                return;
            }
        }

        if self.config.view_primary(self.view_number) != self.id {
            todo!("confirm request");
        }

        self.request_buffer.push(message);
        if self.request_buffer.len() >= self.batch_size {
            self.close_batch();
        }
    }

    fn close_batch(&mut self) {
        assert!(self.config.view_primary(self.view_number) == self.id);
        let batch = ..self.batch_size.min(self.request_buffer.len());
        let batch: Vec<_> = self.request_buffer.drain(batch).collect();
        self.op_number += 1;
        let mut order_request = message::OrderRequest {
            view_number: self.view_number,
            op_number: self.op_number,
            history_digest: Digest::default(),
            digest: Digest::default(),
        };
        let history_digest = self
            .history
            .last()
            .map(|item| item.history_digest)
            .unwrap_or_default();
        self.submit.stateless(move |shared| {
            order_request.digest =
                Sha256::digest(bincode::options().serialize(&batch).unwrap()).into();
            order_request.history_digest = Sha256::new()
                .chain_update(history_digest)
                .chain_update(&order_request.digest)
                .finalize()
                .into();
            let signed =
                SignedMessage::sign(order_request.clone(), shared.config.signing_key(shared));
            shared.transport.send_message_to_all(
                shared,
                shared.config.replica(..),
                serialize(ToReplica::OrderRequest(signed.clone(), batch.clone())),
            );
            shared.submit.stateful(move |state| {
                if state.view_number != order_request.view_number {
                    info!("ignore batch from past view");
                    return;
                }
                state.speculative_execute(
                    LogItem {
                        view_number: state.view_number,
                        op_number: order_request.op_number,
                        batch,
                        history_digest: order_request.history_digest,
                    },
                    &signed,
                );
            });
        });
    }

    fn speculative_execute(
        &mut self,
        item: LogItem,
        order_request: &SignedMessage<message::OrderRequest>,
    ) {
        if item.op_number as usize != self.history.len() + 1 {
            info!("reorder history: op number = {}", item.op_number);
            self.reorder_history.insert(item.op_number, item);
            return;
        }

        for (i, request) in item.batch.iter().enumerate() {
            // is it possible for a leader to order a duplicated request?
            // for now i cannot think of a case even during view change
            // should be similar to viewstamped replication right?
            let op_number = item.op_number * self.batch_size as OpNumber + i as OpNumber;
            let result = self.app.execute(op_number, request.op.clone());
            let client_id = request.client_id;
            let request_number = request.request_number;
            let mut response = message::SpeculativeResponse {
                view_number: self.view_number,
                op_number: item.op_number,
                history_digest: item.history_digest,
                digest: Digest::default(),
                client_id,
                request_number,
            };
            let remote = self.route_table.get(&request.client_id).cloned();
            let order_request = order_request.clone();
            let replica_id = self.id;
            self.submit.stateless(move |shared| {
                response.digest = Sha256::digest(&result).into();
                let to_client = ToClient::SpeculativeResponse(
                    SignedMessage::sign(response, shared.config.signing_key(shared)),
                    replica_id,
                    result,
                    order_request,
                );
                if let Some(remote) = remote {
                    shared
                        .transport
                        .send_message(shared, &remote, serialize(to_client.clone()));
                } else {
                    debug!("no route record, skip reply");
                }

                shared.submit.stateful(move |state| {
                    // but a reorder still can happen here, if stateless part
                    // above is too slow
                    if state
                        .client_table
                        .get(&client_id)
                        .map(|(request_number0, _)| *request_number0 < request_number)
                        .unwrap_or(true)
                    {
                        state
                            .client_table
                            .insert(client_id, (request_number, to_client));
                    }
                });
            });
        }
        self.history.push(item);
    }
}
