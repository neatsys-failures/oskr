use std::{
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, Thread},
};

use crate::transport::{Receiver, Transport};

pub struct Executor<S> {
    state: Mutex<S>,
    stateful_list: Mutex<Vec<Box<dyn FnOnce(&mut S, &Self) + Send>>>,
    // stateless list
    park_list: Mutex<Vec<Thread>>,
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
        }
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
        if let Some(thread) = self.park_list.lock().unwrap().pop() {
            thread.unpark();
        }
    }

    fn steal_with_state<'a>(&self, state: MutexGuard<'a, S>) -> Work<'a, S> {
        loop {
            if let Some(task) = self.stateful_list.lock().unwrap().pop() {
                return Work::Stateful(task, state);
            }
            // try steal stateless task
            self.park_list.lock().unwrap().push(thread::current());
            thread::park();
        }
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
            thread::park();
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
