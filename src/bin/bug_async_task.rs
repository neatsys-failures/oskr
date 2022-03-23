use std::{
    cell::RefCell,
    thread::{self, sleep},
    time::{Duration, Instant},
};

use async_task::{Runnable, Task};
use futures::{channel::mpsc, Future, StreamExt};

thread_local! {
    static RUNNABLE_LIST: RefCell<Vec<Runnable>> = RefCell::new(Vec::new());
}

fn poll_once() -> usize {
    let runnable_list: Vec<_> =
        RUNNABLE_LIST.with(|runnable_list| runnable_list.borrow_mut().drain(..).collect());
    let count = runnable_list.len();
    for runnable in runnable_list {
        let waker = runnable.waker();
        runnable.run();
        waker.wake();
    }
    count
}

fn spawn(task: impl Future<Output = ()> + Send + 'static) -> Task<()> {
    let (runnable, handle) = async_task::spawn(task, |runnable| {
        RUNNABLE_LIST.with(|runnable_list| runnable_list.borrow_mut().push(runnable));
    });
    runnable.schedule();
    handle
}

fn main() {
    let (tx, mut rx) = mpsc::unbounded();
    let handle = spawn(async move {
        loop {
            let _: () = rx.next().await.unwrap();
        }
    });

    thread::spawn(move || loop {
        sleep(Duration::from_millis(20));
        tx.unbounded_send(()).unwrap();
    });

    let start = Instant::now();
    while Instant::now() - start < Duration::from_millis(1 * 1000) {
        // assert_eq!(poll_once(), 1);
        poll_once();
    }
    assert_eq!(poll_once(), 1);
    drop(handle);
}
