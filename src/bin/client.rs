use std::{
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
    task::Context,
    time::Duration,
};

use clap::{ArgEnum, Parser};
use futures::{channel::oneshot, select, task::noop_waker_ref, FutureExt};
use hdrhistogram::SyncHistogram;
use oskr::{
    app::ycsb,
    common::{panic_abort, Config, Opaque},
    dpdk_shim::{rte_eal_mp_remote_launch, rte_eal_mp_wait_lcore, rte_rmt_call_main_t},
    facade::{self, AsyncEcosystem as _, Invoke, Receiver},
    framework::{
        busy_poll::AsyncEcosystem,
        dpdk::Transport,
        latency::{Latency, LocalLatency, MeasureClock},
        ycsb_workload::{OpKind, Property, Workload},
    },
    protocol::{hotstuff, pbft, unreplicated, zyzzyva},
};
use tracing::{debug, info};

fn main() {
    tracing_subscriber::fmt::init();
    panic_abort();

    #[derive(Debug, Clone, Copy, PartialEq, Eq, ArgEnum)]
    #[allow(clippy::upper_case_acronyms)]
    enum Mode {
        Unreplicated,
        UnreplicatedSigned,
        PBFT,
        HotStuff,
        Zyzzyva,
        YCSB,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, ArgEnum)]
    #[allow(clippy::upper_case_acronyms)]
    enum WorkloadName {
        Null,
        YCSB,
    }
    impl WorkloadName {
        // seems like no better way but it still feels like go back to 90's
        const ALL: usize = 0;
        const NULL_TAG: &'static [&'static str] = &["all"];
        const YCSB_READ: usize = 1;
        const YCSB_UPDATE: usize = 2;
        const YCSB_SCAN: usize = 3;
        const YCSB_INSERT: usize = 4;
        const YCSB_READ_MODIFY_WRITE: usize = 5;
        const YCSB_TAG: &'static [&'static str] = &[
            "all",
            "ycsb.read",
            "ycsb.update",
            "ycsb.scan",
            "ycsb.insert",
            "ycsb.read_modify_write",
        ];
    }

    #[derive(Parser, Debug)]
    #[clap(name = "Oskr Client", version)]
    struct Args {
        #[clap(short, long, arg_enum)]
        mode: Mode,
        #[clap(short, long, arg_enum)]
        workload: WorkloadName,
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
        #[clap(short = 'P')]
        property_file: PathBuf,
        #[clap(short = 'p')]
        property_list: Vec<String>,
    }
    let args = Args::parse();
    let core_mask = u128::from_str_radix(&args.mask, 16).unwrap();
    info!("initialize with {} cores", core_mask.count_ones());
    assert!(core_mask.count_ones() > args.n_worker as u32); // strictly greater-than to preserve one rx core

    let prefix = args.config.file_name().unwrap().to_str().unwrap();
    let config = args.config.with_file_name(format!("{}.config", prefix));
    let mut config: facade::Config<_> = {
        let mut buf = String::new();
        File::open(config)
            .unwrap()
            .read_to_string(&mut buf)
            .unwrap();
        buf.parse().unwrap()
    };
    config.collect_signing_key(&args.config);
    let config = Config::for_shard(config, 0); // TODO

    let property = Property::default();

    let mut transport = Transport::setup(core_mask, args.port_id, 1, args.n_tx);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Status {
        WarmUp,
        Run,
        Shutdown,
    }

    struct WorkerData<Client> {
        client_list: Vec<Vec<Client>>,
        count: Arc<AtomicU32>,
        latency: Vec<LocalLatency>,
        status: Arc<AtomicU32>,
        args: Arc<Args>,
        ycsb_workload: Arc<Workload>,
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
            let ycsb_workload = worker_data.ycsb_workload.clone();
            let args = worker_data.args.clone();

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
                    let clock = MeasureClock::default();
                    let workload = args.workload;
                    let ycsb_workload = ycsb_workload.clone();
                    (
                        AsyncEcosystem::spawn(async move {
                            debug!("{}", client.get_address());
                            loop {
                                let measure = clock.measure();
                                match workload {
                                    WorkloadName::Null => {
                                        select! {
                                            _ = client.invoke(Opaque::default()).fuse() => {
                                                if status.load(Ordering::SeqCst) == Status::Run as _ {
                                                    latency[WorkloadName::ALL] += measure;
                                                    count.fetch_add(1, Ordering::SeqCst);
                                                }
                                            },
                                            _ = shutdown => return,
                                        }
                                    }
                                    WorkloadName::YCSB => {
                                        let (op, post_action) = ycsb_workload.one_op();
                                        let invoke = async {
                                            match op {
                                                OpKind::Read(op) => {
                                                    client.invoke(op).await;
                                                    WorkloadName::YCSB_READ
                                                }
                                                OpKind::Update(op) => {
                                                    client.invoke(op).await;
                                                    WorkloadName::YCSB_UPDATE
                                                }
                                                OpKind::ReadModifyWrite(read_op, update_op) => {
                                                    client.invoke(read_op).await;
                                                    client.invoke(update_op).await;
                                                    WorkloadName::YCSB_READ_MODIFY_WRITE
                                                }
                                                OpKind::Scan(op) => {
                                                    client.invoke(op).await;
                                                    WorkloadName::YCSB_SCAN
                                                }
                                                OpKind::Insert(op) => {
                                                    client.invoke(op).await;
                                                    WorkloadName::YCSB_INSERT
                                                }
                                            }
                                        };
                                        select! {
                                            row = invoke.fuse() => {
                                                latency[row] += measure;
                                                latency[WorkloadName::ALL] += measure;
                                                post_action(&*ycsb_workload);
                                            },
                                            _ = shutdown => return,
                                        }
                                    }
                                }
                            }
                        }),
                        shutdown_tx,
                    )
                })
                .unzip();

            if worker_id == 0 {
                AsyncEcosystem::poll(Duration::from_secs(args.warm_up_duration));
                status.store(Status::Run as _, Ordering::SeqCst);
                info!("warm up finish");

                for _ in 0..args.duration {
                    AsyncEcosystem::poll(Duration::from_secs(1));
                    let count = count.swap(0, Ordering::SeqCst);
                    println!("{}", count);
                }
                status.store(Status::Shutdown as _, Ordering::SeqCst);
            } else {
                AsyncEcosystem::poll(Duration::from_secs(args.warm_up_duration + args.duration));
            }

            // with new latency there is no need to join tasks
            // save it here for debug and future use
            for shutdown in shutdown_list {
                shutdown.send(()).unwrap();
            }
            AsyncEcosystem::poll_all();
            client_list.into_iter().for_each(|mut client| {
                assert!(Pin::new(&mut client)
                    .poll(&mut Context::from_waker(noop_waker_ref()))
                    .is_ready())
            });
            0
        }

        fn launch(
            mut client: impl FnMut() -> C,
            args: Args,
            status: Arc<AtomicU32>,
            latency: Vec<LocalLatency>,
            property: Property,
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
                ycsb_workload: Arc::new(Workload::new(property)),
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
    let latency: Vec<_> = match args.workload {
        WorkloadName::Null => WorkloadName::NULL_TAG,
        WorkloadName::YCSB => WorkloadName::YCSB_TAG,
    }
    .iter()
    .map(|latency| Latency::new(latency))
    .collect();
    match args.mode {
        Mode::Unreplicated => WorkerData::launch(
            || {
                unreplicated::Client::<_, AsyncEcosystem>::register_new(
                    config.clone(),
                    &mut transport,
                    false,
                )
            },
            args,
            status.clone(),
            latency.iter().map(|latency| latency.local()).collect(),
            property,
        ),
        Mode::UnreplicatedSigned => WorkerData::launch(
            || {
                unreplicated::Client::<_, AsyncEcosystem>::register_new(
                    config.clone(),
                    &mut transport,
                    true,
                )
            },
            args,
            status.clone(),
            latency.iter().map(|latency| latency.local()).collect(),
            property,
        ),
        Mode::PBFT => WorkerData::launch(
            || pbft::Client::<_, AsyncEcosystem>::register_new(config.clone(), &mut transport),
            args,
            status.clone(),
            latency.iter().map(|latency| latency.local()).collect(),
            property,
        ),
        Mode::HotStuff => WorkerData::launch(
            || hotstuff::Client::<_, AsyncEcosystem>::register_new(config.clone(), &mut transport),
            args,
            status.clone(),
            latency.iter().map(|latency| latency.local()).collect(),
            property,
        ),
        Mode::Zyzzyva => WorkerData::launch(
            || zyzzyva::Client::<_, AsyncEcosystem>::register_new(config.clone(), &mut transport),
            args,
            status.clone(),
            latency.iter().map(|latency| latency.local()).collect(),
            property,
        ),
        Mode::YCSB => WorkerData::launch(
            || ycsb::TraceClient::default(),
            args,
            status.clone(),
            latency.iter().map(|latency| latency.local()).collect(),
            property,
        ),
    }

    transport.run(0, || status.load(Ordering::SeqCst) == Status::Shutdown as _);
    unsafe { rte_eal_mp_wait_lcore() };

    for mut latency in latency {
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
}
