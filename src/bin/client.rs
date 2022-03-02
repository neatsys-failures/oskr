use std::{
    ffi::c_void,
    mem::take,
    process,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use oskr::{
    common::Opaque,
    dpdk_shim::{oskr_lcore_id, rte_eal_mp_remote_launch, rte_rmt_call_main_t},
    replication::unreplicated,
    transport::{dpdk::Transport, Config, Receiver},
    Invoke,
};
use tokio::{runtime, spawn, time::sleep};

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
    let client_list: Vec<Vec<_>> = (0..n_worker)
        .map(|i| {
            (i * n_worker..n_client.min((i + 1) * n_worker))
                .map(|_| unreplicated::Client::register_new(&mut transport))
                .collect()
        })
        .collect();

    struct WorkerData<Client> {
        client_list: Vec<Vec<Client>>,
        count: Arc<AtomicU32>,
        duration_second: u32,
    }
    // TODO prevent leak memory
    let worker_data = Box::leak(Box::new(WorkerData {
        client_list,
        count: Arc::new(AtomicU32::new(0)),
        duration_second,
    }));
    extern "C" fn worker<Client: Receiver<Transport> + Invoke + Send + 'static>(
        arg: *mut c_void,
    ) -> i32 {
        let worker_data: &mut WorkerData<Client> = unsafe { &mut *(arg as *mut _) };
        let client_list = if let Some(client_list) = worker_data
            .client_list
            .get_mut(unsafe { oskr_lcore_id() } as usize - 1)
        {
            take(client_list)
        } else {
            return 0;
        };
        let count = &worker_data.count;
        let duration_second = worker_data.duration_second;

        let runtime = runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async move {
            for mut client in client_list {
                spawn(async move {
                    println!("{}", client.get_address());
                    loop {
                        let _ = client.invoke(Opaque::default()).await;
                        count.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
            if unsafe { oskr_lcore_id() } == 1 {
                for _ in 0..duration_second {
                    sleep(Duration::from_secs(1)).await;
                    let count = count.swap(0, Ordering::SeqCst);
                    println!("{}", count);
                }
                process::exit(0); // TODO more graceful
            } else {
                sleep(Duration::from_secs(duration_second as u64 + 1)).await;
            }
        });
        unreachable!()
    }
    unsafe {
        rte_eal_mp_remote_launch(
            worker::<unreplicated::Client<Transport>>,
            worker_data as *mut _ as *mut _,
            rte_rmt_call_main_t::SKIP_MAIN,
        );
    }

    transport.run(0);
}
