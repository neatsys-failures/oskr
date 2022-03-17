use std::{
    cell::RefCell,
    ffi::c_void,
    fs::File,
    future::Future,
    io::Read,
    mem::take,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use clap::{ArgEnum, Parser};
use futures::{channel::oneshot, future::BoxFuture, select, task::noop_waker_ref, FutureExt};
use hdrhistogram::SyncHistogram;
use oskr::{
    common::{panic_abort, Opaque},
    dpdk::Transport,
    dpdk_shim::{rte_eal_mp_remote_launch, rte_eal_mp_wait_lcore, rte_rmt_call_main_t},
    latency::{Latency, LocalLatency},
    replication::{pbft, unreplicated},
    transport::{Config, Receiver},
    AsyncExecutor as _, Invoke,
};
use quanta::{Clock, Instant};
use tokio::{
    runtime::{self, Runtime},
    spawn,
    time::{error::Elapsed, sleep, timeout, Timeout},
};
use tracing::{debug, info};

pub struct AsyncExecutor;
impl<'a, T: Send + 'static> oskr::AsyncExecutor<'a, T> for AsyncExecutor {
    type JoinHandle = BoxFuture<'static, T>;
    type Timeout = Timeout<BoxFuture<'a, T>>;
    type Elapsed = Elapsed;

    fn spawn(task: impl Future<Output = T> + Send + 'static) -> Self::JoinHandle {
        Box::pin(async move { spawn(task).await.unwrap() })
    }

    fn timeout(duration: Duration, task: impl Future<Output = T> + Send + 'a) -> Self::Timeout {
        timeout(duration, Box::pin(task))
    }
}

fn main() {
    tracing_subscriber::fmt::init();
    panic_abort();

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ArgEnum)]
    enum Mode {
        Unreplicated,
        PBFT,
    }

    #[derive(Parser, Debug)]
    #[clap(name = "Oskr Client", version)]
    struct Args {
        #[clap(short, long, arg_enum)]
        mode: Mode,
        #[clap(short, long, parse(from_os_str))]
        config: PathBuf,
        #[clap(long, default_value = "000000ff")]
        mask: String,
        #[clap(short, long, default_value_t = 0)]
        port_id: u16,
        #[clap(short, long = "worker-number", default_value_t = 1)]
        n_worker: usize,
        #[clap(long = "tx", default_value_t = 1)]
        n_tx: u16,
        #[clap(short = 't', long = "client-number", default_value_t = 1)]
        n_client: usize,
        #[clap(short, long, default_value_t = 1)]
        duration: u64,
        #[clap(short, long = "warm-up", default_value_t = 0)]
        warm_up_duration: u64,
    }
    let args = Args::parse();
    let core_mask = u128::from_str_radix(&args.mask, 16).unwrap();
    info!("initialize with {} cores", core_mask.count_ones());
    assert!(core_mask.count_ones() > args.n_worker as u32); // strictly greater-than to preserve one rx core

    let prefix = args.config.file_name().unwrap().to_str().unwrap();
    let config = args.config.with_file_name(format!("{}.config", prefix));
    let mut config: Config<_> = {
        let mut buf = String::new();
        File::open(config)
            .unwrap()
            .read_to_string(&mut buf)
            .unwrap();
        buf.parse().unwrap()
    };
    config.collect_signing_key(&args.config);

    let mut transport = Transport::setup(config, core_mask, args.port_id, 1, args.n_tx);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Status {
        WarmUp,
        Run,
        Shutdown,
    }

    struct WorkerData<Client> {
        client_list: Vec<Vec<Client>>,
        count: Arc<AtomicU32>,
        latency: LocalLatency,
        status: Arc<AtomicU32>,
        args: Arc<Args>,
    }
    impl<C: Receiver<Transport> + Invoke + Send + 'static> WorkerData<C> {
        extern "C" fn worker(arg: *mut c_void) -> i32 {
            // TODO take a safer shared reference
            let worker_data: &mut Self = unsafe { &mut *(arg as *mut _) };
            let worker_id = Transport::worker_id();
            let client_list = if let Some(client_list) = worker_data.client_list.get_mut(worker_id)
            {
                take(client_list)
            } else {
                return 0;
            };
            info!("worker {} poll {} clients", worker_id, client_list.len());

            let count = worker_data.count.clone();
            let status = worker_data.status.clone();
            let latency = worker_data.latency.clone();
            let args = worker_data.args.clone();

            runtime::Builder::new_multi_thread()
                .enable_time()
                .build()
                .unwrap()
                .block_on(async move {
                    //

                    // I want a broadcast :|
                    let (client_list, shutdown_list): (Vec<_>, Vec<_>) = client_list
                        .into_iter()
                        .map(|mut client| {
                            let count = count.clone();
                            // we send shutdown actively, for the case when system stuck
                            // and cause client invoking never finish
                            let (shutdown_tx, mut shutdown) = oneshot::channel();
                            let status = status.clone();
                            let mut latency = latency.clone();
                            (
                                spawn(async move {
                                    debug!("{}", client.get_address());
                                    loop {
                                        let measure = latency.measure();
                                        select! {
                                            _ = client.invoke(Opaque::default()).fuse() => {},
                                            _ = shutdown => return,
                                        }
                                        if status.load(Ordering::SeqCst) == Status::Run as _ {
                                            latency += measure;
                                            count.fetch_add(1, Ordering::SeqCst);
                                        }
                                    }
                                }),
                                shutdown_tx,
                            )
                        })
                        .unzip();

                    if worker_id == 0 {
                        sleep(Duration::from_secs(args.warm_up_duration)).await;
                        status.store(Status::Run as _, Ordering::SeqCst);
                        info!("warm up finish");

                        for _ in 0..args.duration {
                            sleep(Duration::from_secs(1)).await;
                            let count = count.swap(0, Ordering::SeqCst);
                            println!("{}", count);
                        }
                        status.store(Status::Shutdown as _, Ordering::SeqCst);
                    } else {
                        sleep(Duration::from_secs(args.warm_up_duration + args.duration)).await;
                    }

                    // with new latency there is no need to join tasks
                    // save it here for debug and future use
                    for shutdown in shutdown_list {
                        shutdown.send(()).unwrap();
                    }
                    for client in client_list {
                        client.await.unwrap();
                    }
                });
            0
        }

        fn launch(
            mut client: impl FnMut() -> C,
            args: Args,
            status: Arc<AtomicU32>,
            latency: LocalLatency,
        ) {
            let client_list: Vec<Vec<_>> = (0..args.n_worker)
                .map(|i| {
                    (i..args.n_client)
                        .step_by(args.n_worker as usize)
                        .map(|_| client())
                        .collect()
                })
                .collect();
            let mut worker_data = Self {
                client_list,
                count: Arc::new(AtomicU32::new(0)),
                latency,
                status,
                args: Arc::new(args),
            };
            unsafe {
                rte_eal_mp_remote_launch(
                    Self::worker,
                    &mut worker_data as *mut _ as *mut _,
                    rte_rmt_call_main_t::SKIP_MAIN,
                );
            }
        }
    }

    let status = Arc::new(AtomicU32::new(Status::WarmUp as _));
    let mut latency = Latency::new("latency");
    match args.mode {
        Mode::Unreplicated => WorkerData::launch(
            || unreplicated::Client::<_, AsyncExecutor>::register_new(&mut transport),
            args,
            status.clone(),
            latency.local(),
        ),
        Mode::PBFT => WorkerData::launch(
            || pbft::Client::<_, AsyncExecutor>::register_new(&mut transport),
            args,
            status.clone(),
            latency.local(),
        ),
    }

    transport.run(0, || status.load(Ordering::SeqCst) == Status::Shutdown as _);
    unsafe { rte_eal_mp_wait_lcore() };

    latency.refresh();
    println!("{}", latency);
    let hist: SyncHistogram<_> = latency.into();
    println!(
        "p50(mean) {:?}({:?}) p99 {:?}",
        Duration::from_nanos(hist.value_at_quantile(0.5)),
        Duration::from_nanos(hist.mean() as _),
        Duration::from_nanos(hist.value_at_quantile(0.99))
    );
}
