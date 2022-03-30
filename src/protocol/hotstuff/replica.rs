use std::{borrow::Borrow, collections::HashMap, ops::Index, sync::Arc};

use tracing::{debug, trace, warn};

use crate::{
    common::{
        deserialize, serialize, signed::VerifiedMessage, ClientId, Digest, OpNumber, ReplicaId,
        RequestNumber, SignedMessage, SigningKey, VerifyingKey, ViewNumber,
    },
    facade::{App, Receiver, Transport, TxAgent},
    protocol::hotstuff::message::{self, GenericNode, QuorumCertification, ToReplica, GENESIS},
    stage::{Handle, State, StatefulContext, StatelessContext},
};

// Check message module for modification in messages format
// Currently this implementation contains one fixed pacemaker. I don't see a
// meaningful reason to implementation more pacemakers. It could be extracted
// as trait and be replaciable, if necessary.
// The pacemaker has such behavior:
// * Round-robin select leader base on view number, and increase view number
//   on every synchronized new view.
// * Preempt leader if either: client request is not proposed soon enough, or
//   leader's proposal is not justified (and the justify is received) soon
//   enough
// * Close batch, i.e. `on_beat`, only after collected parent block's QC. In
//   adaptive batching configuration new block is proposed immediately as long
//   as it's not empty, otherwise new block is proposed after receiving enough
//   client requests to fill the batch.
// * In adaptive batching configuration, extra empty-block proposal is sent to
//   drive non-empty blocks which is not committed yet to progress. In normal
//   configuration these blocks will not reach commit point until enough number
//   of following client requests be proposed.

pub struct Replica<T: Transport> {
    address: T::Address,
    transport: T::TxAgent,
    id: ReplicaId,
    batch_size: usize,
    adaptive_batching: bool,

    // although not present in event-driven HotStuff, current view must be kept,
    // so we have something to fill message field
    // the paper really didn't tell when to update, so probably in pacemaker, i
    // guess
    current_view: ViewNumber,
    vote_table: HashMap<Digest, HashMap<ReplicaId, SignedMessage<message::VoteGeneric>>>,
    voted_height: OpNumber,
    block_locked: Digest,
    block_executed: Digest,
    // pacemaker states
    qc_high: QuorumCertification,
    block_leaf: Digest,
    will_beat: bool,

    client_table: HashMap<ClientId, (RequestNumber, Option<SignedMessage<message::Reply>>)>,
    log: HashMap<Digest, GenericNode>,
    request_buffer: Vec<message::Request>,

    app: Box<dyn App + Send>,
    route_table: HashMap<ClientId, T::Address>,

    shared: Arc<Shared<T>>,
}

pub struct Shared<T: Transport> {
    address: T::Address,
    transport: T::TxAgent,

    signing_key: SigningKey,
    verifying_key: HashMap<T::Address, VerifyingKey>,
}

impl<T: Transport> Replica<T> {
    fn extend(&self, block: &GenericNode, ancestor: &Digest) -> bool {
        if &block.parent == ancestor {
            return true;
        }
        if let Some(parent) = self.log.get(&block.parent) {
            self.extend(parent, ancestor)
        } else {
            false
        }
    }

    fn insert_log(&mut self, digest: Digest, block: GenericNode) {
        for request in &block.command {
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

        self.log.insert(digest, block);
    }
}

impl<D: Borrow<Digest>, T: Transport> Index<D> for Replica<T> {
    type Output = GenericNode;
    fn index(&self, index: D) -> &Self::Output {
        self.log.get(index.borrow()).unwrap()
    }
}

impl<T: Transport> State for Replica<T> {
    type Shared = Arc<Shared<T>>;
    fn shared(&self) -> Self::Shared {
        self.shared.clone()
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

impl<T: Transport> Replica<T> {
    pub fn register_new(
        transport: &mut T,
        replica_id: ReplicaId,
        app: impl App + Send + 'static,
        batch_size: usize,
        adaptive_batching: bool,
    ) -> Handle<Self> {
        assert!(transport.tx_agent().config().replica_address.len() > 1); // TODO

        let log = [(GENESIS.justify.node, GENESIS.clone())]
            .into_iter()
            .collect();

        let address = transport.tx_agent().config().replica_address[replica_id as usize].clone();
        let replica: Handle<_> = Self {
            address: address.clone(),
            transport: transport.tx_agent(),
            id: replica_id,
            batch_size,
            adaptive_batching,
            current_view: 0,
            vote_table: HashMap::new(),
            voted_height: 0,
            block_locked: GENESIS.justify.node,
            block_executed: GENESIS.justify.node,
            block_leaf: GENESIS.justify.node,
            qc_high: GENESIS.justify.clone(),
            will_beat: true,
            client_table: HashMap::new(),
            log,
            request_buffer: Vec::new(),
            app: Box::new(app),
            route_table: HashMap::new(),
            shared: Arc::new(Shared {
                signing_key: transport.tx_agent().config().signing_key[&address].clone(),
                verifying_key: transport.tx_agent().config().verifying_key(),
                address,
                transport: transport.tx_agent(),
            }),
        }
        .into();

        replica.with_stateful(|replica| {
            let submit = replica.submit.clone();
            transport.register(replica, move |remote, buffer| {
                submit.stateless(move |replica| replica.receive_buffer(remote, buffer));
            });
        });

        replica
    }
}

// "algorithm 4" in HotStuff paper
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    // block3: b*, block2: b'', block1: b', block0: b
    fn update(&mut self, block3: &Digest) {
        trace!("update");
        let block2 = &if let Some(block3) = self.log.get(block3) {
            block3.justify.node
        } else {
            unreachable!("block3 should always present");
        };
        let block1 = &if let Some(block2) = self.log.get(block2) {
            block2.justify.node
        } else {
            return;
        };
        let block0 = &if let Some(block1) = self.log.get(block1) {
            block1.justify.node
        } else {
            return;
        };

        let commit_block1 = self[block1].height > self[self.block_locked].height;
        let decide_block0 = self[block2].parent == *block1 && self[block1].parent == *block0;

        self.update_qc_high(self[block3].justify.clone());
        if commit_block1 {
            self.block_locked = *block1;
        }
        if decide_block0 {
            trace!(
                "on commit: block = {}",
                block0
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join("")
            );
            self.on_commit(block0);
            self.block_executed = *block0;
        }
    }

    fn on_commit(&mut self, block: &Digest) {
        if !self.log.contains_key(block) {
            todo!("state transfer on execution gap");
        }
        if self[self.block_executed].height < self[block].height {
            self.on_commit(&{ self[block].parent });
            self.execute(block);
        }
    }
}
impl<T: Transport> StatelessContext<Replica<T>> {
    fn on_receive_proposal(&self, message: message::Generic) {
        let block_new = message.node;
        let digest = block_new.digest();
        self.submit.stateful(move |replica| {
            let safe_node = if replica.extend(&block_new, &replica.block_locked) {
                true
            } else if let Some(node) = replica.log.get(&block_new.justify.node) {
                node.height > replica[replica.block_locked].height
            } else {
                false
            };
            if block_new.height > replica.voted_height && safe_node {
                replica.voted_height = block_new.height;
                let vote_generic = message::VoteGeneric {
                    view_number: replica.current_view,
                    node: digest,
                    replica_id: replica.id,
                };

                let primary = replica.get_leader();
                replica.submit.stateless(move |replica| {
                    let signed = SignedMessage::sign(vote_generic, &replica.signing_key);
                    replica.transport.send_message_to_replica(
                        replica,
                        primary,
                        serialize(ToReplica::VoteGeneric(signed)),
                    );
                });
            }

            replica.insert_log(digest, block_new);
            replica.update(&digest);
        });
    }
}
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    fn on_receive_vote(&mut self, message: VerifiedMessage<message::VoteGeneric>) {
        self.insert_vote(
            message.node,
            message.replica_id,
            message.signed_message().clone(),
        );
    }

    fn insert_vote(
        &mut self,
        node: Digest,
        replica_id: ReplicaId,
        vote: SignedMessage<message::VoteGeneric>,
    ) {
        let quorum = self.vote_table.entry(node).or_default();
        quorum.insert(replica_id, vote);
        let vote_count = quorum.len();
        if vote_count
            >= self.transport.config().replica_address.len() - self.transport.config().n_fault
        {
            let qc = QuorumCertification {
                view_number: self.current_view,
                node,
                signature: self
                    .vote_table
                    .get(&node)
                    .unwrap()
                    .clone()
                    .into_iter()
                    .collect(),
            };
            self.update_qc_high(qc);
        }
    }

    // b_leaf and qc_high are read from state
    // returned b_new has to be delivered in CPS, and I blame HotStuff for that
    fn on_propose(
        &mut self,
        command: Vec<message::Request>,
        k: impl for<'a> FnOnce(&mut StatefulContext<'a, Replica<T>>, Digest) + Send + 'static,
    ) {
        let block_leaf = self.block_leaf;
        let qc_high = self.qc_high.clone();
        let height = self[&self.block_leaf].height + 1;
        let view_number = self.current_view;
        let replica_id = self.id;
        self.submit.stateless(move |replica| {
            let block_new = GenericNode::create_leaf(&block_leaf, command, qc_high, height);
            let generic = message::Generic {
                view_number,
                node: block_new.clone(),
            };
            replica
                .transport
                .send_message_to_all(replica, serialize(ToReplica::Generic(generic)));

            let digest = block_new.digest();
            let vote_generic = SignedMessage::sign(
                message::VoteGeneric {
                    view_number,
                    node: digest,
                    replica_id,
                },
                &replica.signing_key,
            );

            replica.submit.stateful(move |replica| {
                replica.insert_log(digest, block_new);
                k(replica, digest);

                // propose locally
                replica.update(&digest);
                // vote locally
                replica.insert_vote(digest, replica.id, vote_generic);
            });
        });
    }
}
// "algorithm 5" in HotStuff paper
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    fn get_leader(&self) -> ReplicaId {
        self.transport.config().view_primary(self.current_view)
    }

    fn update_qc_high(&mut self, qc_high1: QuorumCertification) {
        if self[&qc_high1.node].height > self[&self.qc_high.node].height {
            self.block_leaf = qc_high1.node;
            self.qc_high = qc_high1;

            let on_beat = if !self.adaptive_batching {
                self.request_buffer.len() >= self.batch_size
            } else if self.request_buffer.len() > 0 {
                true
            } else {
                let mut not_committed = self.block_leaf;
                loop {
                    if not_committed == self.block_executed {
                        break false;
                    }
                    if self[not_committed].command.len() > 0 {
                        break true;
                    }
                    not_committed = self[not_committed].parent;
                }
            };
            if on_beat {
                let command = ..self.batch_size.min(self.request_buffer.len());
                let command = self.request_buffer.drain(command).collect();
                self.on_beat(command);
            } else {
                debug!("skip beat");
                self.will_beat = true;
            }
        }
    }

    pub(super) fn on_beat(&mut self, command: Vec<message::Request>) {
        trace!("on beat");
        if self.get_leader() == self.id {
            self.on_propose(command, |replica, block_leaf| {
                replica.block_leaf = block_leaf;
            });
        }
        self.will_beat = false;
    }

    // TODO new view
}

// the other thing to support
impl<T: Transport> StatelessContext<Replica<T>> {
    fn receive_buffer(&self, remote: T::Address, buffer: T::RxBuffer) {
        match deserialize(buffer.as_ref()) {
            Ok(ToReplica::Request(request)) => {
                self.submit
                    .stateful(move |replica| replica.handle_request(remote, request));
                return;
            }
            Ok(ToReplica::Generic(generic)) => {
                let verifying_key = |replica| {
                    &self.verifying_key[&self.transport.config().replica_address[replica as usize]]
                };
                let threshold =
                    self.transport.config().replica_address.len() - self.transport.config().n_fault;
                if generic
                    .node
                    .justify
                    .verify(verifying_key, threshold)
                    .is_err()
                {
                    warn!("failed to verify generic node justify");
                    return;
                }

                self.on_receive_proposal(generic);
                return;
            }
            Ok(ToReplica::VoteGeneric(vote_generic)) => {
                if let Ok(verified) = vote_generic.verify(&self.verifying_key[&remote]) {
                    self.submit.stateful(move |replica| {
                        if verified.view_number == replica.current_view {
                            replica.on_receive_vote(verified);
                        }
                    });
                } else {
                    warn!("failed to verify vote generic");
                }
                return;
            }
            _ => {}
        }
        warn!("failed to deserialize");
    }
}
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    fn handle_request(&mut self, remote: T::Address, message: message::Request) {
        self.route_table.insert(message.client_id, remote.clone());

        if let Some((request_number, reply)) = self.client_table.get(&message.client_id) {
            if *request_number > message.request_number {
                return;
            }
            if *request_number == message.request_number {
                if let Some(reply) = reply {
                    self.transport
                        .send_message(self, &remote, serialize(reply.clone()));
                }
                return;
            }
        }

        if self.get_leader() != self.id {
            return;
        }

        self.request_buffer.push(message);

        if self.will_beat
            && (self.adaptive_batching || self.request_buffer.len() >= self.batch_size)
        {
            let command = ..self.batch_size.min(self.request_buffer.len());
            let command = self.request_buffer.drain(command).collect();
            self.on_beat(command);
        }
    }

    fn execute(&mut self, block: &Digest) {
        for request in self[block].command.clone() {
            if let Some((request_number, reply)) = self.client_table.get(&request.client_id) {
                if *request_number > request.request_number
                    || (*request_number == request.request_number && reply.is_some())
                {
                    continue;
                }
            }

            debug!("execute");
            let result = self.app.execute(request.op.clone());
            let reply = message::Reply {
                request_number: request.request_number,
                result,
                replica_id: self.id,
            };

            let remote = self.route_table.get(&request.client_id).cloned();
            let client_id = request.client_id;
            let request_number = request.request_number;
            self.submit.stateless(move |replica| {
                let signed = SignedMessage::sign(reply, &replica.signing_key);
                if let Some(remote) = remote {
                    replica
                        .transport
                        .send_message(replica, &remote, serialize(&signed));
                } else {
                    warn!("no route record so skip sending reply");
                }
                replica.submit.stateful(move |replica| {
                    replica
                        .client_table
                        .insert(client_id, (request_number, Some(signed)));
                });
            });
        }
    }
}
