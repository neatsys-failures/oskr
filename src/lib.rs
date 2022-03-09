use async_trait::async_trait;
use common::Opaque;
use std::{future::Future, time::Duration};

pub mod transport;

pub mod dpdk;
pub mod dpdk_shim;

#[cfg(test)]
pub mod simulated;

#[cfg(not(test))]
pub mod stage;
#[cfg(test)]
#[path = "stage.rs"]
pub mod stage_prod; // for production
#[cfg(test)]
pub mod stage {
    pub use crate::simulated::{Handle, StatefulContext, StatelessContext, Submit};
    pub use crate::stage_prod::State;
}

pub mod common;

pub mod replication {
    pub mod pbft;
    pub mod unreplicated;
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
        pub static ref TRACING: () = {
            tracing_subscriber::fmt::init();
            // panic_abort();
        };
    }
}
