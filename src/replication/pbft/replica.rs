use std::{
    collections::{HashMap, HashSet},
    io::Write,
    ops::Range,
    sync::Arc,
};

use k256::ecdsa::{SigningKey, VerifyingKey};
use sha2::{Digest as _, Sha256};
use tracing::warn;

use crate::{
    common::{
        deserialize, serialize, ClientId, Digest, OpNumber, ReplicaId, RequestNumber,
        SignedMessage, ViewNumber,
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
    commit_number: OpNumber,

    // instead of OpNumber, most data structure is keyed by Digest, because of
    // two reasons:
    // the implementation is built with batching from beginning, where
    // consecutive OpNumber share the same Digest
    // under BFT condition multiple version of message with same OpNumber can
    // be received, making OpNumber not be unique key any more
    digest_table: HashMap<OpNumber, Digest>,

    client_table: HashMap<ClientId, (RequestNumber, Option<message::Reply>)>,
    log: HashMap<Digest, (SignedMessage<message::PrePrepare>, Vec<message::Request>)>,
    batch: Vec<message::Request>,

    prepare_quorum: HashMap<Digest, HashMap<ReplicaId, SignedMessage<message::Prepare>>>,
    commit_quorum: HashMap<Digest, HashSet<ReplicaId>>,

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
                commit_number: 0,
                client_table: HashMap::new(),
                log: HashMap::new(),
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
        let to_replica: Result<ToReplica, _> = deserialize(&mut buffer);
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
            self.close_batch();
        }
    }

    fn close_batch(&mut self) {
        assert!(self.transport.config().view_primary(self.view_number) == self.id);
        if self.batch_size == 0 {
            return;
        }

        let batch: Vec<_> = self.batch.drain(..).collect();

        let mut pre_prepare = message::PrePrepare {
            view_number: self.view_number,
            op_number: self.op_number + 1,
            digest: Default::default(),
        };
        self.op_number += batch.len() as OpNumber;

        let shared = self.shared.clone();
        self.submit.stateless(move |replica| {
            let mut batch_buffer = Vec::new();
            serialize(batch.clone())(&mut batch_buffer);
            let digest = Sha256::digest(&batch_buffer).into();
            pre_prepare.digest = digest;

            let op_number = pre_prepare.op_number;
            let pre_prepare = SignedMessage::sign(pre_prepare, &shared.signing_key);

            replica.transport.send_message_to_all(replica, |buffer| {
                let offset = serialize(ToReplica::PrePrepare(pre_prepare.clone()))(buffer);
                (&mut buffer[offset as usize..])
                    .write(&batch_buffer)
                    .unwrap();
                offset + batch_buffer.len() as u16
            });

            replica.submit.stateful(move |replica| {
                replica.digest_table.insert(op_number, digest);
                replica.log.insert(digest, (pre_prepare, batch));
            });
        });
    }

    fn handle_pre_prepare(
        &mut self,
        remote: T::Address,
        message: message::PrePrepare,
        batch: Vec<message::Request>,
    ) {
        if message.view_number < self.view_number {
            return;
        }
        if message.view_number > self.view_number {
            // TODO state transfer
            return;
        }
        assert!(self.transport.config().view_primary(self.view_number) != self.id);
    }
}
