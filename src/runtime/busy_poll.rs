use std::{
    cell::RefCell,
    collections::HashMap,
    pin::Pin,
    sync::atomic::{AtomicU32, Ordering},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::{channel::oneshot, future::BoxFuture, task::noop_waker_ref, Future};
use quanta::Clock;

use crate::facade;

pub struct AsyncEcosystem;
impl AsyncEcosystem {
    thread_local! {
        static TASK_TABLE: RefCell<HashMap<u32, BoxFuture<'static, ()>>> = RefCell::new(HashMap::new());
        static TASK_NUMBER: AtomicU32 = AtomicU32::new(0);
    }

    fn poll_once() {
        let snapshot_list: Vec<_> =
            Self::TASK_TABLE.with(|task_table| task_table.borrow().keys().cloned().collect());
        for id in snapshot_list {
            if let Some(mut task) =
                Self::TASK_TABLE.with(|task_table| task_table.borrow_mut().remove(&id))
            {
                let poll = Pin::new(&mut task).poll(&mut Context::from_waker(noop_waker_ref()));
                if poll.is_pending() {
                    Self::TASK_TABLE
                        .with(move |task_table| task_table.borrow_mut().insert(id, task));
                }
            }
        }
    }

    pub fn poll(duration: Duration) {
        let clock = Clock::new();
        let start = clock.start();
        while clock.delta(start, clock.end()) < duration {
            Self::poll_once();
        }
    }

    pub fn poll_all() {
        while Self::TASK_TABLE.with(|task_list| task_list.borrow().len()) > 0 {
            Self::poll_once();
        }
    }

    fn cancel(id: u32) {
        Self::TASK_TABLE.with(|task_table| task_table.borrow_mut().remove(&id));
    }
}

pub struct Sleep(quanta::Instant);
impl Future for Sleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if quanta::Instant::now() >= self.0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[derive(Debug)]
pub struct JoinHandle<T>(u32, oneshot::Receiver<T>);
impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        AsyncEcosystem::cancel(self.0);
    }
}
impl<T> Future for JoinHandle<T> {
    type Output = T;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.1).poll(cx).map(Result::unwrap)
    }
}

impl<T: Send + 'static> facade::AsyncEcosystem<T> for AsyncEcosystem {
    type JoinHandle = JoinHandle<T>;
    type Sleep = Sleep;

    fn spawn(task: impl Future<Output = T> + Send + 'static) -> Self::JoinHandle {
        let id =
            Self::TASK_NUMBER.with(|task_number| task_number.fetch_add(1, Ordering::SeqCst)) + 1;
        let (tx, rx) = oneshot::channel();
        let task =
            Box::pin(async move { tx.send(task.await).map_err(|_| unreachable!()).unwrap() });

        Self::TASK_TABLE.with(|task_table| task_table.borrow_mut().insert(id, task));
        JoinHandle(id, rx)
    }

    fn cancel(handle: Self::JoinHandle) {
        drop(handle);
    }

    fn sleep_until(instant: Instant) -> Self::Sleep {
        // so shit...
        Sleep(quanta::Instant::now() + (instant - Instant::now()))
    }
}
