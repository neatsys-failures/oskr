use crate::common::*;
use crate::model::{Config, *};
use bincode::deserialize;
use serde_derive::*;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::*;
use tokio::task::*;
use tokio::time::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestMessage {
    id: ClientId,
    sequence: u32,
    op: Data,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplyMessage {
    sequence: u32,
    result: Data,
}

pub struct Replica {
    op_number: OpNumber,
    app: Box<dyn App>,
    client_table: HashMap<ClientId, ReplyMessage>,

    transport: Arc<dyn Transport>,
    tx: UnboundedSender<(TransportAddress, RxBuffer)>,
    rx: UnboundedReceiver<(TransportAddress, RxBuffer)>,
    config: Arc<Config>,
}

impl Replica {
    pub fn new(config: Arc<Config>, app: Box<dyn App>) -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            op_number: 0,
            app,
            client_table: HashMap::new(),
            transport: Arc::new(NullTransport),
            tx,
            rx,
            config,
        }
    }

    pub fn init(&mut self, transport: Arc<dyn Transport>) {
        self.transport = transport;
    }
}

impl TransportReceiver for Replica {
    fn get_address(&self) -> &TransportAddress {
        &self.config.address_list[0]
    }
    fn get_inbox(&self) -> Box<dyn Fn(&TransportAddress, RxBuffer)> {
        let tx = self.tx.clone();
        Box::new(move |remote, buffer| tx.send((remote.clone(), buffer)).unwrap())
    }
}

impl Replica {
    pub async fn run(&mut self) {
        loop {
            let (remote, buffer) = self.rx.recv().await.unwrap();
            let request = deserialize(&*buffer).unwrap();
            self.handle_request(&remote, &request);
        }
    }

    fn handle_request(&mut self, remote: &TransportAddress, request: &RequestMessage) {
        if let Some(reply) = self.client_table.get(&request.id) {
            if reply.sequence > request.sequence {
                return;
            }
            if reply.sequence == request.sequence {
                let reply = reply.clone();
                self.transport
                    .send_message(self, remote, &mut bincode(reply));
                return;
            }
        }
        self.op_number += 1;
        let result = self.app.execute(&request.op);
        let mut reply = ReplyMessage::default();
        reply.sequence = request.sequence;
        reply.result = result;

        self.client_table.insert(request.id, reply.clone());
        self.transport
            .send_message(self, remote, &mut bincode(reply));
    }
}

pub struct Client {
    pending: Option<Pending>,
    sequence: u32,

    transport: Arc<dyn Transport>,
    address: TransportAddress,
    tx: UnboundedSender<Activity>,
    rx: UnboundedReceiver<Activity>,
    id: ClientId,
}

struct Pending {
    op: Data,
    timer: JoinHandle<()>,
}

#[derive(Debug)]
enum Activity {
    Rx(TransportAddress, RxBuffer),
    Resend,
    Resolve(Data),
}

impl Client {
    pub fn new(_config: Arc<Config>, address: TransportAddress) -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            pending: None,
            sequence: 0,
            transport: Arc::new(NullTransport),
            address,
            tx,
            rx,
            id: generate_id(),
        }
    }
    pub fn init(&mut self, transport: Arc<dyn Transport>) {
        self.transport = transport;
    }
}

impl TransportReceiver for Client {
    fn get_address(&self) -> &TransportAddress {
        &self.address
    }
    fn get_inbox(&self) -> Box<dyn Fn(&TransportAddress, RxBuffer)> {
        let tx = self.tx.clone();
        Box::new(move |remote, buffer| tx.send(Activity::Rx(remote.clone(), buffer)).unwrap())
    }
}

impl Invoke for Client {
    fn invoke<'a>(&'a mut self, op: Data) -> Pin<Box<dyn 'a + Future<Output = Data>>> {
        if self.pending.is_some() {
            panic!("duplicated invoke");
        }

        let tx = self.tx.clone();
        let timer = spawn(async move {
            let mut interval = interval(Duration::from_millis(1000));
            interval.tick().await;
            loop {
                interval.tick().await;
                tx.send(Activity::Resend).unwrap();
            }
        });

        self.sequence += 1;
        self.pending = Some(Pending { op, timer });
        self.send_request();

        Box::pin(async { self.run().await })
    }
}

impl Client {
    async fn run(&mut self) -> Data {
        while self.rx.try_recv().is_ok() {} // clear rx, only handle new activities
        loop {
            match self.rx.recv().await.unwrap() {
                Activity::Rx(remote, buffer) => {
                    let reply = deserialize(&*buffer).unwrap();
                    self.handle_reply(&remote, &reply);
                }
                Activity::Resend => self.send_request(),
                Activity::Resolve(data) => return data,
            }
        }
    }

    fn handle_reply(&mut self, _remote: &TransportAddress, reply: &ReplyMessage) {
        assert!(reply.sequence <= self.sequence);
        if reply.sequence != self.sequence {
            return;
        }
        let pending = self.pending.take().unwrap();
        pending.timer.abort();
        self.tx
            .send(Activity::Resolve(reply.result.clone()))
            .unwrap();
    }

    fn send_request(&mut self) {
        let mut request = RequestMessage::default();
        request.id = self.id;
        request.sequence = self.sequence;
        request.op = self.pending.as_ref().unwrap().op.clone();
        self.transport
            .send_message_to_replica(self, 0, &mut bincode(request));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::mock::App;
    use crate::simulated::{Transport, *};
    use ::futures::future::*;

    struct System {
        replica: Option<Replica>,
        replica_thread: Option<JoinHandle<()>>,
        client: Vec<Client>,
    }

    struct Blueprint(Arc<Config>, usize); // number of clients

    impl TestSystem for System {
        type Blueprint = Blueprint;
        fn new(transport: &mut Transport, blueprint: Self::Blueprint) -> Self {
            let replica = Replica::new(blueprint.0.clone(), Box::new(App {}));
            transport.register(&replica);
            Self {
                replica: Some(replica),
                replica_thread: None,
                client: (0..blueprint.1)
                    .map(|i| {
                        let client = Client::new(
                            blueprint.0.clone(),
                            Transport::address(&format!("client-{}", i)),
                        );
                        transport.register(&client);
                        client
                    })
                    .collect(),
            }
        }
        fn set_up(&mut self, transport: Arc<Transport>) {
            let mut replica = self.replica.take().unwrap();
            replica.init(transport.clone());
            for client in &mut self.client {
                client.init(transport.clone());
            }
            self.replica_thread = Some(spawn_local(async move { replica.run().await }));
        }
        fn tear_down(self) {
            self.replica_thread.unwrap().abort();
        }
    }

    #[tokio::test]
    async fn one_request() {
        let config = Arc::new(Config {
            f: 0,
            address_list: vec![Transport::address("replica-0")],
        });
        Transport::run(
            config.clone(),
            Blueprint(config, 1),
            |system: &mut System, _transport| {
                Box::pin(async {
                    let result = system.client[0].invoke(b"hello".to_vec()).await;
                    assert_eq!(result, b"reply: hello");
                })
            },
        )
        .await;
    }

    #[tokio::test]
    async fn one_client_ten_request() {
        let config = Arc::new(Config {
            f: 0,
            address_list: vec![Transport::address("replica-0")],
        });
        Transport::run(
            config.clone(),
            Blueprint(config, 1),
            |system: &mut System, _transport| {
                Box::pin(async {
                    for i in 0..10 {
                        assert_eq!(
                            system.client[0]
                                .invoke(format!("hello-{}", i).into_bytes())
                                .await,
                            format!("reply: hello-{}", i).into_bytes()
                        );
                    }
                })
            },
        )
        .await;
    }

    #[tokio::test]
    async fn ten_client_one_request() {
        let config = Arc::new(Config {
            f: 0,
            address_list: vec![Transport::address("replica-0")],
        });
        Transport::run(
            config.clone(),
            Blueprint(config, 10),
            |system: &mut System, transport| {
                Box::pin(async {
                    transport.time_limit(Duration::from_millis(0));
                    let result_list =
                        join_all(
                            system.client.iter_mut().enumerate().map(|(i, client)| {
                                client.invoke(format!("hello+{}", i).into_bytes())
                            }),
                        )
                        .await;
                    for (i, result) in result_list.into_iter().enumerate() {
                        assert_eq!(result, format!("reply: hello+{}", i).into_bytes());
                    }
                })
            },
        )
        .await;
    }
}
