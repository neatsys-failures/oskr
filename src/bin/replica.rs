use std::{
    ffi::c_void,
    fs::File,
    io::Read,
    path::PathBuf,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use clap::{ArgEnum, Parser};
use oskr::{
    common::{panic_abort, Config, OpNumber, Opaque, ReplicaId},
    dpdk_shim::{rte_eal_mp_remote_launch, rte_eal_mp_wait_lcore, rte_rmt_call_main_t},
    facade::{self, App},
    framework::{dpdk::Transport, sqlite::Database, ycsb_workload::Property},
    protocol::{hotstuff, pbft, unreplicated, zyzzyva},
    stage::{Handle, State},
};
use tracing::{info, warn};

struct NullApp;
impl App for NullApp {
    fn execute(&mut self, _op_number: OpNumber, _op: Opaque) -> Opaque {
        Opaque::default()
    }
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
    }

    // should be `AppName` which is more pricese
    // just try to match client side
    #[derive(Debug, Clone, Copy, PartialEq, Eq, ArgEnum)]
    enum WorkloadName {
        Null,
        YCSB,
    }

    #[derive(Parser, Debug)]
    #[clap(name = "Oskr Replica", version)]
    struct Args {
        #[clap(short, long, arg_enum)]
        mode: Mode,
        #[clap(short, long, arg_enum, default_value_t = WorkloadName::Null)]
        workload: WorkloadName,
        #[clap(short, long, parse(from_os_str))]
        config: PathBuf,
        #[clap(short = 'i')]
        replica_id: ReplicaId,
        #[clap(long, default_value = "000000ff")]
        mask: String,
        #[clap(long = "port", default_value_t = 0)]
        port_id: u16,
        #[clap(short, long = "worker-number", default_value_t = 1)]
        n_worker: usize,
        #[clap(long = "tx", default_value_t = 1)]
        n_tx: u16,
        #[clap(short, long, default_value_t = 1)]
        batch_size: usize,
        #[clap(long)]
        adaptive: bool,
        // property list if necessary
        #[clap(long = "db")]
        database_file: Option<PathBuf>,
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

    let shutdown = Arc::new(AtomicBool::new(false));
    ctrlc::set_handler({
        let shutdown = shutdown.clone();
        move || {
            println!();
            if !shutdown.load(Ordering::SeqCst) {
                shutdown.store(true, Ordering::SeqCst);
            } else {
                warn!("double ctrl-c, quit ungracefully");
                process::abort();
            }
        }
    })
    .unwrap();

    let mut transport = Transport::setup(core_mask, args.port_id, 1, args.n_tx);

    struct WorkerData<Replica: State> {
        replica: Arc<Handle<Replica>>,
        args: Arc<Args>,
        shutdown: Arc<AtomicBool>,
    }
    impl<R: State> WorkerData<R> {
        extern "C" fn worker(arg: *mut c_void) -> i32 {
            let worker_data: &Self = unsafe { &*(arg as *mut _) };
            let replica = worker_data.replica.clone();
            let args = worker_data.args.clone();
            let shutdown = worker_data.shutdown.clone();

            if Transport::worker_id() < args.n_worker {
                replica.run_worker(|| shutdown.load(Ordering::SeqCst));
            }
            0
        }

        fn launch(replica: Handle<R>, args: Args, shutdown: Arc<AtomicBool>) -> Box<dyn FnOnce()>
        where
            R: 'static,
        {
            let replica = Arc::new(replica);
            let data = Self {
                replica: replica.clone(),
                args: Arc::new(args),
                shutdown,
            };
            unsafe {
                rte_eal_mp_remote_launch(
                    Self::worker,
                    &data as *const _ as *mut _,
                    rte_rmt_call_main_t::SKIP_MAIN,
                );
            }
            Box::new(move || replica.unpark_all())
        }
    }

    fn launch_app(
        app: impl App + Send + 'static,
        args: Args,
        config: Config<Transport>,
        transport: &mut Transport,
        shutdown: Arc<AtomicBool>,
    ) -> impl FnOnce() {
        match args.mode {
            Mode::Unreplicated => WorkerData::launch(
                unreplicated::Replica::register_new(config, transport, args.replica_id, app, false),
                args,
                shutdown.clone(),
            ),
            Mode::UnreplicatedSigned => WorkerData::launch(
                unreplicated::Replica::register_new(config, transport, args.replica_id, app, true),
                args,
                shutdown.clone(),
            ),
            Mode::PBFT => WorkerData::launch(
                pbft::Replica::register_new(
                    config,
                    transport,
                    args.replica_id,
                    app,
                    args.batch_size,
                    args.adaptive,
                ),
                args,
                shutdown.clone(),
            ),
            Mode::HotStuff => WorkerData::launch(
                hotstuff::Replica::register_new(
                    config,
                    transport,
                    args.replica_id,
                    NullApp,
                    args.batch_size,
                    args.adaptive,
                ),
                args,
                shutdown.clone(),
            ),
            Mode::Zyzzyva => WorkerData::launch(
                zyzzyva::Replica::register_new(
                    config,
                    transport,
                    args.replica_id,
                    app,
                    args.batch_size,
                ),
                args,
                shutdown.clone(),
            ),
        }
    }

    let unpark: Box<dyn FnOnce()> = match args.workload {
        WorkloadName::Null => Box::new(launch_app(
            NullApp,
            args,
            config,
            &mut transport,
            shutdown.clone(),
        )),
        WorkloadName::YCSB => {
            let property = Property::default(); // TODO
            let create_table = args
                .database_file
                .as_ref()
                .map(|db| !db.exists())
                .unwrap_or(true);
            let app = if let Some(db) = args.database_file.as_ref() {
                Database::open(db)
            } else {
                Database::default()
            };
            if create_table {
                app.create_table(
                    &property.table,
                    // what the hell
                    &*(0..property.field_count)
                        .map(|i| format!("field{}", i))
                        .collect::<Vec<_>>()
                        .iter()
                        .map(|s| &**s)
                        .collect::<Vec<_>>(),
                );
            }
            Box::new(launch_app(
                app,
                args,
                config,
                &mut transport,
                shutdown.clone(),
            ))
        }
    };
    transport.run1(|| shutdown.load(Ordering::SeqCst));
    unpark();
    unsafe { rte_eal_mp_wait_lcore() };
    info!("gracefully shutdown");
}
