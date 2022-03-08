use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use bincode::Options;
use k256::ecdsa::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::{
    common::{
        deserialize, serialize, ClientId, OpNumber, ReplicaId, RequestNumber, SignedMessage,
        ViewNumber,
    },
    replication::pbft::message::{self, ToReplica},
    stage::{Handle, StatefulContext, StatelessContext},
    transport::Transport,
    transport::TxAgent,
    App,
};

pub struct Replica<T: Transport> {
    id: ReplicaId,
    shared: Arc<Shared<T>>,
    batch_size: usize,

    view_number: ViewNumber,
    op_number: OpNumber,

    client_table: HashMap<ClientId, (RequestNumber, Option<message::Reply>)>,
    log: Vec<(ViewNumber, message::Request)>,
    batch: Vec<message::Request>,

    digest_table: HashMap<[u8; 32], Range<OpNumber>>,
    prepare_quorum: HashMap<[u8; 32], HashSet<ReplicaId>>,
    commit_quorum: HashMap<[u8; 32], HashSet<ReplicaId>>,

    app: Box<dyn App + Send>,
    route_table: HashMap<ClientId, T::Address>,
}

struct Shared<T: Transport> {
    signing_key: SigningKey,
    verifying_key: HashMap<T::Address, VerifyingKey>,
}

impl<T: Transport> Replica<T> {
    pub fn register_new(
        transport: &mut T,
        replica_id: ReplicaId,
        app: impl App + Send + 'static,
        batch_size: usize,
    ) -> Handle<Self, T> {
        let address = transport.tx_agent().config().replica_address[replica_id as usize].clone();
        let shared = Arc::new(Shared {
            verifying_key: transport.tx_agent().config().verifying_key(),
            signing_key: transport.tx_agent().config().signing_key[&address].clone(),
        });
        let replica = Handle::new(
            transport.tx_agent(),
            address.clone(),
            Self {
                id: replica_id,
                shared: shared.clone(),
                batch_size,
                view_number: 0,
                op_number: 0,
                client_table: HashMap::new(),
                log: Vec::new(),
                batch: Vec::new(),
                digest_table: HashMap::new(),
                prepare_quorum: HashMap::new(),
                commit_quorum: HashMap::new(),
                app: Box::new(app),
                route_table: HashMap::new(),
            },
        );

        replica.with_state(|replica| {
            let submit = replica.submit.clone();
            transport.register(replica, move |remote, buffer| {
                if shared.verifying_key.contains_key(&remote) {
                    let shared = shared.clone();
                    submit.stateless(move |replica| replica.receive_buffer(remote, buffer, shared));
                } else {
                    submit.stateful(move |replica| replica.receive_buffer(remote, buffer));
                }
            });
        });
        replica
    }
}

impl<'a, T: Transport> StatefulContext<'a, Replica<T>, T> {
    fn receive_buffer(&mut self, remote: T::Address, buffer: T::RxBuffer) {
        let to_replica: Result<ToReplica, _> = deserialize(buffer.as_ref());
        let to_replica = if let Ok(to_replica) = to_replica {
            to_replica
        } else {
            warn!("receive malformed client message");
            return;
        };
        match to_replica {
            ToReplica::Request(request) => {
                self.route_table
                    .entry(request.client_id)
                    .or_insert(remote.clone());
                self.handle_request(Some(remote), request);
                return;
            }

            _ => warn!("receive unexpected client message"),
        }
    }
}

impl<T: Transport> StatelessContext<Replica<T>, T> {
    fn receive_buffer(&self, remote: T::Address, buffer: T::RxBuffer, shared: Arc<Shared<T>>) {
        let mut buffer = buffer.as_ref();
        let to_replica: Result<ToReplica, _> = bincode::DefaultOptions::new()
            .allow_trailing_bytes()
            .deserialize_from(&mut buffer);
        let to_replica = if let Ok(to_replica) = to_replica {
            to_replica
        } else {
            warn!("receive malformed replica message");
            return;
        };
        let verifying_key = &shared.verifying_key[&remote];
        match to_replica {
            ToReplica::RelayedRequest(request) => {
                self.submit.stateful(|replica| {
                    replica.handle_request(
                        replica.route_table.get(&request.client_id).cloned(),
                        request,
                    )
                });
                return;
            }
            ToReplica::PrePrepare(pre_prepare) => {
                if let Ok(pre_prepare) = pre_prepare.verify(verifying_key) {
                    if Sha256::digest(buffer)[..] == pre_prepare.digest {
                        let batch: Result<Vec<message::Request>, _> = deserialize(buffer);
                        if let Ok(batch) = batch {
                            self.submit.stateful(|replica| {
                                replica.handle_pre_prepare(remote, pre_prepare, batch)
                            });
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
        warn!("fail to verify replica message");
    }
}

impl<'a, T: Transport> StatefulContext<'a, Replica<T>, T> {
    fn handle_request(&mut self, remote: Option<T::Address>, message: message::Request) {
        if let Some((request_number, reply)) = self.client_table.get(&message.client_id) {
            if *request_number > message.request_number {
                return;
            }
            if *request_number == message.request_number {
                match (remote, reply) {
                    (Some(remote), Some(reply)) => {
                        let reply = reply.clone();
                        let shared = self.shared.clone();
                        self.submit.stateless(move |replica| {
                            replica.transport.send_message(
                                replica,
                                &remote,
                                serialize(SignedMessage::sign(reply, &shared.signing_key)),
                            );
                        });
                    }
                    _ => {}
                }
            }
        }

        let primary = self.transport.config().view_primary(self.view_number);
        if self.id != primary {
            self.transport.send_message_to_replica(
                self,
                primary,
                serialize(ToReplica::RelayedRequest(message)),
            );
            return;
        }

        self.batch.push(message);
        if self.batch.len() == self.batch_size {
            // close batch
        }
    }

    fn close_batch(&mut self) {
        if self.batch_size == 0 {
            return;
        }
        //
    }

    fn handle_pre_prepare(
        &mut self,
        remote: T::Address,
        message: message::PrePrepare,
        batch: Vec<message::Request>,
    ) {
        //
    }
}
