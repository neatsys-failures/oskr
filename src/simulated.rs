use crate::model::{Transport as TransportTrait, *};
use ::futures::future::FutureExt;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::future::Future;
use std::panic::*;
use std::pin::Pin;
use std::sync::Arc;
use std::thread::Result as CatchResult;
use std::time::Duration;
use tokio::runtime::Builder;
use tokio::sync::mpsc::*;
use tokio::task::*;
use tokio::time::sleep;

pub struct Transport {
    config: Arc<Config>,
    inbox_table: HashMap<TransportAddress, Box<dyn Fn(&TransportAddress, RxBuffer)>>,
    drop_tx: UnboundedSender<RxBufferData>,
    shutdown_tx: Sender<CatchResult<()>>,
}

unsafe impl Sync for Transport {}

impl Transport {
    pub fn register(&mut self, receiver: &dyn TransportReceiver) {
        self.inbox_table
            .insert(receiver.get_address().clone(), receiver.get_inbox());
    }
}

impl TransportTrait for Transport {
    fn send_message(
        &self,
        source: &dyn TransportReceiver,
        dest: &TransportAddress,
        message: &mut dyn FnMut(&mut [u8]) -> u64,
    ) {
        let mut buffer = Box::new([0; 9000]);
        let length = message(&mut *buffer) as usize;
        let buffer = RxBuffer {
            data: RxBufferData(Box::leak(buffer) as *const u8),
            length,
            drop_tx: self.drop_tx.clone(),
        };
        self.inbox_table.get(dest).unwrap()(source.get_address(), buffer);
    }

    fn send_message_to_replica(
        &self,
        source: &dyn TransportReceiver,
        id: ReplicaId,
        message: &mut dyn FnMut(&mut [u8]) -> u64,
    ) {
        self.send_message(source, &self.config.address_list[id as usize], message);
    }

    fn send_message_to_all(
        &self,
        source: &dyn TransportReceiver,
        message: &mut dyn FnMut(&mut [u8]) -> u64,
    ) {
        let mut buffer = [0; 9000];
        let length = message(&mut buffer) as usize;
        for dest in &self.config.address_list {
            if dest != source.get_address() {
                let buffer = Box::new(buffer);
                let buffer = RxBuffer {
                    data: RxBufferData(Box::leak(buffer) as *const u8),
                    length,
                    drop_tx: self.drop_tx.clone(),
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
        let (drop_tx, mut drop_rx): (_, UnboundedReceiver<RxBufferData>) = unbounded_channel();
        let drop_thread = spawn(async move {
            while let Some(data) = drop_rx.recv().await {
                drop(unsafe { Box::from_raw(data.0 as *mut [u8; 9000]) });
            }
        });

        let result = spawn_blocking(|| {
            let (shutdown_tx, mut shutdown_rx) = channel(1);
            let mut transport = Transport {
                config,
                inbox_table: HashMap::new(),
                drop_tx,
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

        drop_thread.await.unwrap();
        if let Err(error) = result {
            resume_unwind(error);
        }
    }
}
