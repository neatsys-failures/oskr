use std::time::Duration;

use async_trait::async_trait;
use bincode::deserialize;
use serde_derive::{Deserialize, Serialize};
use tokio::{
    select,
    sync::mpsc::{unbounded_channel, UnboundedReceiver},
    time::interval,
};

use crate::{
    common::{generate_id, serialize, ClientId, Opaque, RequestNumber},
    transport::{Receiver, Transport, TxAgent},
    Invoke,
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

    request_number: RequestNumber,
}

impl<T: Transport> Receiver<T> for Client<T> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Client<T> {
    pub fn register_new(transport: &mut T) -> Self {
        let (tx, rx) = unbounded_channel();
        let client = Self {
            address: transport.ephemeral_address(),
            id: generate_id(),
            transport: transport.tx_agent(),
            rx,
            request_number: 0,
        };
        transport.register(&client, move |remote, buffer| {
            tx.send((remote.clone(), buffer))
                .map_err(|_| "failed receive message")
                .unwrap();
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

        let mut interval = interval(Duration::from_millis(1000));
        loop {
            select! {
                _ = interval.tick() =>
                    self.transport.send_message_to_replica(self, 0, serialize(request.clone())),
                Some((_remote, buffer)) = self.rx.recv() => {
                    let reply: ReplyMessage = deserialize(buffer.as_ref()).unwrap();
                    if reply.request_number != self.request_number {
                        continue;
                    }
                    return reply.result;
                }
                else => unreachable!(),
            }
        }
    }
}
