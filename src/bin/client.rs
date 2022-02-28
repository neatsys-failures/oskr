use core_affinity::CoreId;
use oskr::common::Opaque;
use oskr::replication::unreplicated;
use oskr::transport::dpdk::rte_thread_register;
use oskr::transport::dpdk::Transport;
use oskr::transport::Config;
use oskr::transport::Receiver;
use oskr::Invoke;
use std::iter;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::Duration;
use tokio::runtime::Builder;
use tokio::sync::Mutex;
use tokio::task::{spawn, spawn_blocking};
use tokio::time::sleep;

fn main() {
    let core_mask: u128 = 0xff; // TODO
    let rx_core_mask: u128 = 0x1;
    let port_id = 0;
    let duration = 10;
    let n_client = 8;

    let config = Config {
        replica_address: vec!["b8:ce:f6:2a:2f:94#0".parse().unwrap()],
        multicast_address: None,
        n_fault: 0,
    };
    let mut transport = Transport::setup(config, port_id, 1);

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
    let worker_core = Arc::new(SyncMutex::new(worker_core));
    let thread_start = move || {
        let worker_core = worker_core.clone();
        move || {
            if let Some(core) = worker_core.lock().unwrap().pop() {
                core_affinity::set_for_current(core);
            }
            assert_eq!(unsafe { rte_thread_register() }, 0);
        }
    };
    let runtime = Builder::new_multi_thread()
        .enable_time()
        .worker_threads(worker_threads)
        .thread_keep_alive(Duration::MAX)
        .on_thread_start(thread_start())
        .build()
        .unwrap();
    runtime.block_on(async move {
        let latency_list = Arc::new(Mutex::new(Vec::new()));
        let client_list: Vec<_> = iter::repeat(latency_list.clone())
            .map(|latency_list| {
                // TODO select client
                let mut client = unreplicated::Client::new(&transport);
                println!("client address {}", client.get_address());
                client.register(&mut transport);
                spawn(async move {
                    loop {
                        client.invoke(Opaque::default()).await;
                        latency_list.lock().await.push(0); // TODO
                    }
                })
            })
            .take(n_client)
            .collect();

        let rx = spawn_blocking(move || {
            core_affinity::set_for_current(rx_core);
            transport.run(0);
        });

        for _ in 0..duration {
            sleep(Duration::from_secs(1)).await;
            let mut interval_list: Vec<_> = latency_list.lock().await.drain(..).collect();
            interval_list.sort_unstable();
            println!("throughput: {} ops / sec", interval_list.len());
        }

        for client in client_list.into_iter() {
            client.abort();
        }
        rx.abort();
    });
    runtime.shutdown_background();
}
