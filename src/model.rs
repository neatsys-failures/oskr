use smallvec::SmallVec;
use std::any::Any;
use std::fmt::*;
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::Arc;

pub(crate) type OpNumber = u64;
pub(crate) type ViewNumber = u16;
pub(crate) type ReplicaId = i8;
pub(crate) type ClientId = [u8; 4];
pub type Data = Vec<u8>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransportAddress(pub SmallVec<[u8; 16]>);

pub trait Transport
where
    Self: Send + Sync,
{
    fn deref_rx_buffer(&self, data: RxBufferData) -> &[u8];
    fn drop_rx_buffer(&self, data: RxBufferData);

    fn send_message(
        self: Arc<Self>,
        source: &dyn TransportReceiver,
        dest: &TransportAddress,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    );
    fn send_message_to_replica(
        self: Arc<Self>,
        source: &dyn TransportReceiver,
        id: ReplicaId,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    );
    fn send_message_to_all(
        self: Arc<Self>,
        source: &dyn TransportReceiver,
        message: &mut dyn FnMut(&mut [u8]) -> u16,
    );
}

pub trait TransportReceiver {
    fn get_address(&self) -> &TransportAddress;
    fn get_inbox(&self) -> Box<dyn Send + Sync + Fn(&TransportAddress, RxBuffer)>;
}

#[derive(Debug, Clone)]
pub struct RxBufferData(pub NonNull<dyn Any>);

unsafe impl Send for RxBufferData {}

pub struct RxBuffer {
    pub data: RxBufferData,
    pub transport: Arc<dyn Transport>,
}

impl Debug for RxBuffer {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("RxBuffer")
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl Deref for RxBuffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.transport.deref_rx_buffer(self.data.clone())
    }
}

impl Drop for RxBuffer {
    fn drop(&mut self) {
        self.transport.drop_rx_buffer(self.data.clone());
    }
}

pub struct Config {
    pub f: u8,
    pub address_list: Vec<TransportAddress>,
}

pub trait App {
    fn execute(&mut self, op: &[u8]) -> Vec<u8>;
}

pub trait Invoke {
    fn invoke<'a>(&'a mut self, op: Data) -> Pin<Box<dyn 'a + Future<Output = Data>>>;
}
