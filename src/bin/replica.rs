use std::{
    ffi::c_void,
    fs,
    path::PathBuf,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use clap::{ArgEnum, Parser};
use oskr::{
    app::ycsb::Database as _,
    common::{panic_abort, Config, OpNumber, Opaque, ReplicaId, SigningKey},
    dpdk_shim::{
        rte_eal_mp_remote_launch, rte_eal_mp_wait_lcore, rte_eth_dev_socket_id,
        rte_rmt_call_main_t, rte_socket_id,
    },
    facade::{self, App},
    framework::{
        dpdk::Transport,
        memory_database,
        sqlite::Database,
        ycsb_workload::{Property, Workload},
    },
    protocol::{
        hotstuff,
        neo::{self, message::MulticastVerifyingKey},
        pbft, unreplicated, zyzzyva,
    },
    stage::{Handle, State},
};
use tracing::{debug, info, warn};

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
        Neo,
    }

    // should be `AppName` which is more pricese
    // just try to match client side
    #[derive(Debug, Clone, Copy, PartialEq, Eq, ArgEnum)]
    enum AppName {
        Null,
        YCSB,
        YCSBMemory,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, ArgEnum)]
    enum MulticastKey {
        HMAC,
        PKEY,
    }

    #[derive(Parser, Debug)]
    #[clap(name = "Oskr Replica", version)]
    struct Args {
        #[clap(short, long, arg_enum)]
        mode: Mode,
        #[clap(short, long, arg_enum, default_value_t = AppName::Null)]
        app: AppName,
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
        #[clap(short)]
        property_list: Vec<String>,
        #[clap(long = "db")]
        database_file: Option<PathBuf>,
        #[clap(long = "check-eq")]
        check_equivocation: bool,
        #[clap(long = "multi-key", arg_enum, default_value_t = MulticastKey::HMAC)]
        multicast_key: MulticastKey,
        #[clap(long = "k256")]
        use_k256: bool,
        #[clap(long = "drop", default_value_t = 0.)]
        drop_rate: f32,
    }
    let args = Args::parse();
    let core_mask = u128::from_str_radix(&args.mask, 16).unwrap();
    info!("initialize with {} cores", core_mask.count_ones());
    assert!(core_mask.count_ones() > args.n_worker as u32); // strictly greater-than to preserve one rx core

    let prefix = args.config.file_name().unwrap().to_str().unwrap();
    let config = args.config.with_file_name(format!("{}.config", prefix));
    let mut config: facade::Config<_> = fs::read_to_string(config).unwrap().parse().unwrap();
    config.collect_signing_key(&args.config, !args.use_k256);
    let config = Config::for_shard(config, 0); // TODO

    let mut property = Property::default();
    // TODO property file
    for property_item in &args.property_list {
        let (key, value) = property_item.split_once('=').unwrap();
        property.rewrite(key, value);
    }

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
    if let Some(address) = config.multicast {
        transport.set_multicast_address(address);
    }
    transport.set_drop_rate(args.drop_rate);

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
                if unsafe { rte_socket_id() == rte_eth_dev_socket_id(args.port_id) } {
                    replica.run_worker(|| shutdown.load(Ordering::SeqCst));
                } else {
                    debug!(
                        "start stateless worker {} on remote socket",
                        Transport::worker_id()
                    );
                    replica.run_stateless_worker(|| shutdown.load(Ordering::SeqCst));
                }
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
            Mode::Neo => {
                let multicast_key = match args.multicast_key {
                    MulticastKey::HMAC => MulticastVerifyingKey::HMac([0; 4]),
                    MulticastKey::PKEY => MulticastVerifyingKey::PublicKey(
                        {
                            let mut key = SigningKey::K256(
                                k256::ecdsa::SigningKey::from_bytes(&[0xaa; 32]).unwrap(),
                            );
                            if !args.use_k256 {
                                key = key.use_secp256k1();
                            }
                            key
                        }
                        .verifying_key(),
                    ),
                };
                WorkerData::launch(
                    neo::Replica::register_new(
                        config,
                        transport,
                        args.replica_id,
                        app,
                        args.batch_size,
                        multicast_key,
                        args.check_equivocation,
                    ),
                    args,
                    shutdown.clone(),
                )
            }
        }
    }

    let unpark: Box<dyn FnOnce()> = match args.app {
        AppName::Null => Box::new(launch_app(
            NullApp,
            args,
            config,
            &mut transport,
            shutdown.clone(),
        )),
        AppName::YCSB => {
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
        AppName::YCSBMemory => {
            let mut app = memory_database::Database::default();
            // TODO more real setup
            property.do_transaction = false;
            let table = property.table.clone();
            let op_count = property.record_count;
            let workload = Workload::new(property);
            for _ in 0..op_count {
                let (key, mut value_table) = workload.next_insert_entry();
                // overwrite values to make replication has matching database
                for value in value_table.values_mut() {
                    for (i, b) in value.iter_mut().enumerate() {
                        *b = i.to_le_bytes()[0];
                    }
                }
                app.insert(table.clone(), key, value_table).unwrap();
            }
            info!("finish building database");

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
