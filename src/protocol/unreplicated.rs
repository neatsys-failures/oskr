use std::{
    collections::HashMap,
    marker::PhantomData,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver},
    select, FutureExt, StreamExt,
};
use serde_derive::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::{
    common::{
        deserialize, generate_id, serialize, ClientId, Config, OpNumber, Opaque, ReplicaId,
        RequestNumber,
    },
    facade::{self, App, AsyncEcosystem, Invoke, Receiver, Transport, TxAgent},
    stage::{Handle, State, StatefulContext},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RequestMessage {
    client_id: ClientId,
    request_number: RequestNumber,
    op: Opaque,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplyMessage {
    request_number: RequestNumber,
    result: Opaque,
}

pub struct Client<T: Transport, E> {
    address: T::Address,
    id: ClientId,
    config: Config<T>,
    transport: T::TxAgent,
    rx: UnboundedReceiver<(T::Address, T::RxBuffer)>,
    _executor: PhantomData<E>,

    request_number: RequestNumber,
}

impl<T: Transport, E> facade::Receiver<T> for Client<T, E> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport, E> Client<T, E> {
    pub fn register_new(config: Config<T>, transport: &mut T) -> Self {
        let (tx, rx) = unbounded();
        let client = Self {
            address: transport.ephemeral_address(),
            id: generate_id(),
            config,
            transport: transport.tx_agent(),
            rx,
            request_number: 0,
            _executor: PhantomData,
        };
        transport.register(&client, move |remote, buffer| {
            if tx.unbounded_send((remote, buffer)).is_err() {
                debug!("client channel broken");
            }
        });
        client
    }
}

#[async_trait]
impl<T: Transport, E: AsyncEcosystem<Opaque>> Invoke for Client<T, E>
where
    Self: Send,
{
    async fn invoke(&mut self, op: Opaque) -> Opaque {
        self.request_number += 1;
        let request = RequestMessage {
            client_id: self.id,
            request_number: self.request_number,
            op,
        };

        self.transport
            .send_message(self, self.config.replica(0), serialize(request.clone()));
        let mut timeout = Instant::now() + Duration::from_millis(1000);

        loop {
            select! {
                _ = E::sleep_until(timeout).fuse() => {
                    warn!("resend for request number {}", self.request_number);
                    self.transport
                        .send_message(self, self.config.replica(0), serialize(request.clone()));
                    timeout = Instant::now() + Duration::from_millis(1000);
                }
                recv = self.rx.next() => {
                    let (_remote, buffer) = recv.unwrap();
                    let reply: ReplyMessage = deserialize(buffer.as_ref()).unwrap();
                    if reply.request_number == self.request_number {
                        return reply.result;
                    }
                }
            }
        }
    }
}

pub struct Replica<T: Transport> {
    address: T::Address,
    transport: T::TxAgent,

    op_number: OpNumber,
    client_table: HashMap<ClientId, ReplyMessage>,
    app: Box<dyn App + Send>,
}

impl<T: Transport> State for Replica<T> {
    type Shared = ();
    fn shared(&self) -> Self::Shared {}
}

impl<'a, T: Transport> Receiver<T> for StatefulContext<'a, Replica<T>> {
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
    ) -> Handle<Self> {
        assert_eq!(replica_id, 0);
        let replica: Handle<_> = Self {
            address: config.replica(0).clone(),
            transport: transport.tx_agent(),

            app: Box::new(app),
            op_number: 0,
            client_table: HashMap::new(),
        }
        .into();

        replica.with_stateful(|replica| {
            let submit = replica.submit.clone();
            transport.register(replica, move |remote, buffer| {
                submit.stateful(move |replica| replica.receive_buffer(remote, buffer))
            });
        });
        replica
    }
}

// can also impl to Replica<T> directly since no submit
impl<'a, T: Transport> StatefulContext<'a, Replica<T>> {
    fn receive_buffer(&mut self, remote: T::Address, buffer: T::RxBuffer) {
        let request: RequestMessage = deserialize(buffer.as_ref()).unwrap();
        if let Some(reply) = self.client_table.get(&request.client_id) {
            if reply.request_number > request.request_number {
                return;
            }
            if reply.request_number == request.request_number {
                self.transport.send_message(self, &remote, serialize(reply));
                return;
            }
        }

        self.op_number += 1;
        let result = self.app.execute(request.op);
        let reply = ReplyMessage {
            request_number: request.request_number,
            result,
        };
        self.client_table.insert(request.client_id, reply.clone());
        self.transport.send_message(self, &remote, serialize(reply));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{spawn, time::timeout};

    use crate::{
        app::mock::App, common::Opaque, facade::Invoke, framework::tokio::AsyncEcosystem,
        simulated::Transport, tests::TRACING,
    };

    use super::{Client, Replica};

    #[tokio::test(start_paused = true)]
    async fn one_request() {
        *TRACING;
        let config = Transport::config_builder(1, 0);
        let mut transport = Transport::new(config());
        Replica::register_new(config(), &mut transport, 0, App::default());
        let mut client: Client<_, AsyncEcosystem> = Client::register_new(config(), &mut transport);

        spawn(async move { transport.deliver_now().await });
        assert_eq!(
            timeout(Duration::from_micros(1), client.invoke(b"hello".to_vec()))
                .await
                .unwrap(),
            b"reply: hello".to_vec()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn multiple_request() {
        *TRACING;
        let config = Transport::config_builder(1, 0);
        let mut transport = Transport::new(config());
        Replica::register_new(config(), &mut transport, 0, App::default());
        let mut client: Client<_, AsyncEcosystem> = Client::register_new(config(), &mut transport);

        spawn(async move { transport.deliver_now().await });
        for i in 0..10 {
            assert_eq!(
                timeout(
                    Duration::from_micros(1),
                    client.invoke(format!("#{}", i).into())
                )
                .await
                .unwrap(),
                Opaque::from(format!("reply: #{}", i))
            );
        }
    }
}
