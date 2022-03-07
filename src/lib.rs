use async_trait::async_trait;
use common::Opaque;
use std::{future::Future, time::Duration};

pub mod transport;

pub mod dpdk;
pub mod dpdk_shim;

#[cfg(test)]
pub mod simulated;

#[cfg(not(test))]
pub mod director;
#[cfg(test)]
pub mod director {
    pub use crate::simulated::{Director, StatefulContext, Submit};
}

pub mod common;

pub mod replication {
    pub mod unreplicated;
    pub mod pbft;
}

pub mod app {
    pub mod mock;
}

#[async_trait]
pub trait Invoke {
    async fn invoke(&mut self, op: Opaque) -> Opaque;
}

pub trait App {
    fn execute(&mut self, op: Opaque) -> Opaque;
}

pub trait AsyncExecutor<'a, T> {
    type JoinHandle: Future<Output = T>;
    type Timeout: Future<Output = Result<T, Self::Elapsed>> + Send;
    type Elapsed;
    fn spawn(task: impl Future<Output = T> + Send + 'static) -> Self::JoinHandle;
    fn timeout(duration: Duration, task: impl Future<Output = T> + Send + 'a) -> Self::Timeout;
}

#[cfg(test)]
pub mod tests {
    use lazy_static::lazy_static;
    lazy_static! {
        pub static ref TRACING: () = tracing_subscriber::fmt::init();
    }
}
