use std::{
    collections::HashMap,
    mem,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Once,
    },
};

use sha2::{Digest as _, Sha256};
use tracing::{debug, info, warn};

use crate::{
    common::{
        deserialize, serialize, signed::VerifiedMessage, ClientId, Config, Digest, OpNumber,
        ReplicaId, RequestNumber, SignedMessage, ViewNumber,
    },
    facade::{App, Receiver, Transport, TxAgent},
    protocol::neo::message::{
        self, MulticastVerifyingKey, OrderedMulticast, Status, ToReplica, VerifiedOrderedMulticast,
    },
    stage::{Handle, State, StatefulContext, StatelessContext},
};

pub struct Replica<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    app: Box<dyn App + Send>,
    batch_size: usize,
    check_equivocation: bool,
    view_number: ViewNumber,
    op_number: OpNumber, // the number exposed to app and client
    // using `u32` directly as type, because the number is given by hardware
    // from replica's view, come from nowhere in network
    // so the value width should not be controled by this codebase
    // consider give sequence number a type or type alias
    sequence_number: u32, // speculative number, verified and no gap
    query_number: Option<u32>,
    log: Vec<VerifiedOrderedMulticast<message::Request>>,
    log_hash: Digest,
    received_buffer: HashMap<u32, VerifiedOrderedMulticast<message::Request>>,
    verified_buffer: HashMap<u32, VerifiedOrderedMulticast<message::Request>>,
    confirm_number: u32, // yet to verify and no gap
    // verified locally, so every OrderConfirm whose op_number is lower than
    // confirm_number is already sent
    confirm_digest: Digest,
    confirmed_high: u32, // the OC present in OC table
    ordering_certification_table: HashMap<u32, OrderingCertification>,
    client_table: HashMap<ClientId, (RequestNumber, Option<SignedMessage<message::Reply>>)>,
    route_table: HashMap<ClientId, T::Address>,
    shared: Arc<Shared<T>>,

    // these are u32 just because u32 is fast :)
    signed_count: u32,
    unsigned_count: u32,
    skipped_count: u32,
    queried_count: u32,
}

#[derive(Debug, Clone, Default)]
struct OrderingCertification {
    request: Option<OrderedMulticast<message::Request>>,
    digest: Option<Digest>,
    confirm_table: HashMap<ReplicaId, SignedMessage<message::OrderConfirm>>,
}

pub struct Shared<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    multicast_key: MulticastVerifyingKey,
    skip_size: u32,
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
            view_number: 0,
            op_number: 0,
            sequence_number: 0,
            query_number: None,
            log: Vec::new(),
            log_hash: Digest::default(),
            received_buffer: HashMap::new(),
            verified_buffer: HashMap::new(),
            confirm_number: 0,
            confirm_digest: Digest::default(),
            confirmed_high: if check_equivocation { 0 } else { u32::MAX },
            ordering_certification_table: HashMap::new(),
            client_table: HashMap::new(),
            route_table: HashMap::new(),
            shared: Arc::new(Shared {
                config,
                transport: transport.tx_agent(),
                id: replica_id,
                multicast_key,
                skip_size: batch_size as _,
            }),

            signed_count: 0,
            unsigned_count: 0,
            skipped_count: 0,
            queried_count: 0,
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

// some more local solution?
static SEQUENCE_START: AtomicU32 = AtomicU32::new(u32::MAX);
impl<T: Transport> StatelessContext<Replica<T>> {
    fn receive_multicast_buffer(&self, remote: T::Address, buffer: T::RxBuffer) {
        let ordered_multicast: OrderedMulticast<message::Request> =
            OrderedMulticast::parse(buffer.as_ref());
        SEQUENCE_START.fetch_min(ordered_multicast.sequence_number, Ordering::SeqCst);

        static MAX_SIGNED: AtomicU32 = AtomicU32::new(0);
        let verified = if matches!(self.multicast_key, MulticastVerifyingKey::PublicKey(_))
            // make this dynamically predicate base on system load?
                && MAX_SIGNED.load(Ordering::SeqCst) + self.skip_size / 2
                    > ordered_multicast.sequence_number
        {
            ordered_multicast.skip_verify()
        } else {
            ordered_multicast.verify(&self.multicast_key)
        };

        if let Ok(verified) = verified {
            if verified.status == Status::Signed {
                MAX_SIGNED.fetch_max(verified.meta.sequence_number, Ordering::SeqCst);
            }
            self.submit
                .stateful(move |state| state.handle_request(remote, verified));
        } else {
            warn!("failed to verify multicast");
        }
    }

    fn receive_buffer(&self, remote: T::Address, buffer: T::RxBuffer) {
        match deserialize(buffer.as_ref()) {
            Ok(ToReplica::OrderConfirm(order_confirm)) => {
                let verifying_key = if let Some(key) = self.config.verifying_key(&remote) {
                    key
                } else {
                    warn!("no remote identity for order confirm");
                    return;
                };
                let order_confirm = if let Ok(order_confirm) = order_confirm.verify(verifying_key) {
                    order_confirm
                } else {
                    warn!("failed to verify order confirm");
                    return;
                };
                self.submit
                    .stateful(move |state| state.handle_order_confirm(remote, order_confirm));
            }
            Ok(ToReplica::Query(query)) => self
                .submit
                .stateful(|state| state.handle_query(remote, query)),
            Ok(ToReplica::QueryReply(query_reply)) => {
                let verified = if let Ok(verified) = query_reply.request.skip_verify() {
                    verified
                } else {
                    warn!("failed to verify query reply request");
                    return;
                };
                let query_reply = (query_reply.view_number, verified);
                self.submit
                    .stateful(move |state| state.handle_query_reply(remote, query_reply));
            }
            _ => warn!("failed to parse received buffer"),
        }
    }
}

impl<T: Transport> StatefulContext<'_, Replica<T>> {
    fn handle_request(
        &mut self,
        remote: T::Address,
        request: VerifiedOrderedMulticast<message::Request>,
    ) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            // assert at this point SEQUENCE_START from never go lower any more
            let start = SEQUENCE_START.load(Ordering::SeqCst);
            assert_ne!(start, 0);
            self.sequence_number = start - 1;
            self.confirm_number = start - 1;
            self.confirmed_high = self.confirmed_high.max(start - 1);
        });
        assert!(request.meta.sequence_number > self.sequence_number);

        self.route_table.insert(request.client_id, remote);

        // is this what you expect me to write, Rust?
        if if request.status == Status::Unsigned {
            self.unsigned_count += 1;
            true
        } else if request.status == Status::SkippedSigned {
            self.skipped_count += 1;
            true
        } else {
            false
        } {
            self.insert_chain(request);
            return;
        }

        debug!("insert signed {}", request.meta.sequence_number);
        self.signed_count += 1;

        self.verify_chain(&request.meta);
        self.insert_request(request);
    }

    fn is_confirmed(&self, sequence_number: u32) -> bool {
        if let Some(certification) = self.ordering_certification_table.get(&sequence_number) {
            certification.request.is_some()
                && certification.confirm_table.len() >= 2 * self.config.f + 1
        } else {
            false
        }
    }

    fn insert_chain(&mut self, request: VerifiedOrderedMulticast<message::Request>) {
        // assert!(!request.meta.is_signed());
        let child = if let Some(child) = self
            .verified_buffer
            .get(&(request.meta.sequence_number + 1))
        {
            child
        } else {
            // child not verified yet
            debug!("insert chain {}", request.meta.sequence_number);
            // TODO don't let a faulty chain to cause unnecessary query
            // i.e. store unverified parents like a block chain
            if self
                .received_buffer
                .insert(request.meta.sequence_number, request)
                .is_some()
            {
                warn!("duplicated chain hash");
            }
            return;
        };
        if let Ok(verified) = child.meta.verify_parent(request) {
            if Some(verified.meta.sequence_number) == self.query_number {
                info!("query {:?} fullfiled", self.query_number);
                self.query_number = None;
            }

            self.verify_chain(&verified.meta);
            self.insert_request(verified);
        } else {
            warn!("broken chain");
        }
    }

    fn verify_chain(&mut self, child: &OrderedMulticast<message::Request>) {
        if let Some(request) = self.received_buffer.remove(&(child.sequence_number - 1)) {
            if let Ok(verified) = child.verify_parent(request) {
                self.verify_chain(&verified.meta);
                self.insert_request(verified);
                return;
            } else {
                warn!("broken chain");
            }
        }
        let query_number = child.sequence_number - 1;
        if query_number > self.confirm_number && self.query_number.is_none() {
            self.query_number = Some(query_number);
            self.send_query();
        }
    }

    fn insert_request(&mut self, request: VerifiedOrderedMulticast<message::Request>) {
        assert!(request.status == Status::Signed || request.status == Status::Chained);
        let sequence_number = request.meta.sequence_number;

        // shortcut for no need to insert verified
        if sequence_number == self.sequence_number + 1 && sequence_number <= self.confirmed_high {
            assert_eq!(sequence_number, self.confirm_number + 1); // confirm number should never less than sequence number
            self.confirm_next(&request);
            self.insert_log(request);
            self.flush_verified();
            return;
        }

        if self
            .verified_buffer
            .insert(sequence_number, request)
            .is_some()
        {
            warn!("duplicated sequence number {sequence_number}");
        }
        self.flush_verified();
    }

    fn confirm_next(&mut self, verified: &VerifiedOrderedMulticast<message::Request>) {
        assert_eq!(verified.meta.sequence_number, self.confirm_number + 1);
        self.confirm_number += 1;

        if !self.check_equivocation {
            return;
        }

        // hope this to be fast...
        self.confirm_digest = Sha256::new()
            .chain_update(&self.confirm_digest)
            .chain_update(verified.client_id)
            .chain_update(verified.request_number.to_le_bytes())
            .chain_update(&verified.op)
            .finalize()
            .into();

        if self.confirm_number % self.batch_size as u32 != 0 {
            return;
        }

        let confirm_number = self.confirm_number;
        let confirm_digest = mem::take(&mut self.confirm_digest);
        let certification = self
            .ordering_certification_table
            .entry(confirm_number)
            .or_default();
        certification.request = Some(verified.meta.clone());
        if let Some(previous_digest) = certification.digest {
            if previous_digest != confirm_digest {
                warn!("order certification digest mismatch local one");
                // everything previously collected is diveraged
                // local information is always prioritized
                certification.confirm_table.clear();
            }
        }
        certification.digest = Some(confirm_digest);

        let order_confirm = message::OrderConfirm {
            view_number: self.view_number,
            replica_id: self.id,
            sequence_number: confirm_number,
            digest: confirm_digest,
        };
        self.submit.stateless(move |shared| {
            let signed =
                SignedMessage::sign(order_confirm.clone(), shared.config.signing_key(shared));
            debug!("send order confirm {:?}", order_confirm);
            shared.transport.send_message_to_all(
                shared,
                shared.config.replica(..),
                serialize(ToReplica::OrderConfirm(signed.clone())),
            );
            shared.submit.stateful(move |state| {
                if state.view_number == order_confirm.view_number
                    && order_confirm.sequence_number > state.confirmed_high
                {
                    state.insert_order_confirm(&order_confirm, &signed);
                }
            });
        });
    }

    fn insert_order_confirm(
        &mut self,
        order_confirm: &message::OrderConfirm,
        signed: &SignedMessage<message::OrderConfirm>,
    ) {
        debug!("insert {:?}", order_confirm);
        assert_eq!(order_confirm.view_number, self.view_number);
        assert!(self.confirmed_high < order_confirm.sequence_number);
        let certification = self
            .ordering_certification_table
            .entry(order_confirm.sequence_number)
            .or_default();
        if let Some(digest) = certification.digest {
            if digest != order_confirm.digest {
                warn!("order confirm digest mismatch");
                return;
            }
        } else {
            assert!(certification.confirm_table.is_empty());
            certification.digest = Some(order_confirm.digest);
        }
        certification
            .confirm_table
            .insert(order_confirm.replica_id, signed.clone());
        if self.is_confirmed(order_confirm.sequence_number) {
            debug!("confirmed {}", order_confirm.sequence_number);
            self.confirmed_high = order_confirm.sequence_number;
            self.flush_verified();
        }
    }

    // should be called whenever op_number changed, confirmed_high changed, or
    // verified_buffer changed
    fn flush_verified(&mut self) {
        debug!(
            "flush verified: seq number = {}, confirm number = {}",
            self.sequence_number, self.confirm_number
        );
        assert!(self.sequence_number <= self.confirm_number);

        let mut next_confirm = self.confirm_number + 1;
        let verified_buffer = mem::take(&mut self.verified_buffer);
        while let Some(verified) = verified_buffer.get(&next_confirm) {
            self.confirm_next(verified);
            next_confirm = self.confirm_number + 1;
        }
        self.verified_buffer = verified_buffer;

        let mut next_insert = self.sequence_number + 1;
        while next_insert <= self.confirmed_high {
            if let Some(request) = self.verified_buffer.remove(&next_insert) {
                self.insert_log(request);
                next_insert = self.sequence_number + 1;
            } else {
                break;
            }
        }
        debug!(
            "flush verified (out): seq number = {}, confirm number = {}",
            self.sequence_number, self.confirm_number
        );
        assert!(self.sequence_number <= self.confirm_number);
    }

    fn insert_log(&mut self, verified: VerifiedOrderedMulticast<message::Request>) {
        assert_eq!(verified.meta.sequence_number, self.sequence_number + 1);
        self.sequence_number += 1;
        self.op_number += 1;
        debug!(
            "insert log seq {} op {}",
            self.sequence_number, self.op_number
        );
        let request = (*verified).clone();
        self.log.push(verified);
        self.log_hash = Sha256::new()
            .chain_update(&self.log_hash)
            .chain_update(request.client_id)
            .chain_update(request.request_number.to_le_bytes())
            .chain_update(&request.op)
            .finalize()
            .into();

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
            view_number: self.view_number,
            replica_id: self.id,
            op_number,
            log_hash: self.log_hash,
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

    fn handle_order_confirm(
        &mut self,
        _remote: T::Address,
        message: VerifiedMessage<message::OrderConfirm>,
    ) {
        if message.view_number != self.view_number {
            return;
        }
        if message.sequence_number <= self.confirmed_high {
            return;
        }
        self.insert_order_confirm(&*message, message.signed_message());
    }

    fn send_query(&self) {
        info!("send query {:?}", self.query_number);
        let query = message::Query {
            view_number: self.view_number,
            sequence_number: self.query_number.unwrap(),
        };
        self.transport.send_message_to_all(
            self,
            self.config.replica(..),
            serialize(ToReplica::Query(query)),
        );
    }

    fn handle_query(&mut self, remote: T::Address, message: message::Query) {
        if message.view_number != self.view_number {
            return;
        }
        let op_number = message.sequence_number + self.op_number as u32 - self.sequence_number;
        let request = if let Some(request) = self.log.get((op_number - 1) as usize) {
            request
        } else if let Some(request) = self.verified_buffer.get(&message.sequence_number) {
            request
        } else {
            return;
        };
        let query_reply = message::QueryReply {
            view_number: self.view_number,
            request: request.meta.clone(),
        };
        self.transport
            .send_message(self, &remote, serialize(ToReplica::QueryReply(query_reply)));
    }

    fn handle_query_reply(
        &mut self,
        _remote: T::Address,
        message: (ViewNumber, VerifiedOrderedMulticast<message::Request>),
    ) {
        let (view_number, verified) = message;
        if view_number != self.view_number
            || Some(verified.meta.sequence_number) != self.query_number
        {
            return;
        }
        self.queried_count += 1;
        // assert higher neighbour already verified so just chain into
        self.insert_chain(verified);
    }
}

impl<T: Transport> Drop for Replica<T> {
    fn drop(&mut self) {
        info!(
            "seq number/confirm number: {}/{}",
            self.sequence_number, self.confirm_number
        );
        info!(
            "signed/unsigned/skipped signed/queried: {}/{}/{}/{}",
            self.signed_count, self.unsigned_count, self.skipped_count, self.queried_count
        );
        if !self.received_buffer.is_empty() {
            warn!(
                "not inserted chain request: {} remain ({} skipped)",
                self.received_buffer.len(),
                self.received_buffer
                    .values()
                    .filter(|verified| verified.status == Status::SkippedSigned)
                    .count()
            );
        }
        if !self.verified_buffer.is_empty() {
            warn!(
                "not inserted reorder request: {} remain",
                self.verified_buffer.len()
            );
        }
    }
}
