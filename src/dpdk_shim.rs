use std::{
    convert::Infallible,
    ffi::c_void,
    fmt::{self, Display, Formatter},
    intrinsics::copy_nonoverlapping,
    marker::{PhantomData, PhantomPinned},
    mem::MaybeUninit,
    os::raw::{c_char, c_int, c_uint},
    ptr::NonNull,
    slice,
    str::FromStr,
};

#[repr(C)]
pub struct rte_mempool {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

#[repr(C)]
pub struct rte_mbuf {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

#[repr(C)]
pub struct rte_ether_addr {
    addr_bytes: [u8; 6],
}

#[repr(C)]
#[allow(non_camel_case_types)]
pub enum rte_rmt_call_main_t {
    SKIP_MAIN = 0,
    CALL_MAIN,
}

extern "C" {
    // interfaces that exist in rte_* libraries
    pub fn rte_eal_init(argc: c_int, argv: NonNull<NonNull<c_char>>) -> c_int;
    pub fn rte_thread_register() -> c_int;
    pub fn rte_eal_mp_remote_launch(
        f: extern "C" fn(*mut c_void) -> c_int,
        arg: *mut c_void,
        call_main: rte_rmt_call_main_t,
    );
    pub fn rte_socket_id() -> c_int;
    pub fn rte_lcore_index(lcore_id: c_int) -> c_int;
    pub fn rte_pktmbuf_pool_create(
        name: NonNull<c_char>,
        n: c_uint,
        cache_size: c_uint,
        priv_size: u16,
        data_room_size: u16,
        socket_id: c_int,
    ) -> *mut rte_mempool;
    pub fn rte_eth_dev_socket_id(port_id: u16) -> c_int;
    fn rte_eth_macaddr_get(port_id: u16, mac_addr: NonNull<rte_ether_addr>) -> c_int;

    // interfaces that provided in `static inline` fashion, and "instantiated"
    // by DPDK shim
    // the "rte_*" prefix is replaced to "oskr_*" to indicate symbol's owner
    pub fn oskr_pktmbuf_alloc(mp: NonNull<rte_mempool>) -> *mut rte_mbuf;
    pub fn oskr_pktmbuf_free(m: NonNull<rte_mbuf>);
    pub fn oskr_eth_rx_burst(
        port_id: u16,
        queue_id: u16,
        rx_pkts: NonNull<*mut rte_mbuf>,
        nb_pkts: u16,
    ) -> u16;
    pub fn oskr_eth_tx_burst(
        port_id: u16,
        queue_id: u16,
        tx_pkts: NonNull<NonNull<rte_mbuf>>,
        nb_pkts: u16,
    ) -> u16;
    pub fn oskr_mbuf_default_buf_size() -> u16; // RTE_MBUF_DEFAULT_BUF_SIZE
    pub fn oskr_lcore_id() -> c_uint;

    // addiational custom interfaces on rte_mbuf, not correspond to anything of
    // DPDK
    // it seems tricky to replicate rte_mbuf's struct layout from Rust side, so
    // necessary operations directly performed on struct fields are wrapped
    fn mbuf_get_data(mbuf: NonNull<rte_mbuf>) -> NonNull<u8>;
    fn mbuf_get_packet_length(mbuf: NonNull<rte_mbuf>) -> u16;
    fn mbuf_set_packet_length(mbuf: NonNull<rte_mbuf>, length: u16);

    // one interface to hide all setup detail
    pub fn setup_port(port_id: u16, n_rx: u16, n_tx: u16, pktmpool: NonNull<rte_mempool>) -> c_int;
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
    pub id: u8,
}

impl Address {
    pub unsafe fn new_local(port_id: u16, id: u8) -> Self {
        let mut mac_addr = MaybeUninit::uninit();
        rte_eth_macaddr_get(port_id, NonNull::new(mac_addr.as_mut_ptr()).unwrap());
        let mac = mac_addr.assume_init().addr_bytes;
        Self { mac, id }
    }
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

impl rte_mbuf {
    pub unsafe fn get_data(mbuf: NonNull<rte_mbuf>) -> NonNull<u8> {
        mbuf_get_data(mbuf)
    }

    pub unsafe fn set_buffer_length(mbuf: NonNull<rte_mbuf>, length: u16) {
        mbuf_set_packet_length(mbuf, length + 16);
    }

    // this method instead of Into<RxBuffer> because I want it keep unsafe
    pub unsafe fn into_rx_buffer(mbuf: NonNull<rte_mbuf>, data: NonNull<u8>) -> RxBuffer {
        let buffer = NonNull::new(data.as_ptr().offset(16)).unwrap();
        let length = mbuf_get_packet_length(mbuf) - 16;
        RxBuffer {
            mbuf,
            buffer,
            length,
        }
    }

    pub unsafe fn get_tx_buffer<'a>(data: NonNull<u8>) -> &'a mut [u8] {
        // TODO decide maximum length with reason
        slice::from_raw_parts_mut(data.as_ptr().offset(16), 1480)
    }

    pub unsafe fn get_source(data: NonNull<u8>) -> Address {
        let data = data.as_ptr();
        let mut address = Address::default();
        copy_nonoverlapping(data.offset(6), &mut address.mac as *mut _, 6);
        address.id = *data.offset(15);
        address
    }

    pub unsafe fn set_source(data: NonNull<u8>, address: &Address) {
        let data = data.as_ptr();
        copy_nonoverlapping(&address.mac as *const _, data.offset(6), 6);
        *data.offset(15) = address.id;
        // ethernet type
        copy_nonoverlapping(&0x88d5u16.to_be_bytes() as *const _, data.offset(12), 2);
    }

    pub unsafe fn get_dest(data: NonNull<u8>) -> Address {
        let data = data.as_ptr();
        let mut address = Address::default();
        copy_nonoverlapping(data.offset(0), &mut address.mac as *mut _, 6);
        address.id = *data.offset(14);
        address
    }

    pub unsafe fn set_dest(data: NonNull<u8>, address: &Address) {
        let data = data.as_ptr();
        copy_nonoverlapping(&address.mac as *const _, data.offset(0), 6);
        *data.offset(14) = address.id;
    }
}
