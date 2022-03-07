use std::{ffi::c_void, sync::Arc};

use oskr::{
    common::Opaque,
    director::Handle,
    dpdk::Transport,
    dpdk_shim::{rte_eal_mp_remote_launch, rte_rmt_call_main_t},
    replication::unreplicated,
    transport::Config,
    App,
};

struct NullApp;
impl App for NullApp {
    fn execute(&mut self, _op: Opaque) -> Opaque {
        Opaque::default()
    }
}

fn main() {
    let port_id = 0;
    let n_worker = 1;
    let replica_id = 0;
    let config = Config {
        replica_address: vec!["b8:ce:f6:2a:2f:94#0".parse().unwrap()],
        n_fault: 0,
        multicast_address: None,
        signing_key: Default::default(),
    };

    let mut transport = Transport::setup(config, port_id, 1, n_worker as u16);
    // TODO select replica type
    let replica = Arc::new(unreplicated::Replica::register_new(
        &mut transport,
        replica_id,
        NullApp,
    ));

    struct WorkerData<Replica> {
        replica: Arc<Handle<Replica, Transport>>,
        n_worker: usize,
    }
    let worker_data = WorkerData { replica, n_worker };
    extern "C" fn worker<Replica>(arg: *mut c_void) -> i32 {
        let worker_data: &WorkerData<Replica> = unsafe { &*(arg as *mut _) };
        let replica = worker_data.replica.clone();
        let n_worker = worker_data.n_worker;

        let worker_id = Transport::worker_id();
        if worker_id >= n_worker {
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

    unsafe {
        rte_eal_mp_remote_launch(
            worker::<unreplicated::Replica>,
            &worker_data as *const _ as *mut _,
            rte_rmt_call_main_t::SKIP_MAIN,
        );
    }

    transport.run1();
}
