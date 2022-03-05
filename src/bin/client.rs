use std::{
    ffi::c_void,
    future::Future,
    mem::take,
    pin::Pin,
    process,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Barrier, Mutex,
    },
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot, future::BoxFuture, select, task::noop_waker_ref, FutureExt};
use hdrhistogram::Histogram;
use oskr::{
    common::Opaque,
    dpdk::Transport,
    dpdk_shim::{rte_eal_mp_remote_launch, rte_rmt_call_main_t},
    replication::unreplicated,
    transport::{Config, Receiver},
    Invoke,
};
use quanta::{Clock, Instant};

struct Timeout(Instant);
impl Timeout {
    fn boxed() -> BoxFuture<'static, ()> {
        Box::pin(Self(Instant::now()))
    }
}
impl Future for Timeout {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // configurable?
        if Instant::now().duration_since(self.0) >= Duration::from_millis(1000) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let port_id = 0;
    let n_worker = 20;
    let n_client = 20;
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
                .map(|_| unreplicated::Client::register_new(&mut transport, Timeout::boxed))
                .collect()
        })
        .collect();

    struct WorkerData<Client> {
        client_list: Vec<Vec<Client>>,
        count: Arc<AtomicU32>,
        duration_second: u64,
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
        // TODO take a safer shared reference
        let worker_data: &mut WorkerData<Client> = unsafe { &mut *(arg as *mut _) };
        let worker_id = Transport::worker_id();
        let client_list = if let Some(client_list) = worker_data.client_list.get_mut(worker_id) {
            take(client_list)
        } else {
            return 0;
        };
        let count = worker_data.count.clone();
        let duration_second = worker_data.duration_second;
        let barrier = worker_data.barrier.clone();
        let hist = worker_data.hist.clone();

        // I want a broadcast :|
        let (mut client_list, shutdown_list): (Vec<_>, Vec<_>) = client_list
            .into_iter()
            .map(|mut client| {
                let count = count.clone();
                let (shutdown_tx, mut shutdown) = oneshot::channel();
                (
                    Box::pin(async move {
                        println!("{}", client.get_address());
                        let clock = Clock::new();
                        let mut hist: Histogram<u64> = Histogram::new(2).unwrap();
                        loop {
                            let start = clock.start();
                            select! {
                                _ = client.invoke(Opaque::default()).fuse() => {},
                                _ = shutdown => return hist,
                            }
                            let end = clock.end();
                            hist += clock.delta(start, end).as_nanos() as u64;
                            count.fetch_add(1, Ordering::SeqCst);
                        }
                    }),
                    shutdown_tx,
                )
            })
            .unzip();

        let poll_clock = Clock::new();
        let mut poll = |duration| {
            let start = poll_clock.start();
            while poll_clock.delta(start, poll_clock.end()) < duration {
                for mut client in &mut client_list {
                    let _ = Pin::new(&mut client).poll(&mut Context::from_waker(noop_waker_ref()));
                }
            }
        };
        if worker_id == 0 {
            for _ in 0..duration_second {
                poll(Duration::from_secs(1));
                let count = count.swap(0, Ordering::SeqCst);
                println!("{}", count);
            }
        } else {
            poll(Duration::from_secs(duration_second));
        }

        for shutdown in shutdown_list {
            shutdown.send(()).unwrap();
        }
        let mut worker_hist = Histogram::new(2).unwrap();
        for mut client in client_list {
            if let Poll::Ready(hist) =
                Pin::new(&mut client).poll(&mut Context::from_waker(noop_waker_ref()))
            {
                worker_hist += hist;
            } else {
                unreachable!();
            }
        }

        *hist.lock().unwrap() += worker_hist;
        if barrier.wait().is_leader() {
            for v in hist.lock().unwrap().iter_quantiles(1) {
                if v.count_since_last_iteration() == 0 {
                    continue;
                }
                println!(
                    "{:.7} {}ns\twith {} samples",
                    v.quantile_iterated_to(),
                    v.value_iterated_to(),
                    v.count_since_last_iteration()
                );
            }
            process::exit(0); // TODO more graceful
        } else {
            0
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
