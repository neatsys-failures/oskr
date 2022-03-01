pub mod common;
pub mod executor;
pub mod transport;

pub mod dpdk_shim;

pub mod replication {
    pub mod unreplicated;
}

pub mod app {
    pub mod mock;
}

use async_trait::async_trait;
use common::Opaque;

#[async_trait]
pub trait Invoke {
    async fn invoke(&mut self, op: Opaque) -> Opaque;
}

pub trait App {
    fn execute(&mut self, op: Opaque) -> Opaque;
}
