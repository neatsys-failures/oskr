use std::{
    env, fs,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use oskr::{
    common::Config,
    facade::{self, Receiver, Transport, TxAgent},
    framework::dpdk,
    protocol::tombft::message::TrustedOrderedMulticast,
    stage::{Handle, State},
};
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
        let message = TrustedOrderedMulticast::<u32>::new(buffer.as_ref());
        let verified = message.verify(()).unwrap();
        shared.0.fetch_add(1, Ordering::SeqCst);
        if verified.trusted.is_some() {
            shared.1.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn main() {
    let config = env::args().nth(1).unwrap();
    let config: facade::Config<dpdk::Transport> =
        fs::read_to_string(config).unwrap().parse().unwrap();
    let config = Config::for_shard(config, 0);

    let role = env::args().nth(2).unwrap();
    let is_client = role == "client";
    let replica_id = if is_client { 0 } else { role.parse().unwrap() };

    let mut transport = dpdk::Transport::setup(0xf, 0, 1, 1);
    let address = if is_client {
        transport.ephemeral_address()
    } else {
        config.replica(replica_id).clone()
    };
    let counter: Arc<(_, _)> = Arc::new(Default::default());
    let receiver: MulticastReceiver<dpdk::Transport> = MulticastReceiver(counter.clone(), address);
    if !is_client {
        let handle = Handle::from(receiver);
        handle.with_stateful(|state| {
            let submit = state.submit.clone();
            transport.register_multicast(move |_remote, buffer| {
                debug!("receive multicast");
                submit.stateless(move |shared| {
                    MulticastReceiver::<dpdk::Transport>::receive_buffer(shared, buffer)
                });
            });
            transport.register(&**state, |_remote, _buffer| {
                debug!("receive baseline unicast")
            });
        });
    } else {
        let throughput_benchmark = env::args().nth(3).is_none();
        let transport = transport.tx_agent();
        if throughput_benchmark {
            for i in 0..10000000 as u32 {
                transport.send_message(&receiver, &config.multicast.unwrap(), |buffer| {
                    TrustedOrderedMulticast::send(i, buffer)
                });
            }
        } else {
            for i in 0..10 as u32 {
                transport.send_message(&receiver, &config.multicast.unwrap(), |buffer| {
                    TrustedOrderedMulticast::send(i, buffer)
                });
                transport.send_message_to_all(&receiver, config.replica(..), |_| 0);
            }
        }
    }
}
