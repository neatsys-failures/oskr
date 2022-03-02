use std::{
    env,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    thread::{sleep, spawn},
    time::Duration,
};

use oskr::transport::{self, dpdk::Transport, Config, Transport as _, TxAgent};

fn main() {
    let server_address = "b8:ce:f6:2a:2f:94#0".parse().unwrap();
    let port_id = 0;

    let config = Config {
        replica_address: vec![server_address],
        n_fault: 0,
        multicast_address: None,
    };
    let mut transport = Transport::setup(config, port_id, 1, 1);

    let mut args = env::args();
    let _prog = args.next();
    let invoke = args.next().map(|arg| arg == "invoke").unwrap_or(false);
    let mut n_concurrent = 1;
    if invoke {
        if let Some(arg) = args.next() {
            n_concurrent = arg.parse().unwrap();
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Receiver(<Transport as transport::Transport>::Address);
    impl transport::Receiver<Transport> for Receiver {
        fn get_address(&self) -> &<Transport as transport::Transport>::Address {
            &self.0
        }
    }

    let count = Arc::new(AtomicU32::new(0));
    let register = |transport: &mut Transport, receiver| {
        transport.register(&receiver, {
            let transport = transport.tx_agent();
            let count = count.clone();
            move |remote, _buffer| {
                transport.send_message(&receiver, &remote, |_buffer| 0);
                count.fetch_add(1, Ordering::SeqCst);
            }
        })
    };

    if invoke {
        for _ in 0..n_concurrent {
            let receiver = Receiver(transport.ephemeral_address());
            register(&mut transport, receiver);
            transport
                .tx_agent()
                .send_message(&receiver, &server_address, |_buffer| 0);
        }
    } else {
        register(&mut transport, Receiver(server_address));
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
