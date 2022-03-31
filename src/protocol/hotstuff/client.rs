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
use tracing::{debug, warn};

use crate::{
    common::{
        deserialize, generate_id, serialize, ClientId, Config, Opaque, RequestNumber, SignedMessage,
    },
    facade::{AsyncEcosystem, Invoke, Receiver, Transport, TxAgent},
    protocol::hotstuff::message::{self, ToReplica},
};

pub struct Client<T: Transport, E> {
    address: T::Address,
    pub(super) id: ClientId,
    config: Config<T>,
    transport: T::TxAgent,
    rx: UnboundedReceiver<(T::Address, T::RxBuffer)>,
    _executor: PhantomData<E>,

    request_number: RequestNumber,
}

impl<T: Transport, E> Receiver<T> for Client<T, E> {
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
    Self: Send + Sync,
    E: Send + Sync,
{
    async fn invoke(&mut self, op: Opaque) -> Opaque {
        self.request_number += 1;
        let request = message::Request {
            op,
            request_number: self.request_number,
            client_id: self.id,
        };
        self.transport.send_message_to_all(
            self,
            self.config.replica(..),
            serialize(ToReplica::Request(request.clone())),
        );

        let mut result_table = HashMap::new();
        let mut receive_buffer =
            move |client: &mut Self, _remote: T::Address, buffer: T::RxBuffer| {
                let reply: SignedMessage<message::Reply> = deserialize(buffer.as_ref()).unwrap();
                let reply = reply.assume_verified();
                if reply.request_number != client.request_number {
                    return None;
                }

                result_table.insert(reply.replica_id, reply.result.clone());

                if result_table
                    .values()
                    .filter(|result| **result == reply.result)
                    .count()
                    == client.config.f + 1
                {
                    Some(reply.result)
                } else {
                    None
                }
            };

        let mut timeout = Instant::now() + Duration::from_millis(1000);
        loop {
            select! {
                recv = self.rx.next() => {
                    let (remote, buffer) = recv.unwrap();
                    if let Some(result) = receive_buffer(self, remote, buffer) {
                        return result;
                    }
                }
                _ = E::sleep_until(timeout).fuse() => {
                    warn!("resend for request number {}", self.request_number);
                    self.transport
                        .send_message_to_all(self, self.config.replica(..), serialize(ToReplica::Request(request.clone())));
                    timeout = Instant::now() + Duration::from_millis(1000);
                }
            }
        }
    }
}
