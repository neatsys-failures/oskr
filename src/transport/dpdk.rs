use crate::transport::{self, Config, Receiver};
use std::collections::HashMap;
use std::ffi::CString;
use std::marker::{PhantomData, PhantomPinned};
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_uint};
use std::ptr::{copy_nonoverlapping, NonNull};
use std::slice;
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

extern "C" {
    // interfaces that exist in rte_* libraries
    fn rte_eal_init(argc: c_int, argv: NonNull<NonNull<c_char>>) -> c_int;
    fn rte_pktmbuf_pool_create(
        name: NonNull<c_char>,
        n: c_uint,
        cache_size: c_uint,
        priv_size: u16,
        data_room_size: u16,
        socket_id: c_int,
    ) -> *mut rte_mempool;
    fn rte_eth_dev_socket_id(port_id: u16) -> c_int;

    // interfaces that provided in `static inline` fashion, and "instantiated"
    // by DPDK shim
    // the "rte_*" prefix is replaced to "oskr_*" to indicate symbol's owner
    fn oskr_pktmbuf_alloc(mp: NonNull<rte_mempool>) -> *mut rte_mbuf;
    fn oskr_pktmbuf_free(m: NonNull<rte_mbuf>);
    fn oskr_eth_rx_burst(
        port_id: u16,
        queue_id: u16,
        rx_pkts: *mut *mut rte_mbuf,
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
    fn mbuf_to_buffer_data(mbuf: NonNull<rte_mbuf>) -> NonNull<u8>;
    fn mbuf_get_packet_length(mbuf: NonNull<rte_mbuf>) -> u16;
    fn mbuf_set_packet_length(mbuf: NonNull<rte_mbuf>, length: u16);
}

pub struct RxBuffer {
    mbuf: NonNull<rte_mbuf>,
    data: NonNull<u8>,
    length: u16,
}

unsafe impl Send for RxBuffer {}

impl AsRef<[u8]> for RxBuffer {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data.as_ptr(), self.length as usize) }
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

pub struct TxAgent {
    pktmpool: NonNull<rte_mempool>,
    port_id: u16,
    config: Arc<Config<Transport>>,
}

unsafe impl Send for TxAgent {}

impl TxAgent {
    unsafe fn set_source_address(data: NonNull<u8>, source: &Address) {
        let data = data.as_ptr();
        copy_nonoverlapping(&source.mac as *const _, data.offset(0), 6);
        copy_nonoverlapping(&source.id, data.offset(14), 1);
        copy_nonoverlapping(
            // https://stackoverflow.com/a/52682687
            &0x88d5_u16.to_be_bytes() as *const _,
            data.offset(12),
            2,
        );
    }
    unsafe fn set_dest_address(data: NonNull<u8>, dest: &Address) {
        let data = data.as_ptr();
        copy_nonoverlapping(&dest.mac as *const _, data.offset(6), 6);
        copy_nonoverlapping(&dest.id, data.offset(15), 1);
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
            let data = mbuf_to_buffer_data(mbuf);
            Self::set_source_address(data, source.get_address());
            Self::set_dest_address(data, dest);
            let buffer = slice::from_raw_parts_mut(data.as_ptr(), 1480);
            let length = message(buffer) + 16;
            mbuf_set_packet_length(mbuf, length);
            oskr_eth_tx_burst(self.port_id, 0, NonNull::new(&mut mbuf).unwrap(), 1);
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
pub type RecvTable = HashMap<Address, Box<dyn Fn(&Address, RxBuffer) + Send>>;

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
        Self::Address { mac: [0; 6], id: 0 } // TODO
    }
}

impl Transport {
    pub fn new(config: Config<Self>) -> Self {
        let port_id = 0; // TODO
        let pktmpool = unsafe {
            let name = CString::new("MBUF_POOL").unwrap();
            rte_pktmbuf_pool_create(
                NonNull::new(name.as_ptr() as *mut _).unwrap(),
                8191,
                250,
                0,
                oskr_mbuf_default_buf_size(),
                rte_eth_dev_socket_id(port_id),
            )
        };
        let pktmpool = NonNull::new(pktmpool).unwrap();
        Self {
            port_id,
            pktmpool,
            config: Arc::new(config),
            recv_table: HashMap::new(),
        }
    }

    unsafe fn get_source_address(data: NonNull<u8>) -> Address {
        let data = data.as_ptr();
        let mut address = Address::default();
        copy_nonoverlapping(data.offset(0), &mut address.mac as *mut _, 6);
        copy_nonoverlapping(data.offset(14), &mut address.id, 1);
        address
    }

    unsafe fn get_dest_address(data: NonNull<u8>) -> Address {
        let data = data.as_ptr();
        let mut address = Address::default();
        copy_nonoverlapping(data.offset(6), &mut address.mac as *mut _, 6);
        copy_nonoverlapping(data.offset(15), &mut address.id, 1);
        address
    }

    // must be run with spawn blocking
    pub fn run(&self, queue_id: u16) {
        loop {
            let burst = unsafe {
                let mut burst: MaybeUninit<[*mut rte_mbuf; 32]> = MaybeUninit::uninit();
                let burst_size =
                    oskr_eth_rx_burst(self.port_id, queue_id, burst.as_mut_ptr() as *mut _, 32);
                &(burst.assume_init())[..burst_size as usize]
            };
            for mbuf in burst {
                let mbuf = NonNull::new(*mbuf).unwrap();
                unsafe {
                    let data = mbuf_to_buffer_data(mbuf);
                    let source = Transport::get_source_address(data);
                    let dest = Transport::get_dest_address(data);
                    let length = mbuf_get_packet_length(mbuf);
                    if let Some(rx_agent) = self.recv_table.get(&dest) {
                        rx_agent(&source, RxBuffer { mbuf, data, length });
                    } else {
                        println!("warn: unknown destination {:?}", dest);
                    }
                }
            }
        }
    }
}
