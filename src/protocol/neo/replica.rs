use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use tracing::{debug, info, warn};

use crate::{
    common::{
        serialize, ClientId, Config, Digest, OpNumber, ReplicaId, RequestNumber, SignedMessage,
    },
    facade::{App, Receiver, Transport, TxAgent},
    protocol::neo::message::{self, OrderedMulticast, VerifiedOrderedMulticast},
    stage::{Handle, State, StatefulContext, StatelessContext},
};

use super::message::MulticastVerifyingKey;

pub struct Replica<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    app: Box<dyn App + Send>,
    batch_size: usize,
    check_equivocation: bool,
    op_number: OpNumber,
    log: Vec<VerifiedOrderedMulticast<message::Request>>,
    chain_request: HashMap<Digest, VerifiedOrderedMulticast<message::Request>>,
    reorder_request: HashMap<OpNumber, VerifiedOrderedMulticast<message::Request>>,
    client_table: HashMap<ClientId, (RequestNumber, Option<SignedMessage<message::Reply>>)>,
    route_table: HashMap<ClientId, T::Address>,
    shared: Arc<Shared<T>>,

    signed_count: u32,
    unsigned_count: u32,
}

pub struct Shared<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    multicast_key: MulticastVerifyingKey,
}

impl<T: Transport> State for Replica<T> {
    type Shared = Arc<Shared<T>>;
    fn shared(&self) -> Self::Shared {
        self.shared.clone()
    }
}

impl<T: Transport> Receiver<T> for StatefulContext<'_, Replica<T>> {
    fn get_address(&self) -> &T::Address {
        &self.config.replica(self.id)
    }
}

impl<T: Transport> Receiver<T> for StatelessContext<Replica<T>> {
    fn get_address(&self) -> &T::Address {
        &self.config.replica(self.id)
    }
}

impl<T: Transport> Replica<T> {
    pub fn register_new(
        config: Config<T>,
        transport: &mut T,
        replica_id: ReplicaId,
        app: impl App + Send + 'static,
        batch_size: usize,
        multicast_key: MulticastVerifyingKey,
        check_equivocation: bool,
    ) -> Handle<Self> {
        let state = Handle::from(Self {
            config: config.clone(),
            transport: transport.tx_agent(),
            id: replica_id,
            app: Box::new(app),
            batch_size,
            check_equivocation,
            op_number: 0,
            log: Vec::new(),
            chain_request: HashMap::new(),
            reorder_request: HashMap::new(),
            client_table: HashMap::new(),
            route_table: HashMap::new(),
            shared: Arc::new(Shared {
                config,
                transport: transport.tx_agent(),
                id: replica_id,
                multicast_key,
            }),

            signed_count: 0,
            unsigned_count: 0,
        });
        state.with_stateful(|state| {
            let submit = state.submit.clone();
            transport.register(state, move |remote, buffer| {
                submit.stateless(move |shared| shared.receive_buffer(remote, buffer))
            });
            let submit = state.submit.clone();
            transport.register_multicast(move |remote, buffer| {
                submit.stateless(move |shared| shared.receive_multicast_buffer(remote, buffer))
            });
        });
        state
    }
}

static SEQUENCE_START: AtomicU32 = AtomicU32::new(u32::MAX);
impl<T: Transport> StatelessContext<Replica<T>> {
    fn receive_multicast_buffer(&self, remote: T::Address, buffer: T::RxBuffer) {
        let ordered_multicast: OrderedMulticast<message::Request> =
            OrderedMulticast::parse(buffer.as_ref());
        // TODO
        let verified = if let Ok(verified) = ordered_multicast.verify(&self.multicast_key) {
            verified
        } else {
            warn!("failed to verify multicast");
            return;
        };
        SEQUENCE_START.fetch_min(verified.meta.sequence_number, Ordering::SeqCst);
        self.submit
            .stateful(move |state| state.handle_request(remote, verified));
    }

    fn receive_buffer(&self, remote: T::Address, buffer: T::RxBuffer) {
        //
    }
}

impl<T: Transport> StatefulContext<'_, Replica<T>> {
    fn handle_request(
        &mut self,
        remote: T::Address,
        request: VerifiedOrderedMulticast<message::Request>,
    ) {
        self.route_table.insert(request.client_id, remote);

        if !request.meta.is_signed() {
            debug!("insert chain");
            if self
                .chain_request
                .insert(request.meta.chain_hash, request)
                .is_some()
            {
                warn!("duplicated chain hash");
            }
            self.unsigned_count += 1;
            return;
        }
        debug!("insert signed");
        self.signed_count += 1;

        if self.check_equivocation {
            todo!()
        }

        self.insert_chain(&request.meta.chain_hash);
        self.insert_request(request);
    }

    fn insert_chain(&mut self, chain_hash: &Digest) {
        while let Some(request) = self.chain_request.remove(chain_hash) {
            self.insert_chain(&request.meta.chain_hash);
            self.insert_request(request);
        }
    }

    fn insert_request(&mut self, request: VerifiedOrderedMulticast<message::Request>) {
        let sequence_number = request.meta.sequence_number;
        let op_number = request.meta.sequence_number - SEQUENCE_START.load(Ordering::SeqCst) + 1;
        debug!(
            "insert request: sequence {} on {}",
            op_number, self.op_number
        );
        if op_number != self.op_number + 1 {
            if self.reorder_request.insert(op_number, request).is_some() {
                warn!("duplicated sequence number {sequence_number}");
            }
            return;
        }
        self.insert_log(request);
        let mut insert_number = self.op_number + 1;
        while let Some(request) = self.reorder_request.remove(&insert_number) {
            self.insert_log(request);
            insert_number = self.op_number + 1;
        }
    }

    fn insert_log(&mut self, verified: VerifiedOrderedMulticast<message::Request>) {
        // assert_eq!(verified.meta.sequence_number, self.op_number + 1);
        self.op_number += 1;
        let request = (*verified).clone();
        self.log.push(verified);

        // execution
        let client_id = request.client_id;
        let remote = self.route_table[&client_id].clone();
        if let Some((request_number, reply)) = self.client_table.get(&request.client_id) {
            if *request_number > request.request_number {
                return;
            }
            if *request_number == request.request_number {
                if let Some(reply) = reply {
                    self.transport.send_message(self, &remote, serialize(reply));
                }
                return;
            }
        }
        let op_number = self.op_number;
        let result = self.app.execute(op_number, request.op);
        let request_number = request.request_number;
        let reply = message::Reply {
            view_number: 0, // TODO
            replica_id: self.id,
            op_number,
            log_hash: Digest::default(), // TODO
            request_number,
            result,
        };
        if let Some((previous_number, _)) =
            self.client_table.insert(client_id, (request_number, None))
        {
            assert!(previous_number < request_number);
        }
        self.submit.stateless(move |shared| {
            let signed = SignedMessage::sign(reply, shared.config.signing_key(shared));
            shared
                .transport
                .send_message(shared, &remote, serialize(signed.clone()));
            shared.submit.stateful(move |state| {
                let (current_request, reply) = state.client_table.get_mut(&client_id).unwrap();
                if *current_request == request_number {
                    *reply = Some(signed);
                }
            });
        });
    }
}

impl<T: Transport> Drop for Replica<T> {
    fn drop(&mut self) {
        info!(
            "signed/unsigned: {}/{}",
            self.signed_count, self.unsigned_count
        );
        info!(
            "chain/reorder: {}/{}",
            self.chain_request.len(),
            self.reorder_request.len()
        );
    }
}
