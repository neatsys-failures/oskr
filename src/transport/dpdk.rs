use crate::transport::{self, Config, Receiver};
use std::collections::HashMap;
use std::convert::Infallible;
use std::env;
use std::ffi::CString;
use std::fmt::{self, Display, Formatter};
use std::marker::{PhantomData, PhantomPinned};
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_uint};
use std::ptr::{copy_nonoverlapping, NonNull};
use std::slice;
use std::str::FromStr;
use std::sync::Arc;

#[repr(C)]
struct rte_mempool {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

#[repr(C)]
struct rte_mbuf {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

#[repr(C)]
struct rte_ether_addr {
    addr_bytes: [u8; 6],
}

extern "C" {
    // interfaces that exist in rte_* libraries
    fn rte_eal_init(argc: c_int, argv: NonNull<NonNull<c_char>>) -> c_int;
    fn rte_thread_register() -> c_int;
    fn rte_socket_id() -> c_int;
    fn rte_pktmbuf_pool_create(
        name: NonNull<c_char>,
        n: c_uint,
        cache_size: c_uint,
        priv_size: u16,
        data_room_size: u16,
        socket_id: c_int,
    ) -> *mut rte_mempool;
    fn rte_eth_dev_socket_id(port_id: u16) -> c_int;
    fn rte_eth_macaddr_get(port_id: u16, mac_addr: NonNull<rte_ether_addr>) -> c_int;

    // interfaces that provided in `static inline` fashion, and "instantiated"
    // by DPDK shim
    // the "rte_*" prefix is replaced to "oskr_*" to indicate symbol's owner
    fn oskr_pktmbuf_alloc(mp: NonNull<rte_mempool>) -> *mut rte_mbuf;
    fn oskr_pktmbuf_free(m: NonNull<rte_mbuf>);
    fn oskr_eth_rx_burst(
        port_id: u16,
        queue_id: u16,
        rx_pkts: NonNull<*mut rte_mbuf>,
        nb_pkts: u16,
    ) -> u16;
    fn oskr_eth_tx_burst(
        port_id: u16,
        queue_id: u16,
        tx_pkts: NonNull<NonNull<rte_mbuf>>,
        nb_pkts: u16,
    ) -> u16;
    fn oskr_mbuf_default_buf_size() -> u16; // RTE_MBUF_DEFAULT_BUF_SIZE

    // addiational custom interfaces on rte_mbuf, not correspond to anything of
    // DPDK
    // it seems tricky to replicate rte_mbuf's struct layout from Rust side, so
    // necessary operations directly performed on struct fields are wrapped
    fn mbuf_get_data(mbuf: NonNull<rte_mbuf>) -> NonNull<u8>;
    fn mbuf_get_packet_length(mbuf: NonNull<rte_mbuf>) -> u16;
    fn mbuf_set_packet_length(mbuf: NonNull<rte_mbuf>, length: u16);
    // one interface to hide all setup detail
    fn setup_port(port_id: u16, n_rx: u16, n_tx: u16, pktmpool: NonNull<rte_mempool>) -> c_int;
}

pub struct RxBuffer {
    mbuf: NonNull<rte_mbuf>,
    buffer: NonNull<u8>,
    length: u16,
}

unsafe impl Send for RxBuffer {}

impl AsRef<[u8]> for RxBuffer {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.buffer.as_ptr(), self.length as usize) }
    }
}

impl Drop for RxBuffer {
    fn drop(&mut self) {
        unsafe { oskr_pktmbuf_free(self.mbuf) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Address {
    mac: [u8; 6],
    id: u8,
}

impl FromStr for Address {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split('#');
        let mac = split.next().unwrap();
        let id = split.next().unwrap();
        let mac: Vec<_> = mac
            .split(':')
            .map(|x| u8::from_str_radix(x, 16).unwrap())
            .collect();
        Ok(Self {
            mac: mac.try_into().unwrap(),
            id: id.parse().unwrap(),
        })
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}#{}",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5], self.id
        )
    }
}

pub struct TxAgent {
    pktmpool: NonNull<rte_mempool>,
    port_id: u16,
    config: Arc<Config<Transport>>,
}

unsafe impl Send for TxAgent {}

impl TxAgent {
    unsafe fn set_source_address(data: NonNull<u8>, source: &Address) {
        let data = data.as_ptr();
        copy_nonoverlapping(&source.mac as *const _, data.offset(6), 6);
        copy_nonoverlapping(&source.id, data.offset(15), 1);
        copy_nonoverlapping(
            // https://stackoverflow.com/a/52682687
            &0x8000_u16.to_be_bytes() as *const _,
            data.offset(12),
            2,
        );
    }
    unsafe fn set_dest_address(data: NonNull<u8>, dest: &Address) {
        let data = data.as_ptr();
        copy_nonoverlapping(&dest.mac as *const _, data.offset(0), 6);
        copy_nonoverlapping(&dest.id, data.offset(14), 1);
    }
}

impl transport::TxAgent for TxAgent {
    type Transport = Transport;

    fn config(&self) -> &Config<Self::Transport> {
        &self.config
    }

    fn send_message(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest: &<Self::Transport as transport::Transport>::Address,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        unsafe {
            let mut mbuf = NonNull::new(oskr_pktmbuf_alloc(self.pktmpool)).unwrap();
            let data = mbuf_get_data(mbuf);
            Self::set_source_address(data, source.get_address());
            Self::set_dest_address(data, dest);
            let buffer = slice::from_raw_parts_mut(data.as_ptr().offset(16), 1480);
            let length = message(buffer) + 16;
            mbuf_set_packet_length(mbuf, length);
            let ret = oskr_eth_tx_burst(self.port_id, 0, NonNull::new(&mut mbuf).unwrap(), 1);
            assert_eq!(ret, 1);
        }
    }
    fn send_message_to_all(
        &self,
        source: &impl Receiver<Self::Transport>,
        message: impl FnOnce(&mut [u8]) -> u16,
    ) {
        todo!()
    }
}

pub struct Transport {
    pktmpool: NonNull<rte_mempool>,
    port_id: u16,
    config: Arc<Config<Self>>,
    recv_table: RecvTable,
}
type RecvTable = HashMap<Address, Box<dyn Fn(&Address, RxBuffer) + Send>>;

unsafe impl Send for Transport {}

impl transport::Transport for Transport {
    type Address = Address;
    type RxBuffer = RxBuffer;
    type TxAgent = TxAgent;

    fn tx_agent(&self) -> Self::TxAgent {
        Self::TxAgent {
            pktmpool: self.pktmpool,
            port_id: self.port_id,
            config: self.config.clone(),
        }
    }

    fn register(
        &mut self,
        receiver: &impl Receiver<Self>,
        rx_agent: impl Fn(&Self::Address, Self::RxBuffer) + 'static + Send,
    ) {
        self.recv_table
            .insert(*receiver.get_address(), Box::new(rx_agent));
    }

    fn register_multicast(
        &mut self,
        rx_agent: impl Fn(&Self::Address, Self::RxBuffer) + 'static + Send,
    ) {
        todo!()
    }

    fn ephemeral_address(&self) -> Self::Address {
        let mut mac_addr = MaybeUninit::uninit();
        let mac_addr = unsafe {
            rte_eth_macaddr_get(self.port_id, NonNull::new(mac_addr.as_mut_ptr()).unwrap());
            mac_addr.assume_init()
        };
        let mut address = Self::Address {
            mac: mac_addr.addr_bytes,
            id: 254, // save 255 for multicast
        };
        loop {
            if !self.recv_table.contains_key(&address) {
                return address;
            }
            address.id -= 1;
        }
    }
}

impl Transport {
    pub fn setup(config: Config<Self>, port_id: u16, n_rx: u16) -> Self {
        let args0: Vec<_> = [
            env::args().next().unwrap(),
            //
        ]
        .into_iter()
        .map(|arg| CString::new(arg).unwrap())
        .collect(); // stop here to keep CString alive
        let mut args: Vec<_> = args0
            .iter()
            .map(|arg| NonNull::new(arg.as_ptr() as *mut c_char).unwrap())
            .collect();

        unsafe {
            let ret = rte_eal_init(
                args.len() as i32,
                NonNull::new(&mut *args as *mut [_] as *mut _).unwrap(),
            );
            assert_eq!(ret, 0);
            let name = CString::new("MBUF_POOL").unwrap();
            let pktmpool = rte_pktmbuf_pool_create(
                NonNull::new(name.as_ptr() as *mut _).unwrap(),
                8191,
                250,
                0,
                oskr_mbuf_default_buf_size(),
                rte_eth_dev_socket_id(port_id),
            );
            let pktmpool = NonNull::new(pktmpool).unwrap();

            let ret = setup_port(port_id, n_rx, 1, pktmpool);
            assert_eq!(ret, 0);

            Self {
                port_id,
                pktmpool,
                config: Arc::new(config),
                recv_table: HashMap::new(),
            }
        }
    }

    unsafe fn get_source_address(data: NonNull<u8>) -> Address {
        let data = data.as_ptr();
        let mut address = Address::default();
        copy_nonoverlapping(data.offset(6), &mut address.mac as *mut _, 6);
        copy_nonoverlapping(data.offset(15), &mut address.id, 1);
        address
    }

    unsafe fn get_dest_address(data: NonNull<u8>) -> Address {
        let data = data.as_ptr();
        let mut address = Address::default();
        copy_nonoverlapping(data.offset(0), &mut address.mac as *mut _, 6);
        copy_nonoverlapping(data.offset(14), &mut address.id, 1);
        address
    }

    // must be run with spawn blocking
    pub fn run(&self, queue_id: u16) {
        let ret = unsafe { rte_thread_register() };
        assert_eq!(ret, 0);

        let mut mac_addr = MaybeUninit::uninit();
        let mac_addr = unsafe {
            rte_eth_macaddr_get(self.port_id, NonNull::new(mac_addr.as_mut_ptr()).unwrap());
            mac_addr.assume_init()
        };
        for address in self.recv_table.keys() {
            if address.mac != mac_addr.addr_bytes {
                println!(
                    "warn: registered address (mac = {:?}) and device (mac = {:?}) difference",
                    address.mac, mac_addr.addr_bytes
                );
            }
        }

        let (socket, dev_socket) =
            unsafe { (rte_socket_id(), rte_eth_dev_socket_id(self.port_id)) };
        if socket != dev_socket {
            println!(
                "warn: queue {} rx thread (socket = {}) and device (socket = {}) different",
                queue_id, socket, dev_socket
            );
        }

        loop {
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
                    let data = mbuf_get_data(mbuf);
                    let source = Transport::get_source_address(data);
                    let dest = Transport::get_dest_address(data);
                    let buffer = NonNull::new(data.as_ptr().offset(16)).unwrap();
                    let length = mbuf_get_packet_length(mbuf) - 16;
                    if let Some(rx_agent) = self.recv_table.get(&dest) {
                        rx_agent(
                            &source,
                            RxBuffer {
                                mbuf,
                                buffer,
                                length,
                            },
                        );
                    } else {
                        println!("warn: unknown destination {}", dest);
                    }
                }
            }
        }
    }
}
