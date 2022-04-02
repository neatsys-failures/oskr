use std::{
    collections::{HashMap, HashSet},
    io::Write,
    sync::Arc,
};

use sha2::{Digest as _, Sha256};
use tracing::{debug, info, warn};

use crate::{
    common::{
        deserialize, serialize, signed::VerifiedMessage, ClientId, Config, Digest, OpNumber,
        ReplicaId, RequestNumber, SignedMessage, ViewNumber,
    },
    facade::{App, Receiver, Transport, TxAgent},
    protocol::pbft::message::{self, ToReplica},
    stage::{Handle, State, StatefulContext, StatelessContext},
};

// notes on data structure
// since PBFT clearly models after Viewstamped Replication, I think it's better
// to use linear log and quorum set table instead of tree-like one
// for BFT requirement, following PBFT paper we mostly rely on "first sight"
// pre-prepare: prepare and commit messages are not considered unless they have
// matching digest to pre-prepare message. This policy, along with the fact that
// we only consider matching-view messages, and we are using op number as data
// structures key, is how we implement PBFT's "matching" definition, i.e. same
// view number, op number and digest.
// the caveat the comes from this implementation way: we need extra care during
// view change. we should flush log in VR fashion, and we should flush committed
// quorum as well. Another problem is when prepare before pre-prepare and commit
// before pre-prepare out of order case happens, we don't have a accepted digest
// to match against. My solution is to create reordered buffers for each kind
// of message. According to logging, with proper rate control (described below),
// the out of order happens rarely.
// in PBFT paper, it is mentioned that the number of "messages in progress"
// should not exceed a given maximum, and the maximum is not specified. I choose
// the maximum based on runtime concurrency, i.e. how many messages a server can
// process concurrently. The value is estimated, and is tuned a little bit more
// optimitic to get best performance (without increasing out of order message).
// Finally, these reorder buffers must be cleared on view change as well
// obivously.

pub struct Replica<T: Transport> {
    address: T::Address,
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    batch_size: usize,
    adaptive_batching: bool,

    view_number: ViewNumber,
    op_number: OpNumber, // one OpNumber for a batch
    commit_number: OpNumber,

    client_table: HashMap<ClientId, (RequestNumber, Option<SignedMessage<message::Reply>>)>,
    log: Vec<LogItem>,
    request_buffer: Vec<message::Request>,
    commit_quorum: HashMap<OpNumber, HashSet<ReplicaId>>,

    reorder_log: HashMap<OpNumber, LogItem>,
    reorder_prepare: HashMap<OpNumber, Vec<VerifiedMessage<message::Prepare>>>,
    reorder_commit: HashMap<OpNumber, Vec<VerifiedMessage<message::Commit>>>,

    app: Box<dyn App + Send>,
    pub(super) route_table: HashMap<ClientId, T::Address>,

    shared: Arc<Shared<T>>,
}

struct LogItem {
    view_number: ViewNumber,
    op_number: OpNumber,
    digest: Digest,
    batch: Vec<message::Request>,
    pre_prepare: SignedMessage<message::PrePrepare>,
    prepare_quorum: HashMap<ReplicaId, SignedMessage<message::Prepare>>,
    committed: bool,
}

pub struct Shared<T: Transport> {
    address: T::Address,
    config: Config<T>,
    transport: T::TxAgent,
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
    fn prepared(&self, op_number: OpNumber) -> bool {
        if let Some(item) = self.log_item(op_number) {
            &item.prepare_quorum
        } else {
            return false;
        }
        .len()
            >= self.config.f * 2
    }

    // committed-local, actually
    #[allow(clippy::int_plus_one)] // I want to follow PBFT paper, preceisely
    fn committed(&self, op_number: OpNumber) -> bool {
        // a little bit duplicated, but I accept that
        self.prepared(op_number)
            && if let Some(commit_quorum) = self.commit_quorum.get(&op_number) {
                commit_quorum.len() >= self.config.f * 2 + 1
            } else {
                false
            }
    }

    fn is_primary(&self) -> bool {
        self.config.view_primary(self.view_number) == self.id
    }
}

impl<'a, T: Transport> Receiver<T> for StatefulContext<'a, Replica<T>> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Receiver<T> for StatelessContext<Replica<T>> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> State for Replica<T> {
    type Shared = Arc<Shared<T>>;
    fn shared(&self) -> Self::Shared {
        self.shared.clone()
    }
}

impl<T: Transport> Replica<T> {
    pub fn register_new(
        config: Config<T>,
        transport: &mut T,
        replica_id: ReplicaId,
        app: impl App + Send + 'static,
        batch_size: usize,
        adaptive_batching: bool,
    ) -> Handle<Self> {
        assert!(config.replica(..).len() > 1); // TODO

        let replica: Handle<_> = Self {
            address: config.replica(replica_id).clone(),
            config: config.clone(),
            transport: transport.tx_agent(),
            id: replica_id,
            batch_size,
            adaptive_batching,
            view_number: 0,
            op_number: 0,
            commit_number: 0,
            client_table: HashMap::new(),
            log: Vec::new(),
            request_buffer: Vec::new(),
            commit_quorum: HashMap::new(),
            reorder_log: HashMap::new(),
            reorder_prepare: HashMap::new(),
            reorder_commit: HashMap::new(),
            app: Box::new(app),
            route_table: HashMap::new(),
            shared: Arc::new(Shared {
                address: config.replica(replica_id).clone(),
                config,
                transport: transport.tx_agent(),
            }),
        }
        .into();

        replica.with_stateless(|replica| {
            let config = replica.config.clone();
            let submit = replica.submit.clone();
            transport.register(replica, {
                move |remote, buffer| {
                    // shortcut: if we don't have verifying key for remote, we
                    // cannot do verify so skip stateless task
                    if config.verifying_key(&remote).is_some() {
                        submit.stateless(move |replica| replica.receive_buffer(remote, buffer));
                    } else {
                        submit.stateful(move |replica| replica.receive_buffer(remote, buffer));
                    }
                }
            });
        });
        replica
    }
}

impl<'a, T: Transport> StatefulContext<'a, Replica<T>> {
    fn receive_buffer(&mut self, remote: T::Address, buffer: T::RxBuffer) {
        #[allow(clippy::single_match)] // although no future plan to add more branch
        // just to keep uniform shape
        match deserialize(buffer.as_ref()) {
            Ok(ToReplica::Request(request)) => {
                self.handle_request(remote, request);
                return;
            }
            _ => {}
        }
        warn!("receive unexpected client message");
    }
}

impl<T: Transport> StatelessContext<Replica<T>> {
    fn receive_buffer(&self, remote: T::Address, buffer: T::RxBuffer) {
        let mut buffer = buffer.as_ref();
        match deserialize(&mut buffer) {
            Ok(ToReplica::RelayedRequest(request)) => {
                self.submit
                    .stateful(|replica| replica.handle_relayed_request(remote, request));
                return;
            }
            Ok(ToReplica::PrePrepare(pre_prepare)) => {
                if let Ok(pre_prepare) =
                    pre_prepare.verify(self.config.verifying_key(&remote).unwrap())
                {
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
            Ok(ToReplica::Prepare(prepare)) => {
                if let Ok(prepare) = prepare.verify(self.config.verifying_key(&remote).unwrap()) {
                    self.submit
                        .stateful(|replica| replica.handle_prepare(remote, prepare));
                    return;
                }
            }
            Ok(ToReplica::Commit(commit)) => {
                if let Ok(commit) = commit.verify(self.config.verifying_key(&remote).unwrap()) {
                    self.submit
                        .stateful(|replica| replica.handle_commit(remote, commit));
                    return;
                }
            }
            _ => {}
        }
        warn!("fail to verify replica message");
    }
}

impl<'a, T: Transport> StatefulContext<'a, Replica<T>> {
    fn handle_request(&mut self, remote: T::Address, message: message::Request) {
        self.route_table.insert(message.client_id, remote.clone());
        self.handle_request_internal(Some(remote), message);
    }

    fn handle_relayed_request(&mut self, _remote: T::Address, message: message::Request) {
        self.handle_request_internal(self.route_table.get(&message.client_id).cloned(), message);
    }

    fn handle_request_internal(&mut self, remote: Option<T::Address>, message: message::Request) {
        if let Some((request_number, reply)) = self.client_table.get(&message.client_id) {
            if *request_number > message.request_number {
                return;
            }
            if *request_number == message.request_number {
                if let (Some(remote), Some(reply)) = (remote, reply) {
                    let reply = reply.clone();
                    // maybe not a good idea to serialize in stateful path
                    // I don't want to store binary message in replica state
                    // and the serailization should be blazing fast
                    // and after all, this is not the fast path
                    self.transport.send_message(self, &remote, serialize(reply));
                }
                return;
            }
        }

        if !self.is_primary() {
            self.transport.send_message(
                self,
                self.config
                    .replica(self.config.view_primary(self.view_number)),
                serialize(ToReplica::RelayedRequest(message)),
            );
            // TODO start view change timeout
            return;
        }

        self.request_buffer.push(message);

        // notice this assume all servers set up the same number of workers as
        // leader
        let estimated_available_concurrency = Arc::strong_count(&self.shared);
        // each on-the-fly op number takes n concurrency to sign/verify
        // pre-prepare/prepare, and n concurrency to sign/verify commit
        let op_concurrency = self.config.replica(..).len() * 2;
        let estimated_allocated_concurrency =
            (self.op_number - self.commit_number) as usize * op_concurrency;

        // we give extra concurrency for one op, for
        // * the potential pipelining feature of the system
        // * the fact that sometimes op commits out of order
        // * allow system to move on when available concurrecy is too small
        if estimated_allocated_concurrency < estimated_available_concurrency + op_concurrency {
            if self.request_buffer.len() >= self.batch_size || self.adaptive_batching {
                self.close_batch();
            }
        }
    }

    fn close_batch(&mut self) {
        assert!(self.is_primary());

        let batch = ..self.batch_size.min(self.request_buffer.len());
        let batch: Vec<_> = self.request_buffer.drain(batch).collect();

        self.op_number += 1;
        let mut pre_prepare = message::PrePrepare {
            view_number: self.view_number,
            op_number: self.op_number,
            digest: Default::default(),
        };

        let view_number = self.view_number;
        let op_number = self.op_number;
        self.submit.stateless(move |replica| {
            let mut batch_buffer = Vec::new();
            serialize(batch.clone())(&mut batch_buffer);
            let digest = Sha256::digest(&batch_buffer).into();
            pre_prepare.digest = digest;
            let pre_prepare = SignedMessage::sign(pre_prepare, replica.config.signing_key(replica));
            replica
                .transport
                .send_message_to_all(replica, replica.config.replica(..), |buffer| {
                    let offset = serialize(ToReplica::PrePrepare(pre_prepare.clone()))(buffer);
                    (&mut buffer[offset as usize..])
                        .write_all(&batch_buffer)
                        .unwrap();
                    offset + batch_buffer.len() as u16
                });

            replica.submit.stateful(move |replica| {
                if replica.view_number != view_number {
                    info!("discard log item from another view");
                    return;
                }

                replica.insert_log_item(LogItem {
                    view_number,
                    op_number,
                    digest,
                    batch,
                    pre_prepare,
                    prepare_quorum: HashMap::new(),
                    committed: false,
                });
            });
        });
    }

    fn insert_log_item(&mut self, item: LogItem) {
        for request in &item.batch {
            if self
                .client_table
                .get(&request.client_id)
                .map(|(request_number, _)| *request_number < request.request_number)
                .unwrap_or(true)
            {
                self.client_table
                    .insert(request.client_id, (request.request_number, None));
            }
        }

        let digest = item.digest;
        let op_number = item.op_number;
        if op_number != self.log.len() as OpNumber + 1 {
            info!(
                "out of order log item: {} (expect {})",
                item.op_number,
                self.log.len() as OpNumber + 1
            );
            self.reorder_log.insert(op_number, item);
        } else {
            self.log.push(item);
            let mut insert_number = self.log.len() as OpNumber + 1;
            while let Some(item) = self.reorder_log.remove(&insert_number) {
                self.log.push(item);
                insert_number += 1;
            }
        }

        if let Some(mut prepare_list) = self.reorder_prepare.remove(&op_number) {
            while !self.prepared(op_number) {
                if let Some(prepare) = prepare_list.pop() {
                    if prepare.digest == digest {
                        self.insert_prepare(&*prepare, prepare.signed_message());
                    }
                } else {
                    break;
                }
            }
        }
        if let Some(mut commit_list) = self.reorder_commit.remove(&op_number) {
            while !self.committed(op_number) {
                if let Some(commit) = commit_list.pop() {
                    if commit.digest == digest {
                        self.insert_commit(&*commit);
                    }
                } else {
                    break;
                }
            }
        }
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
        if self.is_primary() {
            warn!("primary receive pre-prepare");
            return;
        }

        if self.log_item(message.op_number).is_some() {
            return;
        }

        self.insert_log_item(LogItem {
            view_number: message.view_number,
            op_number: message.op_number,
            digest: message.digest,
            batch,
            pre_prepare: message.signed_message().clone(),
            prepare_quorum: HashMap::new(),
            committed: false,
        });

        let prepare = message::Prepare {
            view_number: self.view_number,
            op_number: message.op_number,
            digest: message.digest,
            replica_id: self.id,
        };

        let prepared = self.prepared(message.op_number);
        self.submit.stateless(move |replica| {
            let signed_prepare =
                SignedMessage::sign(prepare.clone(), replica.config.signing_key(replica));
            replica.transport.send_message_to_all(
                replica,
                replica.config.replica(..),
                serialize(ToReplica::Prepare(signed_prepare.clone())),
            );

            // shortcut here to avoid submit no-op task
            if !prepared {
                replica.submit.stateful(move |replica| {
                    replica.insert_prepare(&prepare, &signed_prepare);
                });
            }
        });

        if prepared {
            self.send_commit(message.op_number, message.digest);
        }
    }

    fn handle_prepare(&mut self, _remote: T::Address, message: VerifiedMessage<message::Prepare>) {
        if message.view_number < self.view_number {
            return;
        }
        if message.view_number > self.view_number {
            // TODO state transfer
            return;
        }

        let item = if let Some(item) = self.log_item(message.op_number) {
            item
        } else {
            info!("no log item match prepare {}", message.op_number);
            self.reorder_prepare
                .entry(message.op_number)
                .or_default()
                .push(message);
            return;
        };

        if message.digest != item.digest {
            return;
        }

        if !self.prepared(message.op_number) {
            self.insert_prepare(&*message, message.signed_message());
        }
    }

    fn insert_prepare(
        &mut self,
        prepare: &message::Prepare,
        signed_message: &SignedMessage<message::Prepare>,
    ) {
        self.log_item_mut(prepare.op_number)
            .unwrap()
            .prepare_quorum
            .insert(prepare.replica_id, signed_message.clone());

        if self.prepared(prepare.op_number) {
            debug!("prepared");
            self.send_commit(prepare.op_number, prepare.digest);
        }
    }

    fn send_commit(&mut self, op_number: OpNumber, digest: Digest) {
        let commit = message::Commit {
            view_number: self.view_number,
            op_number,
            digest,
            replica_id: self.id,
        };

        self.submit.stateless({
            let commit = commit.clone();
            move |replica| {
                replica.transport.send_message_to_all(
                    replica,
                    replica.config.replica(..),
                    serialize(ToReplica::Commit(SignedMessage::sign(
                        commit,
                        replica.config.signing_key(replica),
                    ))),
                )
            }
        });

        // assert not committed by now?
        self.insert_commit(&commit);
    }

    fn handle_commit(&mut self, _remote: T::Address, message: VerifiedMessage<message::Commit>) {
        if message.view_number < self.view_number {
            return;
        }
        if message.view_number > self.view_number {
            // TODO state transfer
            return;
        }

        let item = if let Some(item) = self.log_item(message.op_number) {
            item
        } else {
            info!("no log item match commit {}", message.op_number);
            self.reorder_commit
                .entry(message.op_number)
                .or_default()
                .push(message);
            return;
        };

        if message.digest != item.digest {
            return;
        }

        if !self.committed(message.op_number) {
            self.insert_commit(&*message);
        }
    }

    fn insert_commit(&mut self, commit: &message::Commit) {
        self.commit_quorum
            .entry(commit.op_number)
            .or_default()
            .insert(commit.replica_id);

        if self.committed(commit.op_number) {
            debug!("committed");

            self.log_item_mut(commit.op_number).unwrap().committed = true;
            self.execute_committed();

            if self.is_primary() {
                if self.request_buffer.len() >= self.batch_size
                    || (self.adaptive_batching && !self.request_buffer.is_empty())
                {
                    self.close_batch();
                }
            }
        }
    }

    fn execute_committed(&mut self) {
        while let Some(item) = self.log.get(self.commit_number as usize) {
            assert_eq!(item.op_number, self.commit_number + 1);
            if !item.committed {
                break;
            }

            let op_number = item.op_number;
            // why have to clone?
            for (i, request) in item.batch.clone().into_iter().enumerate() {
                let op_number = op_number * self.batch_size as OpNumber + i as OpNumber;
                let result = self.app.execute(op_number, request.op);
                let reply = message::Reply {
                    view_number: self.view_number,
                    request_number: request.request_number,
                    client_id: request.client_id,
                    replica_id: self.id,
                    result,
                };
                self.submit.stateless(move |replica| {
                    let reply = SignedMessage::sign(reply, replica.config.signing_key(replica));
                    replica.submit.stateful(move |replica| {
                        replica.client_table.insert(
                            request.client_id,
                            (request.request_number, Some(reply.clone())),
                        );
                        if let Some(remote) = replica.route_table.get(&request.client_id) {
                            replica
                                .transport
                                .send_message(replica, remote, serialize(reply));
                        } else {
                            debug!("no route record, skip reply");
                        }
                    });
                });
            }

            self.commit_number += 1;
        }
    }
}
