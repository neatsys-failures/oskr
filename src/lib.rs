use async_trait::async_trait;
use common::Opaque;

pub mod transport;

pub mod dpdk;
pub mod dpdk_shim;

#[cfg(test)]
pub mod simulated;

#[cfg(not(test))]
pub mod executor;
#[cfg(test)]
pub mod executor {
    pub use crate::simulated::{Executor, StatefulContext, Submit};
}

pub mod common;

pub mod replication {
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

#[cfg(test)]
pub mod tests {
    use lazy_static::lazy_static;
    lazy_static! {
        pub static ref TRACING: () = tracing_subscriber::fmt::init();
    }
}
