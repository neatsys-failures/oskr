use smallvec::SmallVec;
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::slice::from_raw_parts;
use tokio::sync::mpsc::UnboundedSender;

pub(crate) type OpNumber = u64;
pub(crate) type ViewNumber = u16;
pub(crate) type ReplicaId = i8;
pub(crate) type ClientId = [u8; 4];
pub type Data = Vec<u8>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransportAddress(pub SmallVec<[u8; 16]>);

pub trait Transport
where
    Self: Sync,
{
    fn send_message(
        &self,
        source: &dyn TransportReceiver,
        dest: &TransportAddress,
        message: &mut dyn FnMut(&mut [u8]) -> u64,
    );
    fn send_message_to_replica(
        &self,
        source: &dyn TransportReceiver,
        id: ReplicaId,
        message: &mut dyn FnMut(&mut [u8]) -> u64,
    );
    fn send_message_to_all(
        &self,
        source: &dyn TransportReceiver,
        message: &mut dyn FnMut(&mut [u8]) -> u64,
    );
}

pub trait TransportReceiver {
    fn get_address(&self) -> &TransportAddress;
    fn get_inbox(&self) -> Box<dyn Fn(&TransportAddress, RxBuffer)>;
}

#[derive(Debug, Clone)]
pub struct RxBufferData(pub *const u8);

unsafe impl Send for RxBufferData {}

#[derive(Debug)]
pub struct RxBuffer {
    pub data: RxBufferData,
    pub length: usize,
    pub drop_tx: UnboundedSender<RxBufferData>,
}

impl Deref for RxBuffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe { from_raw_parts(self.data.0, self.length) }
    }
}

impl Drop for RxBuffer {
    fn drop(&mut self) {
        self.drop_tx.send(self.data.clone()).unwrap();
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
