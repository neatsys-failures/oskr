pub mod simulated;

use crate::common::ReplicaId;
use std::fmt::Debug;

pub trait Transport
where
    Self: 'static,
{
    type Address: Clone + Eq + Send + Debug;
    type RxBuffer: AsRef<[u8]> + Send + Debug;
    type TxAgent: TxAgent<Transport = Self> + Send;

    fn tx_agent(&self) -> Self::TxAgent;
    fn config(&self) -> &Config<Self>;

    fn register(
        &mut self,
        receiver: &impl Receiver<Self>,
        rx_agent: impl Fn(&Self::Address, Self::RxBuffer) + 'static + Send,
    ) where
        Self: Sized;
    fn register_multicast(
        &mut self,
        rx_agent: impl Fn(&Self::Address, Self::RxBuffer) + 'static + Send,
    );

    fn allocate_address(&self) -> Self::Address;
}

pub trait Receiver<T: Transport> {
    fn get_address(&self) -> &T::Address;
    // anything else?
}

pub trait TxAgent {
    type Transport: Transport;
    fn send_message(
        &self,
        source: &impl Receiver<Self::Transport>,
        dest: &<Self::Transport as Transport>::Address,
        message: impl FnOnce(&mut [u8]) -> u16,
    );
    fn send_message_to_replica(
        &self,
        source: &impl Receiver<Self::Transport>,
        replica_id: ReplicaId,
        message: impl FnOnce(&mut [u8]) -> u16,
    );
    fn send_message_to_all(
        &self,
        source: &impl Receiver<Self::Transport>,
        message: impl FnOnce(&mut [u8]) -> u16,
    );
    fn send_message_to_multicast(
        &self,
        source: &impl Receiver<Self::Transport>,
        message: impl FnOnce(&mut [u8]) -> u16,
    );
}

pub struct Config<T: Transport + ?Sized> {
    pub replica_address: Vec<T::Address>,
    pub multicast_address: Option<T::Address>,
    pub n_fault: usize,
}
