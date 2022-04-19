use std::{
    env,
    ffi::c_void,
    fs,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use oskr::{
    common::Config,
    dpdk_shim::{rte_eal_mp_remote_launch, rte_rmt_call_main_t},
    facade::{self, Receiver, Transport, TxAgent},
    framework::dpdk,
    protocol::neo::message::{MulticastVerifyingKey, OrderedMulticast, Status},
    stage::{Handle, State},
};
use quanta::Clock;
use tracing::debug;

struct MulticastReceiver<T: Transport>(Arc<(AtomicU32, AtomicU32)>, T::Address);
impl<T: Transport> State for MulticastReceiver<T> {
    type Shared = Arc<(AtomicU32, AtomicU32)>;
    fn shared(&self) -> Self::Shared {
        self.0.clone()
    }
}
impl<T: Transport> Receiver<T> for MulticastReceiver<T> {
    fn get_address(&self) -> &T::Address {
        &self.1
    }
}
impl<T: Transport> MulticastReceiver<T> {
    fn receive_buffer(shared: &Arc<(AtomicU32, AtomicU32)>, buffer: T::RxBuffer) {
        let message = OrderedMulticast::<u32>::parse(buffer.as_ref());
        let verified = message.verify(&MulticastVerifyingKey::default()).unwrap();
        shared.0.fetch_add(1, Ordering::SeqCst);
        if verified.status == Status::Signed {
            shared.1.fetch_add(1, Ordering::SeqCst);
        }
        debug!("received: {}", &*verified);
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let config = env::args().nth(1).unwrap();
    let config: facade::Config<dpdk::Transport> =
        fs::read_to_string(config).unwrap().parse().unwrap();
    let config = Config::for_shard(config, 0);

    let role = env::args().nth(2).unwrap();
    let is_client = role == "client";
    let replica_id = if is_client { 0 } else { role.parse().unwrap() };

    let mut transport = dpdk::Transport::setup(0x3, 0, 1, 1);
    transport.set_multicast_address(config.multicast.unwrap());
    let address = if is_client {
        transport.ephemeral_address()
    } else {
        config.replica(replica_id).clone()
    };
    let counter: Arc<(_, _)> = Arc::new(Default::default());
    let receiver: MulticastReceiver<dpdk::Transport> = MulticastReceiver(counter.clone(), address);
    if !is_client {
        let handle = Handle::from(receiver);
        static START: AtomicU64 = AtomicU64::new(0);
        handle.with_stateful(|state| {
            let clock0 = Arc::new(Clock::new());
            let clock = clock0.clone();
            let submit = state.submit.clone();
            transport.register_multicast(move |_remote, buffer| {
                debug!(
                    "receive multicast {:?}",
                    clock.delta(START.load(Ordering::SeqCst), clock.end())
                );
                submit.stateless(move |shared| {
                    MulticastReceiver::<dpdk::Transport>::receive_buffer(shared, buffer)
                });
            });
            transport.register(&**state, move |_remote, _buffer| {
                // debug!("receive baseline unicast");
                START.store(clock0.start(), Ordering::SeqCst);
            });
        });

        let handle = Arc::new(handle);
        extern "C" fn run(arg: *mut c_void) -> i32 {
            let handle: &Arc<Handle<MulticastReceiver<dpdk::Transport>>> =
                unsafe { &*(arg as *mut _) };
            let handle = handle.clone();
            handle.run_worker(|| false);
            0
        }
        unsafe {
            rte_eal_mp_remote_launch(
                run,
                &handle as *const _ as *mut _,
                rte_rmt_call_main_t::SKIP_MAIN,
            )
        };
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(1));
            let (total, signed) = &*counter;
            println!(
                "{}/{}",
                signed.swap(0, Ordering::SeqCst),
                total.swap(0, Ordering::SeqCst)
            );
        });
        transport.run1(|| false);
    } else {
        let throughput_benchmark = env::args().nth(3).is_none();
        let transport = transport.tx_agent();
        if throughput_benchmark {
            for i in 0..10000000 as u32 {
                transport.send_message(&receiver, &config.multicast.unwrap(), |buffer| {
                    OrderedMulticast::assemble(i, buffer)
                });
            }
        } else {
            for i in 0..10 as u32 {
                println!("send #{}", i);
                transport.send_message_to_all(&receiver, config.replica(..), |_| 0);
                transport.send_message(&receiver, &config.multicast.unwrap(), |buffer| {
                    OrderedMulticast::assemble(i, buffer)
                });
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
