// test tx from multiple threads
use std::{
    env,
    ffi::c_void,
    fmt::Display,
    os::raw::c_int,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Arc,
    },
    thread::{sleep, spawn},
    time::Duration,
};

use oskr::{
    dpdk::Transport,
    dpdk_shim::{oskr_lcore_id, rte_eal_mp_remote_launch},
    transport::{self, Config, Transport as _, TxAgent as _},
};

fn main() {
    let server_address = "b8:ce:f6:2a:2f:94#0".parse().unwrap();
    let port_id = 0;
    let n_tx = 7; // for some reason 7 is maximum number of tx queue that works

    let config = Config {
        replica_address: vec![server_address],
        n_fault: 0,
        multicast_address: None,
    };
    let mut transport = Transport::setup(config, port_id, 1, n_tx);

    let mut args = env::args();
    let _prog = args.next();
    let invoke = args.next().map(|arg| arg == "invoke").unwrap_or(false);
    let mut n_concurrent = 1;
    if invoke {
        if let Some(arg) = args.next() {
            n_concurrent = arg.parse().unwrap();
        }
    }
    assert!(n_concurrent <= n_tx);

    let count = Arc::new(AtomicU32::new(0));

    if invoke {
        struct Receiver<T: transport::Transport> {
            address: T::Address,
            transport: T::TxAgent,
            chan_rx: mpsc::Receiver<(T::Address, T::RxBuffer)>,
            count: Arc<AtomicU32>,
        }
        impl<T: transport::Transport> transport::Receiver<T> for Receiver<T> {
            fn get_address(&self) -> &T::Address {
                &self.address
            }
        }
        let receiver_list: Vec<_> = (0..n_concurrent)
            .map(|_| {
                let (chan_tx, chan_rx) = mpsc::channel();
                let receiver = Receiver {
                    address: transport.ephemeral_address(),
                    transport: transport.tx_agent(),
                    chan_rx,
                    count: count.clone(),
                };
                transport.register(&receiver, move |remote, buffer| {
                    chan_tx.send((remote, buffer)).unwrap();
                });
                transport
                    .tx_agent()
                    .send_message(&receiver, &server_address, |_buffer| 0);
                receiver
            })
            .collect();
        impl<T: transport::Transport> Receiver<T> {
            fn run(&self)
            where
                T::Address: Display,
            {
                println!("{}", self.address);
                while let Ok((remote, _buffer)) = self.chan_rx.recv() {
                    self.transport.send_message(self, &remote, |_buffer| 0);
                    self.count.fetch_add(1, Ordering::SeqCst);
                }
                unreachable!();
            }
        }
        extern "C" fn worker(arg: *mut c_void) -> c_int {
            let receiver_list = arg as *mut Vec<Receiver<Transport>>;
            let lcore_id = unsafe { oskr_lcore_id() };
            if let Some(receiver) = unsafe { &*receiver_list }.get(lcore_id as usize - 1) {
                println!("start on lcore {}", lcore_id);
                receiver.run();
            } else {
                println!("idle lcore {} exiting", lcore_id);
                return 0;
            }
            unreachable!()
        }
        unsafe {
            rte_eal_mp_remote_launch(
                worker,
                // TODO prevent memory leak
                Box::leak(Box::new(receiver_list)) as *mut _ as *mut _,
                oskr::dpdk_shim::rte_rmt_call_main_t::SKIP_MAIN,
            );
        }
    } else {
        #[derive(Debug, Clone, Copy)]
        struct Receiver(<Transport as transport::Transport>::Address);
        impl transport::Receiver<Transport> for Receiver {
            fn get_address(&self) -> &<Transport as transport::Transport>::Address {
                &self.0
            }
        }
        let receiver = Receiver(server_address);
        transport.register(&receiver, {
            let transport = transport.tx_agent();
            let count = count.clone();
            move |remote, _buffer| {
                transport.send_message(&receiver, &remote, |_buffer| 0);
                count.fetch_add(1, Ordering::SeqCst);
            }
        });
    };

    spawn(move || loop {
        sleep(Duration::from_secs(1));
        println!("{}", count.swap(0, Ordering::SeqCst));
    });

    if invoke {
        transport.run(0);
    } else {
        transport.run1();
    }
}
