use std::{
    ffi::c_void,
    mem::take,
    process,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Barrier, Mutex,
    },
    time::Duration,
};

use async_std::{
    channel::bounded,
    prelude::FutureExt,
    task::{block_on, sleep, spawn},
};
use futures::future::join_all;
use hdrhistogram::Histogram;
use oskr::{
    common::Opaque,
    dpdk_shim::{oskr_lcore_id, rte_eal_mp_remote_launch, rte_rmt_call_main_t},
    replication::unreplicated,
    transport::{dpdk::Transport, Config, Receiver},
    Invoke,
};
use quanta::Clock;

fn main() {
    let port_id = 0;
    let n_worker = 1;
    let n_client = 1;
    let duration_second = 10;
    let config = Config {
        replica_address: vec!["b8:ce:f6:2a:2f:94#0".parse().unwrap()],
        n_fault: 0,
        multicast_address: None,
    };

    let mut transport = Transport::setup(config, port_id, 1, n_worker);
    let k = (n_client - 1) / n_worker + 1;
    let client_list: Vec<Vec<_>> = (0..n_worker)
        .map(|i| {
            (i * k..n_client.min((i + 1) * k))
                // TODO select client type
                .map(|_| unreplicated::Client::register_new(&mut transport))
                .collect()
        })
        .collect();

    struct WorkerData<Client> {
        client_list: Vec<Vec<Client>>,
        count: Arc<AtomicU32>,
        duration_second: u32,
        hist: Arc<Mutex<Histogram<u64>>>,
        barrier: Arc<Barrier>,
    }
    let mut worker_data = WorkerData {
        client_list,
        count: Arc::new(AtomicU32::new(0)),
        duration_second,
        hist: Arc::new(Mutex::new(Histogram::new(2).unwrap())),
        barrier: Arc::new(Barrier::new(n_worker as usize)),
    };
    extern "C" fn worker<Client: Receiver<Transport> + Invoke + Send + 'static>(
        arg: *mut c_void,
    ) -> i32 {
        let worker_data: &mut WorkerData<Client> = unsafe { &mut *(arg as *mut _) };
        let client_list = if let Some(client_list) = worker_data
            .client_list
            .get_mut(unsafe { oskr_lcore_id() } as usize - 1)
        {
            println!("client count: {}", client_list.len());
            take(client_list)
        } else {
            return 0;
        };
        let count = worker_data.count.clone();
        let duration_second = worker_data.duration_second;
        let barrier = worker_data.barrier.clone();
        let hist = worker_data.hist.clone();
        drop(worker_data);

        let worker_hist: Histogram<_> = block_on(async move {
            let (shutdown_tx, shutdown) = bounded(1);
            let client_list: Vec<_> = client_list
                .into_iter()
                .map(|mut client| {
                    let shutdown = shutdown.clone();
                    let count = count.clone();
                    spawn(async move {
                        println!("{}", client.get_address());
                        let clock = Clock::new();
                        let mut hist: Histogram<u64> = Histogram::new(2).unwrap();
                        loop {
                            let start = clock.start();
                            if async {
                                client.invoke(Opaque::default()).await;
                                false
                            }
                            .race(async {
                                shutdown.recv().await.unwrap();
                                true
                            })
                            .await
                            {
                                return hist;
                            }
                            let end = clock.end();
                            hist += clock.delta(start, end).as_nanos() as u64;
                            count.fetch_add(1, Ordering::SeqCst);
                        }
                    })
                })
                .collect();

            if unsafe { oskr_lcore_id() } == 1 {
                for _ in 0..duration_second {
                    sleep(Duration::from_secs(1)).await;
                    let count = count.swap(0, Ordering::SeqCst);
                    println!("{}", count);
                }
            } else {
                sleep(Duration::from_secs(duration_second as u64 + 1)).await;
            }

            for _ in 0..client_list.len() {
                shutdown_tx.send(()).await.unwrap();
            }
            join_all(client_list).await.into_iter().sum()
        });

        hist.lock().unwrap().add(worker_hist).unwrap();
        if barrier.wait().is_leader() {
            for v in hist.lock().unwrap().iter_quantiles(1) {
                println!(
                    "quantile {:.7} latency {}ns\tcount {}",
                    v.quantile_iterated_to(),
                    v.value_iterated_to(),
                    v.count_since_last_iteration()
                );
            }
            process::exit(0); // TODO more graceful
        } else {
            loop {}
        }
    }
    unsafe {
        rte_eal_mp_remote_launch(
            worker::<unreplicated::Client<Transport>>,
            &mut worker_data as *mut _ as *mut _,
            rte_rmt_call_main_t::SKIP_MAIN,
        );
    }

    transport.run(0);
}
