use crate::model::{Transport as TransportTrait, *};
use smallvec::SmallVec;
use std::marker::*;
use std::mem::*;
use std::ptr::*;
use std::slice::{from_raw_parts, from_raw_parts_mut};
use std::sync::Arc;
use tokio::sync::mpsc::*;
use tokio::task::*;

#[repr(C)]
struct rte_ethdev {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

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
    fn rte_pktmbuf_alloc(mp: NonNull<rte_mempool>) -> *mut rte_mbuf;
    fn rte_pktmbuf_free(m: NonNull<rte_mbuf>);

    // this two corresponding to `rte_eth_*` which is static inline
    fn rx_burst(port_id: u16, queue_id: u16, rx_pkts: *mut *mut rte_mbuf, nb_pkts: u16) -> u16;
    fn tx_burst(
        port_id: u16,
        queue_id: u16,
        tx_pkts: NonNull<NonNull<rte_mbuf>>,
        nb_pkts: u16,
    ) -> u16;

    fn mbuf_to_buffer_data(mbuf: NonNull<rte_mbuf>) -> NonNull<u8>;
    fn buffer_data_to_mbuf(data: NonNull<u8>) -> NonNull<rte_mbuf>;
    fn mbuf_get_packet_length(mbuf: NonNull<rte_mbuf>) -> u16;
    fn mbuf_set_packet_length(mbuf: NonNull<rte_mbuf>, length: u16);
}

pub struct Transport {
    port: u16,
    pktmpool: NonNull<rte_mempool>,
    config: Arc<Config>,
}

unsafe impl Sync for Transport {}
unsafe impl Send for Transport {}

impl Transport {
    unsafe fn set_source_address(zero: NonNull<u8>, source: &TransportAddress) {
        copy_nonoverlapping(source.0.as_ptr().offset(0), zero.as_ptr().offset(0), 6);
        copy_nonoverlapping(source.0.as_ptr().offset(6), zero.as_ptr().offset(14), 1);
        copy_nonoverlapping(
            // https://stackoverflow.com/a/52682687
            &(0x88d5 as u16).to_be_bytes() as *const _,
            zero.as_ptr().offset(12),
            2,
        );
    }
    unsafe fn set_dest_address(mbuf: NonNull<rte_mbuf>, dest: &TransportAddress) {
        let zero = mbuf_to_buffer_data(mbuf).as_ptr().offset(-16);
        copy_nonoverlapping(dest.0.as_ptr().offset(0), zero.offset(6), 6);
        copy_nonoverlapping(dest.0.as_ptr().offset(6), zero.offset(15), 1);
    }
    unsafe fn get_source_address(mbuf: NonNull<rte_mbuf>) -> TransportAddress {
        let zero = mbuf_to_buffer_data(mbuf).as_ptr().offset(-16);
        TransportAddress({
            let mut address = SmallVec::new();
            address.extend_from_slice(from_raw_parts(zero.offset(0), 6));
            address.push(*zero.offset(14));
            address
        })
    }
    unsafe fn get_dest_address(mbuf: NonNull<rte_mbuf>) -> TransportAddress {
        let zero = mbuf_to_buffer_data(mbuf).as_ptr().offset(-16);
        TransportAddress({
            let mut address = SmallVec::new();
            address.extend_from_slice(from_raw_parts(zero.offset(6), 6));
            address.push(*zero.offset(15));
            address
        })
    }

    unsafe fn drop_rx_buffer_data(&self, data: RxBufferData) {
        rte_pktmbuf_free(buffer_data_to_mbuf(data.0));
    }
}

impl TransportTrait for Transport {
    fn send_message(
        &self,
        source: &dyn TransportReceiver,
        dest: &TransportAddress,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        unsafe {
            let mut mbuf = NonNull::new(rte_pktmbuf_alloc(self.pktmpool)).unwrap();
            Self::set_source_address(
                NonNull::new(mbuf_to_buffer_data(mbuf).as_ptr().offset(-16)).unwrap(),
                source.get_address(),
            );
            Self::set_dest_address(mbuf, dest);
            let length = message(from_raw_parts_mut(mbuf_to_buffer_data(mbuf).as_ptr(), 1480)) + 16;
            mbuf_set_packet_length(mbuf, length);
            tx_burst(self.port, 0, NonNull::new(&mut mbuf).unwrap(), 1);
        }
    }

    fn send_message_to_replica(
        &self,
        source: &dyn TransportReceiver,
        id: ReplicaId,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        self.send_message(source, &self.config.address_list[id as usize], message);
    }

    fn send_message_to_all(
        &self,
        source: &dyn TransportReceiver,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    ) {
        unsafe {
            let mut template: [u8; 1500] = MaybeUninit::uninit().assume_init();
            Self::set_source_address(
                NonNull::new(&mut template as *mut _).unwrap(),
                source.get_address(),
            );
            let length = message(&mut template[16..]) + 16;
            for dest in &self.config.address_list {
                if dest == source.get_address() {
                    continue;
                }
                let mut mbuf = NonNull::new(rte_pktmbuf_alloc(self.pktmpool)).unwrap();
                copy_nonoverlapping(
                    &template as *const u8,
                    mbuf_to_buffer_data(mbuf).as_ptr(),
                    length as usize,
                );
                Self::set_dest_address(mbuf, dest);
                mbuf_set_packet_length(mbuf, length);
                tx_burst(self.port, 0, NonNull::new(&mut mbuf).unwrap(), 1);
            }
        }
    }
}

impl Transport {
    pub fn run_single<R: TransportReceiver, I>(
        config: Arc<Config>,
        receiver: &mut R,
        init: I,
    ) -> impl FnOnce()
    where
        I: FnOnce(&mut R, Arc<dyn TransportTrait>),
    {
        let address = receiver.get_address().clone();
        let inbox = receiver.get_inbox();
        let transport = Arc::new(Transport {
            port: 0,
            pktmpool: NonNull::dangling(), // todo
            config,
        });
        init(receiver, transport.clone());

        move || {
            let (drop_tx, mut drop_rx) = unbounded_channel();
            let drop_transport = transport.clone();
            spawn(async move {
                while let Some(data) = drop_rx.recv().await {
                    unsafe { drop_transport.drop_rx_buffer_data(data) };
                }
            });

            loop {
                let burst = unsafe {
                    let mut burst: MaybeUninit<[*mut rte_mbuf; 32]> = MaybeUninit::uninit();
                    let burst_size = rx_burst(0, 0, burst.as_mut_ptr() as *mut _, 32);
                    &(burst.assume_init()[..burst_size as usize])
                };
                for mbuf in burst {
                    let mbuf = NonNull::new(*mbuf).unwrap();
                    unsafe {
                        if Self::get_dest_address(mbuf) != address {
                            continue;
                        }
                        let rx_buffer = RxBuffer {
                            data: RxBufferData(mbuf_to_buffer_data(mbuf)),
                            length: mbuf_get_packet_length(mbuf),
                            drop_tx: drop_tx.clone(),
                        };
                        inbox(&Self::get_source_address(mbuf), rx_buffer);
                    }
                }
            }
        }
    }
}
