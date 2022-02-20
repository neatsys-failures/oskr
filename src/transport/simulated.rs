use crate::common::ReplicaId;
use crate::transport::{self, Config, Receiver};
use rand::{thread_rng, Rng};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::spawn;
use tokio::time::{sleep, sleep_until, Instant};

type Address = String;
type Message = Vec<u8>;

pub struct Transport {
    rx: mpsc::UnboundedReceiver<(Address, Address, Message, bool)>,
    tx: mpsc::UnboundedSender<(Address, Address, Message, bool)>,
    recv_table: RecvTable,
    config: Arc<Config<Self>>,
    filter_table: FilterTable,
}
type RecvTable = HashMap<Address, Box<dyn Fn(&Address, RxBuffer) + Send>>;
type FilterTable =
    HashMap<u32, Box<dyn Fn(&Address, &Address, &[u8], &mut Duration) -> bool + Send>>;

#[derive(Debug, Clone)]
pub struct RxBuffer(Message);
impl AsRef<[u8]> for RxBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub struct TxAgent {
    tx: mpsc::UnboundedSender<(Address, Address, Message, bool)>,
    config: Arc<Config<Transport>>,
}

impl transport::TxAgent for TxAgent {
    type Transport = Transport;
    fn send_message(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest: &<Self::Transport as transport::Transport>::Address,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        let mut buffer = [0; 9000];
        let message_length = message(&mut buffer);
        let message = buffer[..message_length as usize].to_vec();
        self.tx
            .send((source.get_address().clone(), dest.clone(), message, false))
            .unwrap();
    }
    fn send_message_to_replica(
        &self,
        source: &impl Receiver<Self::Transport>,
        replica_id: ReplicaId,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        self.send_message(source, &format!("replica-{}", replica_id), message);
    }
    fn send_message_to_all(
        &self,
        source: &impl Receiver<Self::Transport>,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        let mut buffer = [0; 9000];
        let message_length = message(&mut buffer);
        let message = buffer[..message_length as usize].to_vec();
        for dest in &self.config.replica_address {
            if dest != source.get_address() {
                self.tx
                    .send((
                        source.get_address().clone(),
                        dest.clone(),
                        message.clone(),
                        false,
                    ))
                    .unwrap();
            }
        }
    }
    fn send_message_to_multicast(
        &self,
        source: &impl Receiver<Self::Transport>,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        todo!()
    }
}

impl transport::Transport for Transport {
    type Address = Address;
    type RxBuffer = RxBuffer;
    type TxAgent = TxAgent;

    fn tx_agent(&self) -> Self::TxAgent {
        TxAgent {
            tx: self.tx.clone(),
            config: self.config.clone(),
        }
    }

    fn config(&self) -> &Config<Self> {
        &self.config
    }

    fn register(
        &mut self,
        receiver: &impl Receiver<Self>,
        rx_agent: impl Fn(&Self::Address, Self::RxBuffer) + 'static + Send,
    ) where
        Self: Sized,
    {
        self.recv_table
            .insert(receiver.get_address().clone(), Box::new(rx_agent));
    }

    fn register_multicast(
        &mut self,
        rx_agent: impl Fn(&Self::Address, Self::RxBuffer) + 'static + Send,
    ) {
        todo!()
    }

    fn allocate_address(&self) -> Self::Address {
        format!(
            "client-{}",
            char::from_u32(self.recv_table.len() as u32 + 'A' as u32).unwrap()
        )
    }
}

impl Transport {
    pub fn new(n_replica: usize, n_fault: usize) -> Self {
        let config = Config {
            replica_address: (0..n_replica).map(|i| format!("replica-{}", i)).collect(),
            multicast_address: None, // TODO
            n_fault,
        };
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            rx,
            tx,
            recv_table: HashMap::new(),
            config: Arc::new(config),
            filter_table: HashMap::new(),
        }
    }

    pub async fn deliver(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        loop {
            select! {
                _ = sleep_until(deadline) => break,
                Some((source, dest, message, filtered)) = self.rx.recv() => {
                    if filtered {
                        (self.recv_table.get(&dest).unwrap())(&source, RxBuffer(message));
                        continue;
                    }

                    let mut delay = Duration::ZERO;
                    let mut drop = false;
                    for filter in self.filter_table.values() {
                        if !filter(&source, &dest, &message, &mut delay) {
                            drop = true;
                            break;
                        }
                    }
                    println!("{} -> {} {:?} {}", source, dest, message, if drop {"[drop]".to_string()} else {
                        format!("[delay = {:?}]", delay)
                    });
                    if drop {
                        continue;
                    } else if delay.is_zero() {
                    (self.recv_table.get(&dest).unwrap())(&source, RxBuffer(message));
                    } else {
                        let tx = self.tx.clone();
                        spawn(async move {
                            sleep(delay).await;
                            tx.send((source, dest, message, true)).unwrap();
                        });
                    }
                }
            }
        }
    }

    pub async fn deliver_now(&mut self) {
        self.deliver(Duration::from_micros(1)).await;
    }

    pub fn insert_filter(
        &mut self,
        filter_id: u32,
        filter: impl Fn(&Address, &Address, &[u8], &mut Duration) -> bool + 'static + Send,
    ) {
        self.filter_table.insert(filter_id, Box::new(filter));
    }

    pub fn remove_filter(&mut self, filter_id: u32) {
        self.filter_table.remove(&filter_id);
    }

    pub fn delay(
        min: Duration,
        max: Duration,
    ) -> impl Fn(&Address, &Address, &[u8], &mut Duration) -> bool + 'static + Send {
        move |_, _, _, delay| {
            *delay += thread_rng().gen_range(min..max); // TODO
            true
        }
    }
}
