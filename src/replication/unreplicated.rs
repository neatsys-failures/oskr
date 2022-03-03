use std::{collections::HashMap, sync::Arc, time::Duration};

use async_std::{
    channel::{unbounded, Receiver},
    prelude::{FutureExt, StreamExt},
    stream::interval,
    task::spawn,
};
use async_trait::async_trait;
use bincode::deserialize;
use serde_derive::{Deserialize, Serialize};

use crate::{
    common::{generate_id, serialize, ClientId, OpNumber, Opaque, ReplicaId, RequestNumber},
    executor::Executor,
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
    rx: Receiver<(T::Address, T::RxBuffer)>,

    request_number: RequestNumber,
}

impl<T: Transport> transport::Receiver<T> for Client<T> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<T: Transport> Client<T> {
    pub fn register_new(transport: &mut T) -> Self {
        let (tx, rx) = unbounded();
        let client = Self {
            address: transport.ephemeral_address(),
            id: generate_id(),
            transport: transport.tx_agent(),
            rx,
            request_number: 0,
        };
        transport.register(&client, move |remote, buffer| {
            let _ = tx.try_send((remote, buffer)); // unregister?
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
        let (resend_tx, resend_rx) = unbounded();
        let resend_timer = spawn(async move {
            let mut interval = interval(Duration::from_millis(1000));
            while let Some(_) = interval.next().await {
                if resend_tx.send(()).await.is_err() {
                    return;
                }
            }
        });

        loop {
            if let Some((_remote, buffer)) = async { Some(self.rx.recv().await.unwrap()) }
                .race(async {
                    resend_rx.recv().await.unwrap();
                    None
                })
                .await
            {
                let reply: ReplyMessage = deserialize(buffer.as_ref()).unwrap();
                if reply.request_number == self.request_number {
                    let _ = resend_timer.cancel().await;
                    return reply.result;
                }
            } else {
                println!("resend");
                self.transport
                    .send_message_to_replica(self, 0, serialize(request.clone()));
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

impl<T: Transport, A> transport::Receiver<T> for Replica<T, A> {
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
