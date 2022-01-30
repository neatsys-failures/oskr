use oskr::dpdk::Transport;
use oskr::model::*;
use oskr::replication::unreplicated;
use std::sync::Arc;
use tokio::runtime::Builder;
use tokio::task::*;

struct NullApp;
impl App for NullApp {
    fn execute(&mut self, _op: &[u8]) -> Data {
        Data::new()
    }
}

fn main() {
    let config = Arc::new(Config {
        f: 0,
        address_list: vec![], // todo
    });
    let runtime = Builder::new_multi_thread().enable_time().build().unwrap();
    runtime.block_on(async move {
        let mut replica = unreplicated::Replica::new(config.clone(), Box::new(NullApp));
        spawn_blocking(Transport::run_single(
            config,
            &mut replica,
            unreplicated::Replica::init,
        ))
        .await
        .unwrap();
    });
}
