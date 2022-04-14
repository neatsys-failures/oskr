use std::{
    ffi::c_void,
    fs,
    mem::take,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use clap::{ArgEnum, Parser};
use futures::{channel::oneshot, select, FutureExt};
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
use quanta::Clock;
use tracing::{debug, info};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    WarmUp,
    Run,
    Shutdown,
}

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
    #[derive(Parser, Debug)]
    #[clap(name = "Oskr Client", version)]
    struct Args {
        #[clap(short, long, arg_enum)]
        mode: Mode,
        #[clap(short, long, arg_enum, default_value_t = WorkloadName::Null)]
        workload: WorkloadName,
        #[clap(short, long, parse(from_os_str))]
        config: PathBuf,
        #[clap(long, default_value = "000000ff")]
        mask: String,
        #[clap(long = "port", default_value_t = 0)]
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
        property_file: Option<PathBuf>,
        #[clap(short = 'p')]
        property_list: Vec<String>,
        #[clap(long)]
        load: bool,
    }
    let args = Args::parse();
    let core_mask = u128::from_str_radix(&args.mask, 16).unwrap();
    info!("initialize with {} cores", core_mask.count_ones());
    assert!(core_mask.count_ones() > args.n_worker as u32); // strictly greater-than to preserve one rx core

    let prefix = args.config.file_name().unwrap().to_str().unwrap();
    let config = args.config.with_file_name(format!("{}.config", prefix));
    let mut config: facade::Config<_> = std::str::from_utf8(&*fs::read(config).unwrap())
        .unwrap()
        .parse()
        .unwrap();
    config.collect_signing_key(&args.config);
    let config = Config::for_shard(config, 0); // TODO

    let mut property = Property::default();
    if let Some(property_file) = &args.property_file {
        let rewrite_table = java_properties::read(&*fs::read(property_file).unwrap()).unwrap();
        for (key, value) in rewrite_table {
            property.rewrite(&key, &value);
        }
    }
    for property_item in &args.property_list {
        let (key, value) = property_item.split_once('=').unwrap();
        property.rewrite(key, value);
    }
    if args.load {
        property.rewrite_load();
    }

    let mut transport = Transport::setup(core_mask, args.port_id, 1, args.n_tx);
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
                .map(|client| {
                    spawn_client(
                        client,
                        count.clone(),
                        status.clone(),
                        latency.clone(),
                        args.workload,
                        ycsb_workload.clone(),
                    )
                })
                .unzip();

            // track client exit or not directly?
            let limit = match args.workload {
                WorkloadName::Null => u32::MAX,
                WorkloadName::YCSB => {
                    if ycsb_workload.property.do_transaction {
                        ycsb_workload.property.operation_count as _
                    } else {
                        ycsb_workload.property.record_count as _
                    }
                }
            };
            let clock = Clock::new();
            let start = clock.start();
            if worker_id == 0 {
                AsyncEcosystem::poll_until(|| {
                    clock.delta(start, clock.end()) >= Duration::from_secs(args.warm_up_duration)
                });
                status.store(Status::Run as _, Ordering::SeqCst);
                info!("warm up finish");

                let mut prev = 0;
                for _ in 0..args.duration {
                    let start = clock.start();
                    AsyncEcosystem::poll_until(|| {
                        count.load(Ordering::SeqCst) > limit
                            || clock.delta(start, clock.end()) >= Duration::from_secs(1)
                    });
                    let count = count.load(Ordering::SeqCst);
                    println!("{}", count - prev);
                    prev = count;
                    if count > limit {
                        break;
                    }
                }
                status.store(Status::Shutdown as _, Ordering::SeqCst);
            } else {
                AsyncEcosystem::poll_until(|| {
                    count.load(Ordering::SeqCst) > limit
                        || clock.delta(start, clock.end())
                            >= Duration::from_secs(args.warm_up_duration + args.duration)
                });
            }

            // with new latency there is no need to join tasks
            // save it here for debug and future use
            for shutdown in shutdown_list {
                let _ = shutdown.send(());
            }
            AsyncEcosystem::poll_all();
            // client_list.into_iter().for_each(|mut client| {
            //     assert!(Pin::new(&mut client)
            //         .poll(&mut Context::from_waker(noop_waker_ref()))
            //         .is_ready())
            // });
            drop(client_list);
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

fn spawn_client<C: Receiver<Transport> + Invoke + Send + 'static>(
    mut client: C,
    count: Arc<AtomicU32>,
    status: Arc<AtomicU32>,
    mut latency: Vec<LocalLatency>,
    workload: WorkloadName,
    ycsb_workload: Arc<Workload>,
) -> (
    <AsyncEcosystem as facade::AsyncEcosystem<()>>::JoinHandle,
    oneshot::Sender<()>,
) {
    let (shutdown_tx, mut shutdown) = oneshot::channel();
    let handle = AsyncEcosystem::spawn(async move {
        debug!("{}", client.get_address());
        let clock = MeasureClock::default();

        let limit = match workload {
            WorkloadName::Null => u32::MAX,
            WorkloadName::YCSB => {
                if ycsb_workload.property.do_transaction {
                    ycsb_workload.property.operation_count as _
                } else {
                    ycsb_workload.property.record_count as _
                }
            }
        };
        while status.load(Ordering::SeqCst) == Status::WarmUp as _
            || count.fetch_add(1, Ordering::SeqCst) < limit
        {
            match workload {
                WorkloadName::Null => {
                    let measure = clock.measure();
                    select! {
                        _ = client.invoke(Opaque::default()).fuse() => {
                            if status.load(Ordering::SeqCst) == Status::Run as _ {
                                latency[WorkloadName::ALL] += measure;
                            }
                        },
                        _ = shutdown => return,
                    }
                }
                WorkloadName::YCSB => {
                    let (op, post_action) = ycsb_workload.one_op();
                    let measure = clock.measure();
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
                            if status.load(Ordering::SeqCst) == Status::Run as _ {
                                latency[row] += measure;
                                latency[WorkloadName::ALL] += measure;
                            }
                            post_action(&*ycsb_workload);
                        },
                        _ = shutdown => return,
                    }
                }
            }
        }
    });
    (handle, shutdown_tx)
}
