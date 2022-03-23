use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use futures::Future;
use tokio::{
    spawn,
    task::JoinHandle as TokioHandle,
    time::{sleep_until, Sleep},
};

use crate::async_ecosystem;

pub struct JoinHandle<T>(TokioHandle<T>);

pub struct AsyncEcosystem;
impl<T: Send + 'static> async_ecosystem::AsyncEcosystem<T> for AsyncEcosystem {
    type JoinHandle = JoinHandle<T>;
    type Sleep = Sleep;

    fn spawn(task: impl Future<Output = T> + Send + 'static) -> Self::JoinHandle {
        JoinHandle(spawn(task))
    }

    fn cancel(handle: Self::JoinHandle) {
        handle.0.abort();
    }

    fn sleep_until(instant: Instant) -> Self::Sleep {
        sleep_until(instant.into())
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = T;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        Pin::new(&mut self.0).poll(cx).map(Result::unwrap)
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
