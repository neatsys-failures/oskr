use core_affinity::CoreId;
use oskr::common::Opaque;
use oskr::replication::unreplicated;
use oskr::transport::dpdk::{rte_thread_register, Transport};
use oskr::transport::Config;
use oskr::App;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Builder;
use tokio::task::spawn_blocking;

struct NullApp;
impl App for NullApp {
    fn execute(&mut self, _op: Opaque) -> Opaque {
        Opaque::default()
    }
}

fn main() {
    let core_mask: u128 = 0x03; // TODO
    let rx_core_mask: u128 = 0x01;
    let port_id = 0;
    let replica_id = 0;
    let config = Config {
        replica_address: vec!["b8:ce:f6:2a:2f:94#0".parse().unwrap()],
        multicast_address: None,
        n_fault: 0,
    };

    let (rx_core, worker_core): (Vec<_>, Vec<_>) = (0..128)
        .map(|id| CoreId { id })
        .filter(|core| 1_u128 << core.id & core_mask != 0)
        .partition(|core| 1_u128 << core.id & rx_core_mask != 0);
    assert_eq!(rx_core.len(), 1);
    let rx_core = rx_core[0];
    println!(
        "set up rx on {:?} and {} worker core",
        rx_core,
        worker_core.len()
    );

    let worker_threads = worker_core.len();
    let worker_core = Arc::new(Mutex::new(worker_core));
    let thread_start = move || {
        let worker_core = worker_core.clone();
        move || {
            if let Some(core) = worker_core.lock().unwrap().pop() {
                core_affinity::set_for_current(core);
            }
            assert_eq!(unsafe { rte_thread_register() }, 0);
        }
    };

    let mut transport = Transport::setup(config, port_id, 1);
    let runtime = Builder::new_multi_thread()
        .enable_time()
        .worker_threads(worker_threads)
        .thread_keep_alive(Duration::MAX)
        .on_thread_start(thread_start())
        .build()
        .unwrap();

    runtime.block_on(async move {
        // TODO select replica and app
        let mut replica = unreplicated::Replica::new(&transport, replica_id, NullApp);
        replica.register(&mut transport);

        let rx = spawn_blocking(move || {
            core_affinity::set_for_current(rx_core);
            transport.run(0);
        });

        replica.run().await;
        rx.abort();
    });
}
