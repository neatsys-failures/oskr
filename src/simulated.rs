use crate::model::{Transport as TransportTrait, *};
use ::futures::future::FutureExt;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::future::Future;
use std::panic::*;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::Arc;
use std::thread::Result as CatchResult;
use std::time::Duration;
use tokio::runtime::Builder;
use tokio::sync::mpsc::*;
use tokio::task::*;
use tokio::time::sleep;

pub struct Transport {
    config: Arc<Config>,
    inbox_table: HashMap<TransportAddress, Box<dyn Send + Sync + Fn(&TransportAddress, RxBuffer)>>,
    shutdown_tx: Sender<CatchResult<()>>,
}

impl Transport {
    pub fn register(&mut self, receiver: &dyn TransportReceiver) {
        self.inbox_table
            .insert(receiver.get_address().clone(), receiver.get_inbox());
    }
}

impl TransportTrait for Transport {
    fn deref_rx_buffer(&self, data: RxBufferData) -> &[u8] {
        unsafe { data.0.as_ref() }.downcast_ref::<Vec<_>>().unwrap()
    }

    fn drop_rx_buffer(&self, mut data: RxBufferData) {
        drop(unsafe { Box::from_raw(data.0.as_mut().downcast_mut::<Vec<u8>>().unwrap()) });
    }

    fn send_message(
        self: Arc<Self>,
        source: &dyn TransportReceiver,
        dest: &TransportAddress,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        let mut buffer = [0; 9000];
        let length = message(&mut buffer) as usize;
        let buffer: Box<Vec<_>> = Box::new((&buffer[..length]).into());
        let buffer = RxBuffer {
            data: RxBufferData(NonNull::new(Box::leak(buffer)).unwrap()),
            transport: self.clone(),
        };
        self.inbox_table.get(dest).unwrap()(source.get_address(), buffer);
    }

    fn send_message_to_replica(
        self: Arc<Self>,
        source: &dyn TransportReceiver,
        id: ReplicaId,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        let dest = &self.config.address_list[id as usize];
        self.clone().send_message(source, dest, message);
    }

    fn send_message_to_all(
        self: Arc<Self>,
        source: &dyn TransportReceiver,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        let mut buffer = [0; 9000];
        let length = message(&mut buffer) as usize;
        for dest in &self.config.address_list {
            if dest != source.get_address() {
                let buffer: Box<Vec<_>> = Box::new((&buffer[..length]).into());
                let buffer = RxBuffer {
                    data: RxBufferData(NonNull::new(Box::leak(buffer)).unwrap()),
                    transport: self.clone(),
                };
                self.inbox_table.get(dest).unwrap()(source.get_address(), buffer);
            }
        }
    }
}

pub trait TestSystem {
    type Blueprint;
    fn new(transport: &mut Transport, blueprint: Self::Blueprint) -> Self;
    fn set_up(&mut self, transport: Arc<Transport>);
    fn tear_down(self);
}

impl Transport {
    pub fn address(name: &str) -> TransportAddress {
        TransportAddress(SmallVec::from_slice(name.as_bytes()))
    }

    pub fn shutdown(&self, result: CatchResult<()>) {
        // fail means someone else has shutdown already, so not shutdown again
        let _ = self.shutdown_tx.try_send(result);
    }

    pub fn time_limit(self: Arc<Transport>, duration: Duration) {
        spawn_local(async move {
            sleep(duration).await;
            self.shutdown(catch_unwind(|| panic!("timeout after {:?}", duration)));
        });
    }

    pub async fn run<S: 'static + TestSystem, F>(config: Arc<Config>, blueprint: S::Blueprint, f: F)
    where
        S::Blueprint: Send,
        F: 'static
            + Send
            + for<'s> FnOnce(&'s mut S, Arc<Transport>) -> Pin<Box<dyn 's + Future<Output = ()>>>,
    {
        let result = spawn_blocking(|| {
            let (shutdown_tx, mut shutdown_rx) = channel(1);
            let mut transport = Transport {
                config,
                inbox_table: HashMap::new(),
                shutdown_tx,
            };

            let runtime = Builder::new_current_thread().enable_time().build().unwrap();
            let result = runtime.block_on(LocalSet::new().run_until(async move {
                let mut system = S::new(&mut transport, blueprint);
                let transport = Arc::new(transport);
                spawn_local(async move {
                    system.set_up(transport.clone());
                    let result = AssertUnwindSafe(f(&mut system, transport.clone()))
                        .catch_unwind()
                        .await;
                    system.tear_down();
                    transport.shutdown(result);
                });
                shutdown_rx.recv().await.unwrap()
            }));

            runtime.shutdown_background();
            result
        })
        .await
        .unwrap();

        if let Err(error) = result {
            resume_unwind(error);
        }
    }
}
