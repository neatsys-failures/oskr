use std::{
    collections::{HashMap, HashSet},
    io::Write,
    sync::Arc,
};

use k256::ecdsa::{SigningKey, VerifyingKey};
use sha2::{Digest as _, Sha256};
use tracing::{debug, info, warn};

use crate::{
    common::{
        deserialize, serialize, signed::VerifiedMessage, ClientId, Digest, OpNumber, ReplicaId,
        RequestNumber, SignedMessage, ViewNumber,
    },
    replication::pbft::message::{self, ToReplica},
    stage::{Handle, StatefulContext, StatelessContext},
    transport::Transport,
    transport::TxAgent,
    App,
};

pub struct Replica<T: Transport> {
    id: ReplicaId,
    batch_size: usize,

    view_number: ViewNumber,
    op_number: OpNumber, // one OpNumber for a batch
    commit_number: OpNumber,

    client_table: HashMap<ClientId, (RequestNumber, Option<SignedMessage<message::Reply>>)>,
    log: Vec<LogItem>,
    reorder_log: HashMap<OpNumber, LogItem>,
    batch: Vec<message::Request>,

    prepare_quorum: HashMap<QuorumKey, HashMap<ReplicaId, SignedMessage<message::Prepare>>>,
    commit_quorum: HashMap<QuorumKey, HashSet<ReplicaId>>,

    app: Box<dyn App + Send>,
    route_table: HashMap<ClientId, T::Address>,

    shared: Arc<Shared<T>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
struct QuorumKey {
    view_number: ViewNumber,
    op_number: OpNumber,
    digest: Digest,
}

impl<T: Transport> Replica<T> {
    fn log_item(&self, op_number: OpNumber) -> Option<&LogItem> {
        if let Some(item) = self.log.get((op_number - 1) as usize) {
            Some(item)
        } else if let Some(item) = self.reorder_log.get(&op_number) {
            Some(item)
        } else {
            None
        }
    }

    fn log_item_mut(&mut self, op_number: OpNumber) -> Option<&mut LogItem> {
        if let Some(item) = self.log.get_mut((op_number - 1) as usize) {
            Some(item)
        } else if let Some(item) = self.reorder_log.get_mut(&op_number) {
            Some(item)
        } else {
            None
        }
    }

    // view number is decided by pre-prepare
    fn prepared(&self, op_number: OpNumber, n_fault: usize) -> bool {
        let quorum_key = if let Some(item) = self.log_item(op_number) {
            &item.quorum_key
        } else {
            return false;
        };
        if let Some(prepare_quorum) = self.prepare_quorum.get(quorum_key) {
            prepare_quorum.len() >= n_fault * 2
        } else {
            false
        }
    }

    // committed-local, actually
    fn committed(&self, op_number: OpNumber, n_fault: usize) -> bool {
        // a little bit duplicated, but I accept
        let quorum_key = if let Some(item) = self.log_item(op_number) {
            &item.quorum_key
        } else {
            return false;
        };
        let prepared = if let Some(prepare_quorum) = self.prepare_quorum.get(quorum_key) {
            prepare_quorum.len() >= n_fault * 2
        } else {
            false
        };
        prepared
            && if let Some(commit_quorum) = self.commit_quorum.get(quorum_key) {
                commit_quorum.len() >= n_fault * 2 + 1
            } else {
                false
            }
    }
}

struct LogItem {
    quorum_key: QuorumKey,
    batch: Vec<message::Request>,
    pre_prepare: SignedMessage<message::PrePrepare>,
    committed: bool,
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
        assert!(transport.tx_agent().config().replica_address.len() > 1);

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
                batch_size,
                view_number: 0,
                op_number: 0,
                commit_number: 0,
                client_table: HashMap::new(),
                log: Vec::new(),
                reorder_log: HashMap::new(),
                batch: Vec::new(),
                prepare_quorum: HashMap::new(),
                commit_quorum: HashMap::new(),
                app: Box::new(app),
                route_table: HashMap::new(),
                shared: shared.clone(),
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
            ToReplica::Prepare(prepare) => {
                if let Ok(prepare) = prepare.verify(verifying_key) {
                    self.submit
                        .stateful(|replica| replica.handle_prepare(remote, prepare));
                    return;
                }
            }
            ToReplica::Commit(commit) => {
                if let Ok(commit) = commit.verify(verifying_key) {
                    self.submit
                        .stateful(|replica| replica.handle_commit(remote, commit))
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
                        self.transport.send_message(
                            self,
                            &remote,
                            // we can sign on reference? awesome
                            serialize(SignedMessage::sign(reply, &shared.signing_key)),
                        );
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

        self.op_number += 1;
        let mut pre_prepare = message::PrePrepare {
            view_number: self.view_number,
            op_number: self.op_number,
            digest: Default::default(),
        };

        let shared = self.shared.clone();
        self.submit.stateless(move |replica| {
            let mut batch_buffer = Vec::new();
            serialize(batch.clone())(&mut batch_buffer);
            let digest = Sha256::digest(&batch_buffer).into();
            pre_prepare.digest = digest;
            let quorum_key = QuorumKey {
                view_number: pre_prepare.view_number,
                op_number: pre_prepare.op_number,
                digest,
            };

            let pre_prepare = SignedMessage::sign(pre_prepare, &shared.signing_key);
            replica.transport.send_message_to_all(replica, |buffer| {
                let offset = serialize(ToReplica::PrePrepare(pre_prepare.clone()))(buffer);
                (&mut buffer[offset as usize..])
                    .write(&batch_buffer)
                    .unwrap();
                offset + batch_buffer.len() as u16
            });

            replica.submit.stateful(move |replica| {
                // TODO check view and status
                assert_eq!(quorum_key.op_number, replica.log.len() as OpNumber + 1);
                replica.log.push(LogItem {
                    quorum_key,
                    batch,
                    pre_prepare,
                    committed: false,
                });
            });
        });
    }

    fn handle_pre_prepare(
        &mut self,
        _remote: T::Address,
        message: VerifiedMessage<message::PrePrepare>,
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

        if self.log.len() as OpNumber >= message.op_number
            || self.reorder_log.contains_key(&message.op_number)
        {
            return;
        }

        let quorum_key = QuorumKey {
            view_number: message.view_number,
            op_number: message.op_number,
            digest: message.digest,
        };
        let item = LogItem {
            quorum_key,
            batch,
            pre_prepare: message.signed_message().clone(),
            committed: false,
        };
        if message.op_number != self.log.len() as OpNumber + 1 {
            self.reorder_log.insert(message.op_number, item);
        } else {
            self.log.push(item);
            let mut insert_number = self.log.len() as OpNumber + 1;
            while let Some(item) = self.reorder_log.remove(&insert_number) {
                self.log.push(item);
                insert_number += 1;
            }
        }

        let prepare = message::Prepare {
            view_number: self.view_number,
            op_number: message.op_number,
            digest: message.digest,
            replica_id: self.id,
        };
        let shared = self.shared.clone();
        self.submit.stateless(move |replica| {
            let signed_prepare = SignedMessage::sign(prepare.clone(), &shared.signing_key);
            replica.transport.send_message_to_all(
                replica,
                serialize(ToReplica::Prepare(signed_prepare.clone())),
            );

            replica.submit.stateful(move |replica| {
                replica.insert_prepare(&prepare, &signed_prepare);
            });
        });
    }

    fn handle_prepare(&mut self, _remote: T::Address, message: VerifiedMessage<message::Prepare>) {
        self.insert_prepare(&*message, message.signed_message());
    }

    fn insert_prepare(
        &mut self,
        prepare: &message::Prepare,
        signed_message: &SignedMessage<message::Prepare>,
    ) {
        let quorum_key = QuorumKey {
            view_number: prepare.view_number,
            op_number: prepare.op_number,
            digest: prepare.digest,
        };
        self.prepare_quorum
            .entry(quorum_key)
            .or_default()
            .insert(prepare.replica_id, signed_message.clone());

        if self.prepared(prepare.op_number, self.transport.config().n_fault) {
            debug!("prepared");

            let commit = message::Commit {
                view_number: self.view_number,
                op_number: prepare.op_number,
                digest: prepare.digest,
                replica_id: self.id,
            };

            {
                let commit = commit.clone();
                let shared = self.shared.clone();
                self.submit.stateless(move |replica| {
                    replica.transport.send_message_to_all(
                        replica,
                        serialize(ToReplica::Commit(SignedMessage::sign(
                            commit,
                            &shared.signing_key,
                        ))),
                    )
                });
            }

            self.insert_commit(&commit);
        }
    }

    fn handle_commit(&mut self, _remote: T::Address, message: VerifiedMessage<message::Commit>) {
        self.insert_commit(&*message);
    }

    fn insert_commit(&mut self, commit: &message::Commit) {
        let quorum_key = QuorumKey {
            view_number: commit.view_number,
            op_number: commit.op_number,
            digest: commit.digest,
        };

        self.commit_quorum
            .entry(quorum_key)
            .or_default()
            .insert(commit.replica_id);

        if self.committed(commit.op_number, self.transport.config().n_fault) {
            debug!("committed");

            self.log_item_mut(commit.op_number).unwrap().committed = true;
            self.execute_committed();
        }
    }

    fn execute_committed(&mut self) {
        while let Some(item) = self.log.get(self.commit_number as usize) {
            assert_eq!(item.quorum_key.op_number, self.commit_number + 1);
            if !item.committed {
                break;
            }

            // why have to clone?
            for request in item.batch.clone() {
                let result = self.app.execute(request.op);
                let reply = message::Reply {
                    view_number: self.view_number,
                    request_number: request.request_number,
                    client_id: request.client_id,
                    replica_id: self.id,
                    result,
                };
                let shared = self.shared.clone();
                self.submit.stateless(move |replica| {
                    let reply = SignedMessage::sign(reply, &shared.signing_key);
                    replica.submit.stateful(move |replica| {
                        replica.client_table.insert(
                            request.client_id,
                            (request.request_number, Some(reply.clone())),
                        );
                        if let Some(remote) = replica.route_table.get(&request.client_id) {
                            replica
                                .transport
                                // is it ok to serialize in stateful path?
                                .send_message(replica, remote, serialize(reply));
                        } else {
                            info!("no route record, skip reply");
                        }
                    });
                });
            }

            self.commit_number += 1;
        }
    }
}
