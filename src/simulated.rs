use std::{
    collections::HashMap,
    fmt::Debug,
    io::Write,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Duration,
};

use futures::Future;
use rand::{thread_rng, Rng};
#[cfg(not(doc))]
use tokio::{
    pin, select, spawn,
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        Mutex, MutexGuard,
    },
    time::{sleep, sleep_until, Instant},
};
use tracing::trace;

#[cfg(not(doc))]
use crate::stage_prod::State;
use crate::{
    common::{Config, SigningKey},
    facade::{self, Receiver},
};

type Address = String;
type Message = Vec<u8>;

pub struct Transport {
    #[cfg(not(doc))]
    rx: UnboundedReceiver<(Address, Address, Message, bool)>,
    #[cfg(not(doc))]
    tx: UnboundedSender<(Address, Address, Message, bool)>,
    recv_table: RecvTable,
    // multicast recv table
    filter_table: FilterTable,
}
type RecvTable = HashMap<Address, Box<dyn Fn(Address, RxBuffer) + Send>>;
type FilterTable =
    HashMap<u32, Box<dyn Fn(&Address, &Address, &[u8], &mut Duration) -> bool + Send>>;

#[derive(Debug, Clone)]
pub struct RxBuffer(Message);
impl AsRef<[u8]> for RxBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone)]
pub struct TxAgent {
    #[cfg(not(doc))]
    tx: UnboundedSender<(Address, Address, Message, bool)>,
}

impl facade::TxAgent for TxAgent {
    type Transport = Transport;

    fn send_message(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest: &<Self::Transport as facade::Transport>::Address,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        let mut buffer = [0; 9000];
        let message_length = message(&mut buffer);
        let message = buffer[..message_length as usize].to_vec();
        self.tx
            .send((source.get_address().clone(), dest.clone(), message, false))
            .unwrap();
    }
    fn send_message_to_all(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest_list: &[<Self::Transport as facade::Transport>::Address],
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        let mut buffer = [0; 9000];
        let message_length = message(&mut buffer);
        let message = buffer[..message_length as usize].to_vec();
        for dest in dest_list {
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
}

impl facade::Transport for Transport {
    type Address = Address;
    type RxBuffer = RxBuffer;
    type TxAgent = TxAgent;

    fn tx_agent(&self) -> Self::TxAgent {
        TxAgent {
            tx: self.tx.clone(),
        }
    }

    fn register(
        &mut self,
        receiver: &impl Receiver<Self>,
        rx_agent: impl Fn(Self::Address, Self::RxBuffer) + 'static + Send,
    ) where
        Self: Sized,
    {
        self.recv_table
            .insert(receiver.get_address().clone(), Box::new(rx_agent));
    }

    fn register_multicast(
        &mut self,
        rx_agent: impl Fn(Self::Address, Self::RxBuffer) + 'static + Send,
    ) {
        todo!()
    }

    fn ephemeral_address(&self) -> Self::Address {
        let mut label = 'A' as u32;
        loop {
            let address = format!("client-{}", char::from_u32(label).unwrap());
            if !self.recv_table.contains_key(&address) {
                return address;
            }
            label += 1;
        }
    }
}

impl Transport {
    pub fn config_builder(n_replica: usize, n_fault: usize) -> impl Fn() -> Config<Self> {
        move || {
            let replica: Vec<_> = (0..n_replica).map(|i| format!("replica-{}", i)).collect();
            Config::for_shard(
                facade::Config {
                    group: Vec::new(), // TODO
                    multicast: None,   // TODO
                    f: n_fault,
                    signing_key: replica
                        .iter()
                        .map(|address| {
                            let mut signing_key = [0; 32];
                            signing_key
                                .as_mut_slice()
                                .write(address.as_bytes())
                                .unwrap();
                            (
                                address.clone(),
                                SigningKey::from_bytes(&signing_key).unwrap(),
                            )
                        })
                        .collect(),
                    replica,
                },
                0,
            )
        }
    }

    pub fn new(_config: Config<Self>) -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            rx,
            tx,
            recv_table: HashMap::new(),
            filter_table: HashMap::new(),
        }
    }

    pub async fn deliver(&mut self, duration: Duration) {
        let start = Instant::now();
        let deadline = start + duration;
        loop {
            #[cfg(not(doc))]
            select! {
                _ = sleep_until(deadline) => break,
                Some((source, dest, message, filtered)) = self.rx.recv() => {
                    self.deliver_internal(source, dest, message, filtered, start);
               }
            }
        }
    }

    pub async fn deliver_until<T>(&mut self, predict: impl Future<Output = T>) {
        let start = Instant::now();
        #[cfg(not(doc))]
        pin!(predict);
        loop {
            #[cfg(not(doc))]
            select! {
                _ = &mut predict => break,
                Some((source, dest, message, filtered)) = self.rx.recv() => {
                    self.deliver_internal(source, dest, message, filtered, start)
                }
            }
        }
    }

    #[cfg(not(doc))]
    fn deliver_internal(
        &self,
        source: Address,
        dest: Address,
        message: Message,
        filtered: bool,
        start: Instant,
    ) {
        if filtered {
            (self.recv_table.get(&dest).unwrap())(source, RxBuffer(message));
            return;
        }

        let mut delay = Duration::ZERO;
        let mut drop = false;
        for filter in self.filter_table.values() {
            if !filter(&source, &dest, &message, &mut delay) {
                drop = true;
                break;
            }
        }
        trace!(
            "{:?} {} -> {} [message size = {}] {}",
            Instant::now() - start,
            source,
            dest,
            message.len(),
            if drop {
                "[drop]".to_string()
            } else {
                format!("[delay = {:?}]", delay)
            }
        );

        if !drop {
            let tx = self.tx.clone();
            spawn(async move {
                sleep(delay).await;
                tx.send((source, dest, message, true)).unwrap();
            });
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

#[cfg(not(doc))]
pub use undoc::*;
#[cfg(not(doc))]
mod undoc {
    use super::*;

    // actually what I want is a Executor which can only pair with
    // simulated::Transport, only T: transport::Transport
    // but Rust does not have specialization, and the corresponding RFC seems
    // stalled
    // then the only approach I can think of is to add constrait when implementing
    // trait, but for stage I decide to do conditional compiling instead of
    // trait
    // really hope this would be solved
    pub struct Handle<S: State>(Submit<S>);

    impl<S: State> From<S> for Handle<S> {
        fn from(state: S) -> Self {
            Self(Submit {
                shared: state.shared(),
                state: Arc::new(Mutex::new(state)),
            })
        }
    }

    impl<S: State> Handle<S> {
        pub fn with_stateful(&self, f: impl FnOnce(&mut StatefulContext<'_, S>)) {
            f(&mut StatefulContext {
                state: self.0.state.try_lock().unwrap(),
                submit: self.0.clone(),
            });
        }

        pub fn with_stateless(&self, f: impl FnOnce(&StatelessContext<S>)) {
            f(&StatelessContext {
                shared: self.0.shared.clone(),
                submit: self.0.clone(),
            });
        }
    }

    pub struct StatefulContext<'a, S: State> {
        state: MutexGuard<'a, S>,
        pub submit: Submit<S>,
    }

    pub struct StatelessContext<S: State> {
        shared: S::Shared,
        pub submit: Submit<S>,
    }

    impl<S: State> Clone for StatelessContext<S> {
        fn clone(&self) -> Self {
            Self {
                shared: self.shared.clone(),
                submit: self.submit.clone(),
            }
        }
    }

    impl<'a, S: State> Deref for StatefulContext<'a, S> {
        type Target = S;
        fn deref(&self) -> &Self::Target {
            &*self.state
        }
    }

    impl<'a, S: State> DerefMut for StatefulContext<'a, S> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut *self.state
        }
    }

    impl<S: State> Deref for StatelessContext<S> {
        type Target = S::Shared;
        fn deref(&self) -> &Self::Target {
            &self.shared
        }
    }

    pub struct Submit<S: State> {
        state: Arc<Mutex<S>>,
        shared: S::Shared,
    }

    impl<S: State> Clone for Submit<S> {
        fn clone(&self) -> Self {
            Self {
                state: self.state.clone(),
                shared: self.shared.clone(),
            }
        }
    }

    impl<S: State> Submit<S> {
        pub fn stateful(
            &self,
            task: impl for<'a> FnOnce(&mut StatefulContext<'a, S>) + Send + 'static,
        ) where
            S: Send + 'static,
        {
            let submit = self.clone();
            spawn(async move {
                task(&mut StatefulContext {
                    state: submit.state.lock().await,
                    submit: submit.clone(),
                });
            });
        }

        pub fn stateless(&self, task: impl FnOnce(&StatelessContext<S>) + Send + 'static)
        where
            S: Send + 'static,
        {
            let submit = self.clone();
            spawn(async move {
                task(&StatelessContext {
                    submit: submit.clone(),
                    shared: submit.shared,
                });
            });
        }
    }
}
