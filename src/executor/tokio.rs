use std::sync::Arc;

use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    Mutex,
};

#[derive(Clone)]
pub struct Submit<State>(UnboundedSender<Task<State>>);
enum Task<State> {
    Stateful(Box<dyn FnOnce(&mut State)>),
    Stateless(Box<dyn FnOnce()>),
}

impl<S> Submit<S> {
    pub fn spawn_new() -> (Self, Spawn<S>) {
        let (tx, rx) = unbounded_channel();
        (Self(tx), Spawn(rx))
    }
}

pub struct Spawn<State>(UnboundedReceiver<Task<State>>);
impl<S> Spawn<S> {
    pub async fn run(&mut self, mut state: S) {
        while let Some(task) = self.0.recv().await {
            match task {
                Task::Stateful(task) => task(&mut state),
                Task::Stateless(task) => task(),
            }
        }
        unreachable!()
    }
}
