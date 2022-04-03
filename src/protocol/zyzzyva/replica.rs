use std::{collections::HashMap, sync::Arc};

use crate::{
    common::{
        deserialize, ClientId, Config, OpNumber, ReplicaId, RequestNumber, SignedMessage,
        ViewNumber,
    },
    facade::{App, Receiver, Transport},
    protocol::zyzzyva::message::{self, ToClient, ToReplica},
    stage::{Handle, State, StatefulContext, StatelessContext},
};

pub struct Replica<T: Transport> {
    config: Config<T>,
    transport: T::TxAgent,
    id: ReplicaId,
    app: Box<dyn App + Send>,
    batch_size: usize,
    adaptive_batching: bool,
    address: T::Address,

    view_number: ViewNumber,
    op_number: OpNumber,
    history: Vec<LogItem>,
    client_table: HashMap<ClientId, (RequestNumber, ToClient)>,

    shared: Arc<Shared<T>>,
}

struct LogItem {
    //
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
            view_number: 0,
            op_number: 0,
            history: Vec::new(),
            client_table: HashMap::new(),
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
                //
            }
            _ => {}
        }
    }
}
impl<T: Transport> StatefulContext<'_, Replica<T>> {
    fn handle_request(&mut self, remote: T::Address, message: message::Request) {
        //
    }
}
