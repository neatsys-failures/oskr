use std::sync::Arc;

use crate::{
    common::{Config, ReplicaId},
    facade::{App, Receiver, Transport},
    stage::{Handle, State, StatefulContext, StatelessContext},
};

pub struct Replica<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    app: Box<dyn App>,
    batch_size: usize,
    adaptive_batching: bool,
    address: T::Address,

    shared: Arc<Shared<T>>,
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
        app: impl App + 'static,
        batch_size: usize,
        adaptive_batching: bool,
    ) -> Handle<Self> {
        assert!(config.replica(..).len() > 1);

        let replica = Handle::from(Self {
            config: config.clone(),
            transport: transport.tx_agent(),
            id: replica_id,
            app: Box::new(app),
            batch_size,
            adaptive_batching,
            address: config.replica(replica_id).clone(),
            shared: Arc::new(Shared {
                transport: transport.tx_agent(),
                address: config.replica(replica_id).clone(),
                config,
            }),
        });

        replica.with_stateful(|state| {
            //
        });

        replica
    }
}
