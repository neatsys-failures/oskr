use crate::common::{generate_id, serialize, ClientId, Opaque, ReplicaId, RequestNumber};
use crate::transport::{Receiver, Transport, TxAgent};
use crate::App;
use bincode::deserialize;
use derivative::Derivative;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio::task::spawn;
use tokio::time::interval;

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

pub struct Client<T: Transport> {
    tx: mpsc::UnboundedSender<ClientEvent<T>>,
    rx: mpsc::UnboundedReceiver<ClientEvent<T>>,
    transport: T::TxAgent,

    address: T::Address,
    client_id: ClientId,
    request_number: RequestNumber,
    invoke: Option<Invoke>,
}

struct Invoke {
    request_number: u32,
    op: Opaque,
    tx: oneshot::Sender<Opaque>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum ClientEvent<T: Transport> {
    Receive(
        #[derivative(Debug = "ignore")] T::Address,
        #[derivative(Debug = "ignore")] T::RxBuffer,
    ),
    SendTimer,
}

impl<T: Transport> Receiver<T> for Client<T> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Client<T> {
    pub fn new(transport: &T) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx,
            transport: transport.tx_agent(),
            address: transport.ephemeral_address(),
            client_id: generate_id(),
            request_number: 0,
            invoke: None,
        }
    }

    pub fn register(&self, transport: &mut T) {
        let tx = self.tx.clone();
        transport.register(self, move |remote, buffer| {
            tx.send(ClientEvent::Receive(remote.clone(), buffer))
                .unwrap();
        });
    }
}

impl<'a, T: Transport> crate::Invoke for &'a mut Client<T> {
    type Future = Pin<Box<dyn 'a + Future<Output = Opaque> + Send>>;

    fn invoke(self, op: Opaque) -> Self::Future {
        assert!(self.invoke.is_none());
        self.request_number += 1;
        let (tx, mut result_rx) = oneshot::channel();
        self.invoke = Some(Invoke {
            request_number: self.request_number,
            op,
            tx,
        });

        let send_timer_tx = self.tx.clone();
        let send_timer = spawn(async move {
            let mut send = interval(Duration::from_millis(1000));
            loop {
                send.tick().await;
                send_timer_tx.send(ClientEvent::SendTimer).unwrap();
            }
        });

        Box::pin(async move {
            loop {
                select! {
                    Ok(result) = &mut result_rx => {
                        send_timer.abort();
                        return result;
                    },
                    Some(event) = self.rx.recv() => {
                        match event {
                            ClientEvent::Receive(remote, buffer) => {
                                let message: ReplyMessage = deserialize(buffer.as_ref()).unwrap();
                                self.handle_reply(remote, message);
                            }
                            ClientEvent::SendTimer => self.send_request(),
                        }
                    }
                    else => unreachable!(),
                }
            }
        })
    }
}

impl<T: Transport> Client<T> {
    fn send_request(&self) {
        let invoke = self.invoke.as_ref().unwrap();
        let request = RequestMessage {
            client_id: self.client_id,
            request_number: invoke.request_number,
            op: invoke.op.clone(),
        };
        self.transport
            .send_message_to_replica(self, 0, serialize(request));
    }

    fn handle_reply(&mut self, _remote: T::Address, message: ReplyMessage) {
        if self
            .invoke
            .as_ref()
            .map(|invoke| invoke.request_number != message.request_number)
            .unwrap_or(true)
        {
            return;
        }
        let invoke = self.invoke.take().unwrap();
        invoke.tx.send(message.result).unwrap();
    }
}

pub struct Replica<T: Transport, A> {
    address: T::Address,
    tx: mpsc::UnboundedSender<ReplicaReceive<T>>,
    rx: mpsc::UnboundedReceiver<ReplicaReceive<T>>,
    transport: T::TxAgent,

    app: A,
    client_table: HashMap<ClientId, (RequestNumber, ReplyMessage)>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct ReplicaReceive<T: Transport>(
    #[derivative(Debug = "ignore")] T::Address,
    #[derivative(Debug = "ignore")] T::RxBuffer,
);

impl<T: Transport, A: App> Replica<T, A> {
    pub fn new(transport: &T, replica_id: ReplicaId, app: A) -> Self {
        assert_eq!(replica_id, 0);
        let (tx, rx) = mpsc::unbounded_channel();
        let transport = transport.tx_agent();
        Self {
            address: transport.config().replica_address[0].clone(),
            tx,
            rx,
            transport,
            app,
            client_table: HashMap::new(),
        }
    }

    pub fn register(&self, transport: &mut T) {
        let tx = self.tx.clone();
        transport.register(self, move |remote, buffer| {
            tx.send(ReplicaReceive(remote.clone(), buffer)).unwrap();
        });
    }
}

impl<T: Transport, A> Receiver<T> for Replica<T, A> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport, A: App> Replica<T, A> {
    pub async fn run(&mut self) {
        while let Some(ReplicaReceive(remote, buffer)) = self.rx.recv().await {
            let message: RequestMessage = deserialize(buffer.as_ref()).unwrap();
            self.handle_request(remote, message);
        }
        unreachable!();
    }

    fn handle_request(&mut self, remote: T::Address, message: RequestMessage) {
        if let Some((request_number, reply)) = self.client_table.get(&message.client_id) {
            if *request_number > message.request_number {
                return;
            }
            if *request_number == message.request_number {
                self.transport
                    .send_message(self, &remote, serialize(reply.clone()));
                return;
            }
        }

        let result = self.app.execute(message.op);
        let reply = ReplyMessage {
            request_number: message.request_number,
            result,
        };
        self.client_table
            .insert(message.client_id, (message.request_number, reply.clone()));
        self.transport.send_message(self, &remote, serialize(reply));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::mock::App;
    use crate::transport::simulated::Transport;
    use crate::Invoke;
    use std::iter;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use tokio::test;

    #[test(start_paused = true)]
    async fn one_request() {
        let mut transport = Transport::new(1, 0);

        let mut replica = Replica::new(&transport, 0, App::default());
        replica.register(&mut transport);
        let replica = spawn(async move { replica.run().await });

        let mut client = Client::new(&transport);
        client.register(&mut transport);

        let transport = spawn(async move { transport.deliver_now().await });
        select! {
            result = client.invoke(b"hello".to_vec()) => {
                assert_eq!(result, b"reply: hello".to_vec());
            }
            _ = transport => unreachable!(),
        }

        replica.abort();
    }

    #[test(start_paused = true)]
    async fn multiple_client_close_loop() {
        let mut transport = Transport::new(1, 0);
        let mut replica = Replica::new(&transport, 0, App::default());
        replica.register(&mut transport);
        let replica = spawn(async move { replica.run().await });

        let create_client = || {
            let mut client = Client::new(&transport);
            client.register(&mut transport);
            let count = Arc::new(AtomicU32::new(1));
            let client_count = count.clone();
            let client = spawn(async move {
                loop {
                    let result = client
                        .invoke(format!("request-{:?}", client_count).into_bytes())
                        .await;
                    assert_eq!(
                        result,
                        format!("reply: request-{:?}", client_count).into_bytes()
                    );
                    client_count.fetch_add(1, Ordering::SeqCst);
                }
            });
            (count, client)
        };
        let multiple_client: Vec<_> = iter::repeat_with(create_client).take(3).collect();

        transport.insert_filter(
            1,
            Transport::delay(Duration::from_millis(5), Duration::from_millis(25)),
        );
        transport.deliver(Duration::from_secs(1)).await;

        replica.abort();
        multiple_client.into_iter().for_each(|(count, client)| {
            client.abort();
            assert!(count.load(Ordering::SeqCst) >= 20);
            assert!(count.load(Ordering::SeqCst) <= 100);
        });
    }
}
