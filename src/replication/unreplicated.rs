use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use bincode::deserialize;
use serde_derive::{Deserialize, Serialize};
use tokio::{
    select,
    sync::mpsc::{unbounded_channel, UnboundedReceiver},
    time::interval,
};

use crate::{
    common::{generate_id, serialize, ClientId, OpNumber, Opaque, ReplicaId, RequestNumber},
    executor::Executor,
    transport::{Receiver, Transport, TxAgent},
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
            let _ = tx.send((remote, buffer)); // unregister?
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

pub struct Replica<T: Transport, App> {
    address: T::Address,
    transport: T::TxAgent,

    app: App,
    op_number: OpNumber,
    client_table: HashMap<ClientId, ReplyMessage>,
}

impl<T: Transport, A> Receiver<T> for Replica<T, A> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport, A: App + Send + 'static> Executor<Replica<T, A>> {
    pub fn register_new(transport: &mut T, replica_id: ReplicaId, app: A) -> Arc<Self> {
        assert_eq!(replica_id, 0);
        let replica = Arc::new(Self::new(Replica {
            address: transport.tx_agent().config().replica_address[0].clone(),
            transport: transport.tx_agent(),
            app,
            op_number: 0,
            client_table: HashMap::new(),
        }));
        replica.register_stateful(transport, Replica::receive_buffer);
        replica
    }
}

impl<T: Transport, A: App> Replica<T, A> {
    fn receive_buffer(
        &mut self,
        _executor: &Executor<Self>,
        remote: T::Address,
        buffer: T::RxBuffer,
    ) {
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
