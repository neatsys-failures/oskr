pub mod common;
pub mod transport;
pub mod replication {
    pub mod unreplicated;
}
pub mod app {
    pub mod mock;
}

use common::Opaque;
use std::future::Future;

pub trait Invoke {
    type Future: Future<Output = Opaque> + Send;
    // has to implement against `&'a mut Client` to get a chance to acquire lifetime
    // still waiting for GAT
    fn invoke(self, op: Opaque) -> Self::Future;
}

pub trait App {
    fn execute(&mut self, op: Opaque) -> Opaque;
}
