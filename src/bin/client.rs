use oskr::common::Opaque;
use oskr::replication::unreplicated;
use oskr::transport::dpdk::Transport;
use oskr::transport::Config;
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
    let n_client = 1;

    let (rx_core, worker_core): (Vec<_>, Vec<_>) = core_affinity::get_core_ids()
        .unwrap()
        .into_iter()
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
            let core = worker_core.lock().unwrap().pop().unwrap();
            core_affinity::set_for_current(core);
        }
    };
    let runtime = Builder::new_multi_thread()
        .enable_time()
        .worker_threads(worker_threads)
        .thread_keep_alive(Duration::MAX)
        .on_thread_start(thread_start())
        .build()
        .unwrap();

    let config = Config {
        replica_address: vec![],
        multicast_address: None,
        n_fault: 0,
    };
    let mut transport = Transport::setup(config, port_id, 1);

    runtime.block_on(async move {
        let latency_list = Arc::new(Mutex::new(Vec::new()));
        let client_list: Vec<_> = iter::repeat(latency_list.clone())
            .map(|latency_list| {
                // TODO select client
                let mut client = unreplicated::Client::new(&transport);
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
            let interval_list: Vec<_> = latency_list.lock().await.drain(..).collect();
            println!("throughput: {} ops / sec", interval_list.len());
        }
        client_list.into_iter().for_each(|client| client.abort());
        rx.abort();
    });
}
