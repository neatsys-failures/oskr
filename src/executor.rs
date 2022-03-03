use std::sync::{Arc, Mutex, MutexGuard};

use crate::transport::{Receiver, Transport};

pub struct Executor<S> {
    state: Mutex<S>,
    stateful_list: StatefulList<Self, S>,
    // stateless list
}
type StatefulList<E, S> = Mutex<Vec<Box<dyn FnOnce(&mut S, &E) + Send>>>;

pub enum Work<'a, S> {
    Stateful(Box<dyn FnOnce(&mut S, &Executor<S>)>, MutexGuard<'a, S>),
    Stateless(Box<dyn FnOnce(&Executor<S>)>),
}

impl<S> Executor<S> {
    pub fn new(state: S) -> Self {
        Self {
            state: Mutex::new(state),
            stateful_list: Mutex::new(Vec::new()),
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
        loop {
            if let Ok(mut stateful_list) = self.stateful_list.try_lock() {
                stateful_list.push(Box::new(task));
                return;
            }
        }
    }

    fn steal_with_state<'a>(&'a self, state: MutexGuard<'a, S>) -> Work<'_, S> {
        loop {
            let mut stateful_list = loop {
                if let Ok(stateful_list) = self.stateful_list.try_lock() {
                    break stateful_list;
                }
            };
            if let Some(task) = stateful_list.pop() {
                return Work::Stateful(task, state);
            }
            // try steal stateless task
        }
    }

    fn steal_without_state(&self) -> Work<'_, S> {
        loop {
            // try steal stateless task
            if let Ok(state) = self.state.try_lock() {
                return self.steal_with_state(state);
            }
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
