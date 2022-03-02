use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, park, Thread},
};

use crate::transport::{Receiver, Transport};

pub struct Executor<S> {
    state: Mutex<S>,
    stateful_list: Mutex<Vec<Box<dyn FnOnce(&mut S, &Self) + Send>>>,
    // stateless list
    park_list: Mutex<Vec<Thread>>,
    spin_park: AtomicU32,
}

pub enum Work<'a, S> {
    Stateful(Box<dyn FnOnce(&mut S, &Executor<S>)>, MutexGuard<'a, S>),
    Stateless(Box<dyn FnOnce(&Executor<S>)>),
}

impl<S> Executor<S> {
    pub fn new(state: S) -> Self {
        Self {
            state: Mutex::new(state),
            stateful_list: Mutex::new(Vec::new()),
            park_list: Mutex::new(Vec::new()),
            spin_park: AtomicU32::new(0),
        }
    }

    fn park(&self) {
        let id = self.spin_park.fetch_add(1, Ordering::SeqCst) + 1;
        loop {
            let park_id = self.spin_park.load(Ordering::SeqCst);
            if park_id < id {
                return;
            }
            if park_id > id {
                self.park_list.lock().unwrap().push(thread::current());
                thread::park();
                return;
            }
        }
    }

    fn unpark(&self) {
        let mut park_list = self.park_list.lock().unwrap();
        while let Some(thread) = park_list.pop() {
            thread.unpark();
        }
        self.spin_park.store(0, Ordering::SeqCst);
    }

    pub fn register_stateful<T: Transport>(
        self: &Arc<Self>,
        transport: &mut T,
        task: fn(&mut S, &Self, T::Address, T::RxBuffer),
    ) where
        S: Receiver<T> + Send + 'static,
    {
        transport.register(&*self.state.lock().unwrap(), {
            let executor = self.clone();
            move |remote, buffer| {
                executor
                    .submit_stateful(move |state, executor| task(state, executor, remote, buffer))
            }
        });
    }

    pub fn submit_stateful(&self, task: impl FnOnce(&mut S, &Self) + Send + 'static) {
        self.stateful_list.lock().unwrap().push(Box::new(task));
        self.unpark();
    }

    fn steal_with_state<'a>(&'a self, state: MutexGuard<'a, S>) -> Work<'a, S> {
        if let Some(task) = self.stateful_list.lock().unwrap().pop() {
            return Work::Stateful(task, state);
        }
        drop(state);
        self.steal_without_state()
    }

    fn steal_without_state(&self) -> Work<'_, S> {
        loop {
            // try steal stateless task
            if let Ok(state) = self.state.try_lock() {
                if let Some(task) = self.stateful_list.lock().unwrap().pop() {
                    return Work::Stateful(task, state);
                }
            }
            self.park_list.lock().unwrap().push(thread::current());
            self.park();
        }
    }

    pub fn worker_loop(&self) {
        let mut work = self.steal_without_state();
        loop {
            match work {
                Work::Stateful(task, mut state) => {
                    task(&mut *state, self);
                    work = self.steal_with_state(state);
                }
                Work::Stateless(task) => {
                    task(self);
                    work = self.steal_without_state();
                }
            }
        }
    }
}
