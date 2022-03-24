use std::{collections::HashMap, sync::Arc};

use sha2::Sha256;

use crate::{
    common::{
        ClientId, Digest, OpNumber, ReplicaId, RequestNumber, SignedMessage, SigningKey,
        VerifyingKey,
    },
    facade::{App, Receiver, Transport, TxAgent},
    protocol::hotstuff::message::{self, GenericNode, QuorumCertification},
    stage::{Handle, State, StatefulContext, StatelessContext},
};

pub struct Replica<T: Transport> {
    address: T::Address,
    transport: T::TxAgent,
    id: ReplicaId,
    batch_size: usize,

    vote_table: HashMap<Digest, HashMap<ReplicaId, SignedMessage<message::VoteGeneric>>>,
    voted_height: OpNumber,
    block_locked: Digest,
    block_executed: Digest,
    qc_high: QuorumCertification,
    block_leaf: Digest,

    client_table: HashMap<ClientId, (RequestNumber, Option<SignedMessage<message::Reply>>)>,
    log: HashMap<Digest, GenericNode>,
    batch: Vec<message::Request>,

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
    ) -> Handle<Self> {
        assert!(transport.tx_agent().config().replica_address.len() > 1); // TODO

        let address = transport.tx_agent().config().replica_address[replica_id as usize].clone();
        let replica: Handle<_> = Self {
            address: address.clone(),
            transport: transport.tx_agent(),
            id: replica_id,
            batch_size,
            vote_table: HashMap::new(),
            voted_height: 0,
            block_locked: Digest::default(),
            block_executed: Digest::default(),
            block_leaf: Digest::default(),
            qc_high: QuorumCertification::default(),
            client_table: HashMap::new(),
            log: HashMap::new(), // do we need a block0 guard?
            batch: Vec::new(),
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
                submit.stateless(|replica| {
                    //
                });
            });
        });

        replica
    }
}
/*
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    // "algorithm 4" in HotStuff paper
    fn create_leaf(
        parent: &Digest,
        command: Vec<message::Request>,
        qc: QuorumCertification,
        height: OpNumber,
    ) -> Digest {
        let node = GenericNode {
            parent: *parent,
            command,
            justify: qc,
            height,
        };
        //
    }
}
*/
