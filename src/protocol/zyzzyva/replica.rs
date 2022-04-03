use std::{collections::HashMap, sync::Arc};

use tracing::warn;

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
            // close batch
        }
    }
}
