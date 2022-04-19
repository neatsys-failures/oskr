use std::{collections::HashMap, sync::Arc};

use bincode::Options;
use sha2::{Digest as _, Sha256};
use tracing::{debug, info, warn};

use crate::{
    common::{
        deserialize, serialize, signed::VerifiedMessage, ClientId, Config, Digest, OpNumber,
        ReplicaId, RequestNumber, SignedMessage, ViewNumber,
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
    // it is also the last op number that can be (and will be) speculative executed
    history_digest: Digest,
    commit_number: OpNumber, // last stable checkpoint up to
    history: Vec<LogItem>,
    client_table: HashMap<ClientId, (RequestNumber, ToClient)>,
    checkpoint_quorum:
        HashMap<(OpNumber, Digest), HashMap<ReplicaId, SignedMessage<message::Checkpoint>>>,

    request_buffer: Vec<message::Request>,
    // op number => (item, batch digest, order request)
    // too lazy to define new type :)
    reorder_history: HashMap<OpNumber, (LogItem, Digest, SignedMessage<message::OrderRequest>)>,
    route_table: HashMap<ClientId, T::Address>,

    shared: Arc<Shared<T>>,
}

struct LogItem {
    view_number: ViewNumber,
    op_number: OpNumber,
    batch: Vec<message::Request>,
    history_digest: Digest, // hash(batch, previous history digest), used in checkpoint
                            // do we need order request here?
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
            history_digest: Digest::default(),
            commit_number: 0,
            history: Vec::new(),
            client_table: HashMap::new(),
            checkpoint_quorum: HashMap::new(),
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
                return;
            }
            Ok(ToReplica::OrderRequest(order_request, batch)) => {
                let verifying_key = if let Some(key) = self.config.verifying_key(&remote) {
                    key
                } else {
                    warn!("order request without identity");
                    return;
                };
                let order_request = if let Ok(order_request) = order_request.verify(verifying_key) {
                    order_request
                } else {
                    warn!("fail to verify order request");
                    return;
                };
                if Digest::from(Sha256::digest(
                    bincode::options().serialize(&batch).unwrap(),
                )) != order_request.digest
                {
                    warn!("order request digest mismatch");
                    return;
                }
                self.submit
                    .stateful(move |state| state.handle_order_request(order_request, batch));
                return;
            }
            Ok(ToReplica::Commit(commit)) => {
                let mut certification = commit.certification.into_iter();
                let (replica_id, sample_response) = certification.next().unwrap();
                let sample_response = if let Ok(response) = sample_response.verify(
                    self.config
                        .verifying_key(self.config.replica(replica_id))
                        .unwrap(),
                ) {
                    response
                } else {
                    warn!("failed to verify commit certification");
                    return;
                };
                let mut certification: HashMap<_, _> = certification
                    .map_while(|(replica_id, speculative_response)| {
                        if let Ok(speculative_response) = speculative_response.verify(
                            self.config
                                .verifying_key(self.config.replica(replica_id))
                                .as_ref()
                                .unwrap(),
                        ) {
                            if (
                                speculative_response.view_number,
                                speculative_response.op_number,
                                speculative_response.digest,
                                speculative_response.history_digest,
                            ) == (
                                sample_response.view_number,
                                sample_response.op_number,
                                sample_response.digest,
                                sample_response.history_digest,
                            ) {
                                Some((replica_id, speculative_response))
                            } else {
                                None
                            }
                        } else {
                            warn!("failed to verify commit certification");
                            None
                        }
                    })
                    .collect();
                certification.insert(replica_id, sample_response);
                if certification.len() < 2 * self.config.f + 1 {
                    warn!("commit certification has no matching quorum");
                    return;
                }
                let client_id = commit.client_id;
                self.submit
                    .stateful(move |state| state.handle_commit(remote, (client_id, certification)));
                return;
            }
            Ok(ToReplica::Checkpoint(checkpoint)) => {
                let verifying_key = if let Some(key) = self.config.verifying_key(&remote) {
                    key
                } else {
                    warn!("checkpoint without identity");
                    return;
                };
                let checkpoint = if let Ok(checkpoint) = checkpoint.verify(verifying_key) {
                    checkpoint
                } else {
                    warn!("fail to verify checkpoint");
                    return;
                };
                self.submit
                    .stateful(move |state| state.handle_checkpoint(remote, checkpoint));
                return;
            }
            _ => {}
        }
        warn!("fail to handle received buffer");
    }
}
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    // the interval per request not per op number, i.e. divided by batch size
    // when used. this enable checkpoint frequency only be proportional to
    // overall throughput, not related to selected batch size, which should be
    // desired
    // for benchmark on my platform maximum throughput is about 100-200K for
    // 1-batch and 200K-1000K for 100-batch. a 10K checkpoint interval means
    // several tens of checkpoint broadcast per second, and should be a reasonable
    // setup
    const CHECKPOINT_INTERVAL: usize = 10000;

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
        while self.request_buffer.len() >= self.batch_size
            && self.op_number
                < self.commit_number + 2 * (Self::CHECKPOINT_INTERVAL / self.batch_size) as OpNumber
        {
            self.close_batch();
        }
    }

    fn close_batch(&mut self) {
        assert!(self.config.view_primary(self.view_number) == self.id);
        let batch = ..self.batch_size.min(self.request_buffer.len());
        let batch: Vec<_> = self.request_buffer.drain(batch).collect();
        self.op_number += 1;

        // to ensure history digest to be correct it has to be updated in
        // stateful path (and also be checked in stateful path below),
        // because the history digest is chained, which means there is direct
        // data dependency between consecutive history log items.
        // so it has to be sequential, either in stateful path or in stateless
        // path with barrier, which makes less performance difference but much
        // more complicated
        // it is tested in benchmark that this hashing does not affect
        // performance at all
        let digest = Sha256::digest(bincode::options().serialize(&batch).unwrap()).into();
        self.history_digest = Sha256::new()
            .chain_update(&self.history_digest)
            .chain_update(&digest)
            .finalize()
            .into();
        let order_request = message::OrderRequest {
            view_number: self.view_number,
            op_number: self.op_number,
            history_digest: self.history_digest,
            digest,
        };

        self.submit.stateless(move |shared| {
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
                    order_request.digest,
                    &signed,
                );
            });
        });
    }

    fn speculative_execute(
        &mut self,
        item: LogItem,
        digest: Digest,
        order_request: &SignedMessage<message::OrderRequest>,
    ) {
        if item.op_number as usize != self.history.len() + 1 {
            // this reordering happens too much on fast path so logging affect
            // performance very much
            debug!("reorder history: op number = {}", item.op_number);
            // here, it is possible that the op number already present in the
            // reorder history (reorder future maybe?), and get replaced
            // because of the prefix gap we have no way to determine which item
            // is the correct one (if they are different)
            // and there is chance to discard the correct one, keep the faulty
            // one, which cannot pass the history digest check below, and
            // trigger a state transfer ("fill hole") slow path
            // however, this is still good enough, since the Zyzzyva paper, we
            // are not required to handle reordering at all, just state transfer
            // whenever it is out of order
            self.reorder_history
                .insert(item.op_number, (item, digest, order_request.clone()));
            return;
        }

        let history_digest = self
            .history
            .last()
            .map(|item| item.history_digest)
            .unwrap_or_default();
        // we expect this to be blazing fast
        let history_digest: Digest = Sha256::new()
            .chain_update(history_digest)
            .chain_update(digest)
            .finalize()
            .into();
        if item.history_digest != history_digest {
            warn!("history digest mismatch: op number = {}", item.op_number);
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

        let item_number = item.op_number;
        let history_digest = item.history_digest;
        self.history.push(item);

        if let Some((item, digest, order_request)) = self.reorder_history.remove(&(item_number + 1))
        {
            self.speculative_execute(item, digest, &order_request);
        }

        if item_number % (Self::CHECKPOINT_INTERVAL / self.batch_size) as OpNumber == 0 {
            debug!("checkpoint op number {}", item_number);
            let checkpoint = message::Checkpoint {
                replica_id: self.id,
                op_number: item_number,
                history_digest,
            };
            self.submit.stateless(move |shared| {
                let signed =
                    SignedMessage::sign(checkpoint.clone(), shared.config.signing_key(shared));
                shared.transport.send_message_to_all(
                    shared,
                    shared.config.replica(..),
                    serialize(ToReplica::Checkpoint(signed.clone())),
                );
                shared.submit.stateful(move |state| {
                    state.insert_checkpoint(&checkpoint, &signed);
                });
            });
        }
    }

    fn handle_order_request(
        &mut self,
        order_request: VerifiedMessage<message::OrderRequest>,
        batch: Vec<message::Request>,
    ) {
        if order_request.view_number < self.view_number {
            return;
        }
        if order_request.view_number > self.view_number {
            todo!("state transfer");
        }

        self.speculative_execute(
            LogItem {
                view_number: self.view_number,
                op_number: order_request.op_number,
                batch,
                history_digest: order_request.history_digest,
            },
            order_request.digest,
            order_request.signed_message(),
        );
    }

    fn handle_checkpoint(
        &mut self,
        _remote: T::Address,
        checkpoint: VerifiedMessage<message::Checkpoint>,
    ) {
        if checkpoint.op_number <= self.commit_number {
            return;
        }

        self.insert_checkpoint(&*checkpoint, checkpoint.signed_message());
    }

    fn insert_checkpoint(
        &mut self,
        checkpoint: &message::Checkpoint,
        signed: &SignedMessage<message::Checkpoint>,
    ) {
        // there could be some case when next checkpoint get stablized first
        // assert!(self.commit_number < checkpoint.op_number);

        self.checkpoint_quorum
            .entry((checkpoint.op_number, checkpoint.history_digest))
            .or_default()
            .insert(checkpoint.replica_id, signed.clone());
        if let Some(item) = self.history.get(checkpoint.op_number as usize - 1) {
            assert_eq!(item.op_number, checkpoint.op_number);
            if self
                .checkpoint_quorum
                .get(&(checkpoint.op_number, item.history_digest))
                .map(|quorum| quorum.len())
                .unwrap_or(0)
                >= self.config.f + 1
            {
                debug!("checkpoint commit {}", checkpoint.op_number);
                // TODO garbage collect

                self.app.commit(checkpoint.op_number);
                self.commit_number = checkpoint.op_number;

                while self.request_buffer.len() >= self.batch_size
                    && self.op_number
                        < self.commit_number
                            + 2 * (Self::CHECKPOINT_INTERVAL / self.batch_size) as OpNumber
                {
                    self.close_batch();
                }
            }
        }
    }

    fn handle_commit(
        &mut self,
        remote: T::Address,
        // group into single parameter, keep the uniform shape of handle_* methods
        verified_commit: (
            ClientId,
            HashMap<ReplicaId, VerifiedMessage<message::SpeculativeResponse>>,
        ),
    ) {
        let (client_id, certification) = verified_commit;
        let (_, response) = certification.iter().next().unwrap();
        if response.view_number != self.view_number {
            return;
        }
        // TODO check match local history & insert certification
        let local_commit = message::LocalCommit {
            view_number: self.view_number,
            digest: response.digest,
            history_digest: response.history_digest,
            replica_id: self.id,
            client_id,
        };
        self.submit.stateless(move |shared| {
            shared.transport.send_message(
                shared,
                &remote,
                serialize(ToClient::LocalCommit(SignedMessage::sign(
                    local_commit,
                    shared.config.signing_key(shared),
                ))),
            )
        });
    }
}
