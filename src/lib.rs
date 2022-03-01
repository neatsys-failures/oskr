pub mod common;
pub mod transport;
pub mod replication {
    pub mod unreplicated;
}
pub mod app {
    pub mod mock;
}
pub mod dpdk_shim;

use async_trait::async_trait;
use common::Opaque;

#[async_trait]
pub trait Invoke {
    async fn invoke(&mut self, op: Opaque) -> Opaque;
}

pub trait App {
    fn execute(&mut self, op: Opaque) -> Opaque;
}
