use std::{
    collections::HashMap,
    env,
    ffi::CString,
    mem::MaybeUninit,
    os::raw::{c_char, c_int, c_uint},
    ptr::NonNull,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use crossbeam::utils::Backoff;

use crate::{
    dpdk_shim::{
        oskr_eth_rx_burst, oskr_eth_tx_burst, oskr_lcore_id, oskr_mbuf_default_buf_size,
        oskr_pktmbuf_alloc, oskr_pktmbuf_alloc_bulk, rte_eal_init, rte_eth_dev_socket_id,
        rte_eth_macaddr_get, rte_lcore_index, rte_mbuf, rte_mempool, rte_pktmbuf_pool_create,
        rte_socket_id, setup_port, Address, RxBuffer,
    },
    facade::{self, Config, Receiver},
};

#[derive(Clone)]
pub struct TxAgent {
    mbuf_pool: NonNull<rte_mempool>,
    port_id: u16,
    config: Arc<Config<Transport>>,
    rr: Arc<RoundRobin>,
}

struct RoundRobin {
    sequence: AtomicU32,
    counter: [AtomicU32; 32], // 32 tx queue should be maximum expectation right?
    n: u32,
}

impl RoundRobin {
    fn new(n: u32) -> Self {
        Self {
            sequence: AtomicU32::new(0),
            counter: (0..32)
                .map(|_| AtomicU32::new(0))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            n,
        }
    }

    fn acquire(&self) -> u32 {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let (i, c) = (sequence % self.n, sequence / self.n);
        let backoff = Backoff::new();
        while self.counter[i as usize].load(Ordering::SeqCst) != c {
            backoff.snooze();
        }
        i
    }

    fn release(&self, i: u32) {
        self.counter[i as usize].fetch_add(1, Ordering::SeqCst);
    }
}

unsafe impl Send for TxAgent {}
unsafe impl Sync for TxAgent {}

impl facade::TxAgent for TxAgent {
    type Transport = Transport;

    fn config(&self) -> &Config<Self::Transport> {
        &self.config
    }

    fn send_message(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest: &<Self::Transport as facade::Transport>::Address,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        unsafe {
            let mut mbuf = NonNull::new(oskr_pktmbuf_alloc(self.mbuf_pool)).unwrap();
            let data = rte_mbuf::get_data(mbuf);
            rte_mbuf::set_source(data, source.get_address());
            rte_mbuf::set_dest(data, dest);
            let length = message(rte_mbuf::get_tx_buffer(data));
            rte_mbuf::set_buffer_length(mbuf, length);

            let queue_id = self.rr.acquire();
            let ret = oskr_eth_tx_burst(self.port_id, queue_id as u16, (&mut mbuf).into(), 1);
            assert_eq!(ret, 1);
            self.rr.release(queue_id);
        }
    }

    fn send_message_to_all(
        &self,
        source: &impl Receiver<Self::Transport>,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        let dest_list: Vec<_> = self
            .config
            .replica_address
            .iter()
            .filter(|dest| *dest != source.get_address())
            .collect();
        if dest_list.is_empty() {
            return;
        }
        assert!(dest_list.len() <= 32); // TODO

        let mbuf_list = unsafe {
            let mut mbuf_list: MaybeUninit<[*mut rte_mbuf; 32]> = MaybeUninit::uninit();
            let ret = oskr_pktmbuf_alloc_bulk(
                self.mbuf_pool,
                NonNull::new(mbuf_list.as_mut_ptr() as *mut _).unwrap(),
                dest_list.len() as c_uint,
            );
            assert_eq!(ret, 0);
            &mbuf_list.assume_init()[..dest_list.len()]
        };

        let sample_mbuf = NonNull::new(mbuf_list[0]).unwrap();
        let sample_data = unsafe { rte_mbuf::get_data(sample_mbuf) };
        let length = message(unsafe { rte_mbuf::get_tx_buffer(sample_data) });

        let mut mbuf_list: Vec<_> = mbuf_list
            .iter()
            .zip(dest_list)
            .enumerate()
            .map(|(i, (mbuf, dest))| unsafe {
                let (mbuf, data) = if i == 0 {
                    rte_mbuf::set_source(sample_data, source.get_address());
                    (sample_mbuf, sample_data)
                } else {
                    let mbuf = NonNull::new(*mbuf).unwrap();
                    let data = rte_mbuf::get_data(mbuf);
                    // TODO hide length + 16 behide rte_mbuf abstraction
                    rte_mbuf::copy_data(sample_data, data, length);
                    (mbuf, data)
                };
                rte_mbuf::set_dest(data, dest);
                rte_mbuf::set_buffer_length(mbuf, length);
                mbuf
            })
            .collect();

        let queue_id = self.rr.acquire();
        let ret = unsafe {
            oskr_eth_tx_burst(
                self.port_id,
                queue_id as u16,
                mbuf_list.first_mut().unwrap().into(),
                mbuf_list.len() as u16,
            )
        };
        assert_eq!(ret, mbuf_list.len() as u16);
        self.rr.release(queue_id);
    }
}

pub struct Transport {
    mbuf_pool: NonNull<rte_mempool>,
    port_id: u16,
    config: Arc<Config<Self>>,
    recv_table: RecvTable,
    rr: Arc<RoundRobin>,
}
type RecvTable = HashMap<Address, Box<dyn Fn(Address, RxBuffer) + Send>>;

unsafe impl Send for Transport {}

impl facade::Transport for Transport {
    type Address = Address;
    type RxBuffer = RxBuffer;
    type TxAgent = TxAgent;

    fn tx_agent(&self) -> Self::TxAgent {
        Self::TxAgent {
            mbuf_pool: self.mbuf_pool,
            port_id: self.port_id,
            config: self.config.clone(),
            rr: self.rr.clone(),
        }
    }

    fn register(
        &mut self,
        receiver: &impl Receiver<Self>,
        rx_agent: impl Fn(Self::Address, Self::RxBuffer) + 'static + Send,
    ) {
        let mut port_mac = MaybeUninit::uninit();
        let ret = unsafe {
            rte_eth_macaddr_get(self.port_id, NonNull::new(port_mac.as_mut_ptr()).unwrap())
        };
        assert_eq!(ret, 0);
        let port_mac = unsafe { port_mac.assume_init() };
        if port_mac.addr_bytes != receiver.get_address().mac {
            panic!();
        }

        self.recv_table
            .insert(*receiver.get_address(), Box::new(rx_agent));
    }

    fn register_multicast(
        &mut self,
        rx_agent: impl Fn(Self::Address, Self::RxBuffer) + 'static + Send,
    ) {
        todo!()
    }

    fn ephemeral_address(&self) -> Self::Address {
        for id in 200..=u16::MAX >> 1 {
            let address = Address::new_local(self.port_id, id);
            if !self.recv_table.contains_key(&address) {
                return address;
            }
        }
        unreachable!();
    }
}

impl Transport {
    pub fn setup(
        config: Config<Self>,
        core_mask: u128,
        port_id: u16,
        n_rx: u16,
        n_tx: u16,
    ) -> Self {
        let args = [
            env::args().next().unwrap(),
            "-c".to_string(),
            format!("{:x}", core_mask),
            "-d".to_string(),
            "./target/dpdk/drivers/".to_string(), // TODO any better way?
            "--no-telemetry".to_string(),
        ];
        let args: Vec<_> = args
            .into_iter()
            .map(|arg| CString::new(arg).unwrap())
            .collect(); // stop here to keep CString alive
        let mut args: Vec<_> = args
            .iter()
            .map(|arg| NonNull::new(arg.as_ptr() as *mut c_char).unwrap())
            .collect();

        unsafe {
            let ret = rte_eal_init(args.len() as c_int, args.first_mut().unwrap().into());
            assert_eq!(ret, args.len() as c_int - 1);

            let name = CString::new("MBUF_POOL").unwrap();
            let pktmpool = rte_pktmbuf_pool_create(
                NonNull::new(name.as_ptr() as *mut _).unwrap(),
                // is it necessary to scale mbuf number according to worker number as well?
                8192 + 2048 * ((n_tx + n_rx) as u32),
                256,
                0,
                oskr_mbuf_default_buf_size(),
                rte_eth_dev_socket_id(port_id),
            );
            let mbuf_pool = NonNull::new(pktmpool).unwrap();

            let ret = setup_port(port_id, n_rx, n_tx, mbuf_pool);
            assert_eq!(ret, 0);

            Self {
                port_id,
                mbuf_pool,
                config: Arc::new(config),
                recv_table: HashMap::new(),
                rr: Arc::new(RoundRobin::new(n_tx as u32)),
            }
        }
    }

    pub fn worker_id() -> usize {
        (unsafe { rte_lcore_index(oskr_lcore_id() as c_int) }) as usize - 1
    }

    // rethink for a better interface
    pub fn check_socket(&self) -> bool {
        let (socket, dev_socket) =
            unsafe { (rte_socket_id(), rte_eth_dev_socket_id(self.port_id)) };
        if socket != dev_socket {
            println!(
                "warn: rx thread (socket = {}) and device (socket = {}) different",
                socket, dev_socket
            );
        }
        socket == dev_socket
    }

    fn run_internal(&self, queue_id: u16, dispatch: impl Fn(Address, Address, RxBuffer) -> bool) {
        let burst = unsafe {
            let mut burst: MaybeUninit<[*mut rte_mbuf; 32]> = MaybeUninit::uninit();
            let burst_size = oskr_eth_rx_burst(
                self.port_id,
                queue_id,
                NonNull::new(burst.as_mut_ptr() as *mut _).unwrap(),
                32,
            );
            &(burst.assume_init())[..burst_size as usize]
        };
        for mbuf in burst {
            let mbuf = NonNull::new(*mbuf).unwrap();
            unsafe {
                let data = rte_mbuf::get_data(mbuf);
                let (source, dest) = (rte_mbuf::get_source(data), rte_mbuf::get_dest(data));
                if !dispatch(source, dest, rte_mbuf::into_rx_buffer(mbuf, data)) {
                    println!("warn: unknown destination {}", dest);
                }
            }
        }
    }

    pub fn poll(&self, queue_id: u16) {
        self.run_internal(queue_id, |source, dest, buffer| {
            if let Some(rx_agent) = self.recv_table.get(&dest) {
                rx_agent(source, buffer);
                true
            } else {
                false
            }
        });
    }

    pub fn run(&self, queue_id: u16, mut shutdown: impl FnMut() -> bool) {
        self.check_socket();
        while !shutdown() {
            self.poll(queue_id);
        }
    }

    pub fn run1(&self, mut shutdown: impl FnMut() -> bool) {
        assert_eq!(self.recv_table.len(), 1);
        let (address, rx_agent) = self.recv_table.iter().next().unwrap();
        while !shutdown() {
            self.run_internal(0, |source, dest, buffer| {
                if dest == *address {
                    rx_agent(source, buffer);
                    true
                } else {
                    false
                }
            });
        }
    }
}
