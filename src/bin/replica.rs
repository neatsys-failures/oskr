use std::{ffi::c_void, os::raw::c_uint, sync::Arc};

use oskr::{
    common::Opaque,
    dpdk_shim::{oskr_lcore_id, rte_eal_mp_remote_launch, rte_rmt_call_main_t},
    executor::Executor,
    replication::unreplicated,
    transport::{dpdk::Transport, Config},
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
    };

    let mut transport = Transport::setup(config, port_id, 1, n_worker);
    // TODO select replica type
    let replica = Executor::<unreplicated::Replica<Transport, _>>::register_new(
        &mut transport,
        replica_id,
        NullApp,
    );

    struct WorkerData<Replica> {
        replica: Arc<Executor<Replica>>,
        n_worker: u16,
    }
    let worker_data = WorkerData { replica, n_worker };
    extern "C" fn worker<Replica>(arg: *mut c_void) -> i32 {
        let worker_data: &WorkerData<Replica> = unsafe { &*(arg as *mut _) };
        let replica = worker_data.replica.clone();
        let n_worker = worker_data.n_worker;

        if unsafe { oskr_lcore_id() } > n_worker as c_uint {
            return 0;
        }

        replica.worker_loop();
        unreachable!();
    }

    unsafe {
        rte_eal_mp_remote_launch(
            worker::<unreplicated::Replica<Transport, NullApp>>,
            &worker_data as *const _ as *mut _,
            rte_rmt_call_main_t::SKIP_MAIN,
        );
    }

    transport.run1();
}
