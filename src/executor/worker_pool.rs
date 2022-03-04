use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    executor,
    transport::{Receiver, Transport},
};

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

impl<S> From<S> for Executor<S> {
    fn from(state: S) -> Self {
        Self {
            state: Mutex::new(state),
            stateful_list: Mutex::new(Vec::new()),
        }
    }
}

impl<S> executor::Executor<S> for Executor<S> {
    fn register_stateful<T: Transport>(
        self: &Arc<Self>,
        transport: &mut T,
        task: impl Fn(&mut S, &Self, T::Address, T::RxBuffer) + Send + Copy + 'static,
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

    fn submit_stateful(&self, task: impl FnOnce(&mut S, &Self) + Send + 'static) {
        loop {
            if let Ok(mut stateful_list) = self.stateful_list.try_lock() {
                stateful_list.push(Box::new(task));
                return;
            }
        }
    }

    fn submit_stateless(&self, _: impl FnOnce(&Self) + Send + 'static) {
        todo!()
    }
}

impl<S> Executor<S> {
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

    pub fn run_worker(&self) {
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

    pub fn run_stateful_worker(&self) {
        let mut state = self.state.lock().unwrap();
        loop {
            let mut stateful_list = loop {
                if let Ok(stateful_list) = self.stateful_list.try_lock() {
                    break stateful_list;
                }
            };
            if let Some(task) = stateful_list.pop() {
                task(&mut *state, self);
            } else {
                // park
            }
        }
    }

    // run stateless worker
}
