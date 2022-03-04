use std::sync::Arc;

use crate::transport::{Receiver, Transport};

pub mod worker_pool;

pub trait Executor<State>
where
    Self: From<State>,
{
    fn register_stateful<T: Transport>(
        self: &Arc<Self>,
        transport: &mut T,
        // like this, or simply fn(...)?
        task: impl Fn(&mut State, &Self, T::Address, T::RxBuffer) + Send + Copy + 'static,
    ) where
        State: Receiver<T> + Send + 'static;

    fn submit_stateful(&self, task: impl FnOnce(&mut State, &Self) + Send + 'static);
    fn submit_stateless(&self, task: impl FnOnce(&Self) + Send + 'static);
}
