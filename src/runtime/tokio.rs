use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{future::BoxFuture, Future};
use tokio::{
    spawn,
    task::JoinHandle as TokioHandle,
    time::{error::Elapsed, sleep, timeout, Sleep, Timeout},
};

use crate::async_ecosystem;

pub struct AsyncExecutor;
impl<'a, T: Send + 'static> crate::AsyncExecutor<'a, T> for AsyncExecutor {
    type JoinHandle = BoxFuture<'static, T>;
    type Timeout = Timeout<BoxFuture<'a, T>>;
    type Elapsed = Elapsed;

    fn spawn(task: impl Future<Output = T> + Send + 'static) -> Self::JoinHandle {
        Box::pin(async move { spawn(task).await.unwrap() })
    }

    fn timeout(duration: Duration, task: impl Future<Output = T> + Send + 'a) -> Self::Timeout {
        timeout(duration, Box::pin(task))
    }
}

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

    fn sleep(duration: Duration) -> Self::Sleep {
        sleep(duration)
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
