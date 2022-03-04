use std::collections::HashMap;

use async_trait::async_trait;
use bincode::deserialize;
use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver},
    future::BoxFuture,
    select, FutureExt, StreamExt,
};
use serde_derive::{Deserialize, Serialize};

use crate::{
    common::{generate_id, serialize, ClientId, OpNumber, Opaque, ReplicaId, RequestNumber},
    executor::{Submit, SubmitExt},
    transport::{self, Transport, TxAgent},
    App, Invoke,
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

pub struct Client<T: Transport> {
    address: T::Address,
    id: ClientId,
    transport: T::TxAgent,
    rx: UnboundedReceiver<(T::Address, T::RxBuffer)>,
    timeout: Box<dyn FnMut() -> BoxFuture<'static, ()> + Send>,

    request_number: RequestNumber,
}

impl<T: Transport> transport::Receiver<T> for Client<T> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Client<T> {
    pub fn register_new(
        transport: &mut T,
        timeout: impl FnMut() -> BoxFuture<'static, ()> + Send + 'static,
    ) -> Self {
        let (tx, rx) = unbounded();
        let client = Self {
            address: transport.ephemeral_address(),
            id: generate_id(),
            transport: transport.tx_agent(),
            rx,
            timeout: Box::new(timeout),
            request_number: 0,
        };
        transport.register(&client, move |remote, buffer| {
            let _ = tx.unbounded_send((remote, buffer)); // unregister?
        });
        client
    }
}

#[async_trait]
impl<T: Transport> Invoke for Client<T> {
    async fn invoke(&mut self, op: Opaque) -> Opaque {
        self.request_number += 1;
        let request = RequestMessage {
            client_id: self.id,
            request_number: self.request_number,
            op,
        };

        self.transport
            .send_message_to_replica(self, 0, serialize(request.clone()));
        loop {
            select! {
                recv = self.rx.next() => {
                    let (_remote, buffer) = recv.unwrap();
                    let reply: ReplyMessage = deserialize(buffer.as_ref()).unwrap();
                    if reply.request_number == self.request_number {
                        return reply.result;
                    }
                }
                _ = (self.timeout)().fuse() => {
                    println!("resend");
                    self.transport.send_message_to_replica(self, 0, serialize(request.clone()));
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
    app: Box<dyn App>,
}

impl<T: Transport> transport::Receiver<T> for Replica<T> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Replica<T> {
    pub fn register_new(
        transport: &mut T,
        submit: impl Submit<Self> + Send + 'static,
        replica_id: ReplicaId,
        app: impl App + Send + 'static,
    ) -> Self {
        assert_eq!(replica_id, 0);
        let replica = Replica {
            address: transport.tx_agent().config().replica_address[0].clone(),
            transport: transport.tx_agent(),
            app: Box::new(app),
            op_number: 0,
            client_table: HashMap::new(),
        };

        transport.register(&replica, move |remote, buffer| {
            submit.stateful(move |replica| replica.receive_buffer(remote, buffer))
        });
        replica
    }
}

impl<T: Transport> Replica<T> {
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

    use tokio::{
        spawn,
        task::{spawn_blocking, yield_now},
        time::timeout,
    };

    use crate::{app::mock::App, executor::Submit, transport::simulated::Transport, Invoke};

    use super::Client;

    #[tokio::test(start_paused = true)]
    async fn one_request() {
        let mut transport = Transport::new(1, 0);
    }
}
