use std::time::Duration;

use futures::{future::BoxFuture, Future};
use tokio::{
    spawn,
    time::{error::Elapsed, timeout, Timeout},
};

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
