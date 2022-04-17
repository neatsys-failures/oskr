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
    common::{deserialize, generate_id, ClientId, Config, Opaque, RequestNumber, SignedMessage},
    facade::{AsyncEcosystem, Invoke, Receiver, Transport, TxAgent},
    protocol::neo::message::{self, OrderedMulticast},
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
        self.transport
            .send_message(self, self.config.multicast.as_ref().unwrap(), |buffer| {
                OrderedMulticast::assemble(request.clone(), buffer)
            });

        let mut result_table = HashMap::new();
        let mut receive_buffer =
            move |client: &mut Self, _remote: T::Address, buffer: T::RxBuffer| {
                let reply: SignedMessage<message::Reply> = deserialize(buffer.as_ref()).unwrap();
                let reply = reply.assume_verified();
                if reply.request_number != client.request_number {
                    return None;
                }

                let matcher = (
                    reply.view_number,
                    reply.op_number,
                    reply.log_hash,
                    reply.result,
                );
                result_table.insert(reply.replica_id, matcher.clone());

                if result_table
                    .values()
                    .filter(|record| **record == matcher)
                    .count()
                    == 2 * client.config.f + 1
                {
                    Some(matcher.3)
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
                        .send_message(self, self.config.multicast.as_ref().unwrap(), |buffer| {
                            OrderedMulticast::assemble(request.clone(), buffer)
                        });
                    timeout = Instant::now() + Duration::from_millis(1000);
                }
            }
        }
    }
}
