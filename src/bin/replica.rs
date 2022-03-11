use std::{ffi::c_void, fs::File, io::Read, path::PathBuf, sync::Arc};

use clap::{ArgEnum, Parser};
use oskr::{
    common::{Opaque, ReplicaId},
    dpdk::Transport,
    dpdk_shim::{rte_eal_mp_remote_launch, rte_rmt_call_main_t},
    replication::{pbft, unreplicated},
    stage::{Handle, State},
    transport::Config,
    App,
};
use tracing::info;

struct NullApp;
impl App for NullApp {
    fn execute(&mut self, _op: Opaque) -> Opaque {
        Opaque::default()
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ArgEnum)]
    enum Mode {
        Unreplicated,
        PBFT,
    }

    #[derive(Parser, Debug)]
    #[clap(name = "Oskr Replica", version)]
    struct Args {
        #[clap(short, long, arg_enum)]
        mode: Mode,
        #[clap(short, long, parse(from_os_str))]
        config: PathBuf,
        #[clap(short = 'i')]
        replica_id: ReplicaId,
        #[clap(long, default_value = "000000ff")]
        mask: String,
        #[clap(short, long, default_value_t = 0)]
        port_id: u16,
        #[clap(short, long = "worker-number", name = "N", default_value_t = 1)]
        n_worker: u16,
        #[clap(short, long, default_value_t = 1)]
        batch_size: usize,
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

    let mut transport = Transport::setup(config, core_mask, args.port_id, 1, args.n_worker);

    struct WorkerData<Replica: State> {
        replica: Arc<Handle<Replica>>,
        n_worker: u16,
    }
    impl<R: State> WorkerData<R> {
        extern "C" fn worker(arg: *mut c_void) -> i32 {
            let worker_data: &Self = unsafe { &*(arg as *mut _) };
            let replica = worker_data.replica.clone();
            let n_worker = worker_data.n_worker;

            let worker_id = Transport::worker_id();
            if worker_id >= n_worker as usize {
                return 0;
            } else if worker_id == 0 {
                // it runs slower with this optimization...
                // replica.run_stateful_worker();
                replica.run_worker();
            } else {
                // run stateless worker
            }
            unreachable!();
        }

        fn launch(replica: Handle<R>, args: Args) {
            let data = Self {
                replica: Arc::new(replica),
                n_worker: args.n_worker,
            };
            unsafe {
                rte_eal_mp_remote_launch(
                    Self::worker,
                    &data as *const _ as *mut _,
                    rte_rmt_call_main_t::SKIP_MAIN,
                );
            }
        }
    }

    match args.mode {
        Mode::Unreplicated => WorkerData::launch(
            unreplicated::Replica::register_new(&mut transport, args.replica_id, NullApp),
            args,
        ),
        Mode::PBFT => WorkerData::launch(
            pbft::Replica::register_new(&mut transport, args.replica_id, NullApp, args.batch_size),
            args,
        ),
    }

    transport.run1();
}
