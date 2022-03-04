use std::sync::{Arc, Mutex, MutexGuard};

use crate::executor;

pub struct Submit<State> {
    stateful_list: Mutex<StatefulList<State>>,
    // stateless list
}
type StatefulList<State> = Vec<Box<dyn FnOnce(&mut State) + Send>>;

impl<S> Default for Submit<S> {
    fn default() -> Self {
        Self {
            stateful_list: Mutex::new(Vec::new()),
        }
    }
}

enum Task<'a, State> {
    Stateful(Box<dyn FnOnce(&mut State)>, MutexGuard<'a, State>),
    Stateless(Box<dyn FnOnce()>),
}

impl<S> executor::Submit<S> for Submit<S> {
    fn stateful_boxed(&self, task: Box<dyn FnOnce(&mut S) + Send>) {
        loop {
            if let Ok(mut stateful_list) = self.stateful_list.try_lock() {
                stateful_list.push(task);
                return;
            }
        }
    }

    fn stateless_boxed(&self, _: Box<dyn FnOnce() + Send>) {
        todo!()
    }
}

pub struct Driver<State> {
    state: Mutex<State>,
    submit: Arc<Submit<State>>,
}

impl<S> Driver<S> {
    pub fn new(state: S, submit: Arc<Submit<S>>) -> Self {
        Self {
            state: Mutex::new(state),
            submit,
        }
    }

    fn steal_with_state<'a>(&'a self, state: MutexGuard<'a, S>) -> Task<'_, S> {
        loop {
            let mut stateful_list = loop {
                if let Ok(stateful_list) = self.submit.stateful_list.try_lock() {
                    break stateful_list;
                }
            };
            if let Some(task) = stateful_list.pop() {
                return Task::Stateful(task, state);
            }
            // try steal stateless task
        }
    }

    fn steal_without_state(&self) -> Task<'_, S> {
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
                Task::Stateful(task, mut state) => {
                    task(&mut *state);
                    work = self.steal_with_state(state);
                }
                Task::Stateless(task) => {
                    task();
                    work = self.steal_without_state();
                }
            }
        }
    }

    pub fn run_stateful_worker(&self) {
        let mut state = self.state.lock().unwrap();
        loop {
            let mut stateful_list = loop {
                if let Ok(stateful_list) = self.submit.stateful_list.try_lock() {
                    break stateful_list;
                }
            };
            if let Some(task) = stateful_list.pop() {
                task(&mut *state);
            } else {
                // park
            }
        }
    }

    // run stateless worker
}
