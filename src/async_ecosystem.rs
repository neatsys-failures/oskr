use std::time::Duration;

use futures::Future;

// wait for GAT to remove trait generic parameter
pub trait AsyncEcosystem<T> {
    type JoinHandle: Future<Output = T> + Send;
    type Sleep: Future<Output = ()> + Send;

    fn spawn(task: impl Future<Output = T> + Send + 'static) -> Self::JoinHandle;
    fn cancel(handle: Self::JoinHandle);
    fn sleep(duration: Duration) -> Self::Sleep;
}
