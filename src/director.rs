use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
};

use crate::transport::{Receiver, Transport};

pub struct Director<State, T: Transport> {
    state: Mutex<State>,
    address: T::Address,
    submit: Arc<Submit<State, T>>,
}

pub struct StatefulContext<'a, State, T: Transport> {
    state: MutexGuard<'a, State>,
    address: T::Address,
    pub submit: Arc<Submit<State, T>>,
}

// stateless context

pub struct Submit<State, T: Transport> {
    stateful_list: Mutex<Vec<StatefulTask<State, T>>>,
    // stateless list
}
type StatefulTask<S, T> = Box<dyn for<'a> FnOnce(&mut StatefulContext<'a, S, T>) + Send>;
// stateless task

impl<S, T: Transport> Director<S, T> {
    pub fn new(address: T::Address, state: S) -> Self {
        Self {
            state: Mutex::new(state),
            address,
            submit: Arc::new(Submit {
                stateful_list: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn with_state(&self, f: impl FnOnce(&StatefulContext<'_, S, T>)) {
        f(&StatefulContext {
            state: self.state.lock().unwrap(),
            address: self.address.clone(),
            submit: self.submit.clone(),
        })
    }
}

impl<'a, S, T: Transport> Receiver<T> for StatefulContext<'a, S, T> {
    fn get_address(&self) -> &T::Address {
        &self.address
    }
}

impl<'a, S, T: Transport> Deref for StatefulContext<'a, S, T> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &*self.state
    }
}

impl<'a, S, T: Transport> DerefMut for StatefulContext<'a, S, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.state
    }
}

impl<S, T: Transport> Submit<S, T> {
    pub fn stateful(
        &self,
        task: impl for<'a> FnOnce(&mut StatefulContext<'a, S, T>) + Send + 'static,
    ) {
        loop {
            if let Ok(stateful_list) = self.stateful_list.try_lock() {
                break stateful_list;
            }
        }
        .push(Box::new(task));
    }
}

enum Task<'a, S, T: Transport> {
    Stateful(StatefulTask<S, T>, StatefulContext<'a, S, T>),
    // stateless
}

impl<S, T: Transport> Director<S, T> {
    fn steal_with_state<'a>(&'a self, context: StatefulContext<'a, S, T>) -> Task<'_, S, T> {
        loop {
            let mut stateful_list = loop {
                if let Ok(stateful_list) = self.submit.stateful_list.try_lock() {
                    break stateful_list;
                }
            };
            if let Some(task) = stateful_list.pop() {
                return Task::Stateful(task, context);
            }

            // try steal stateless task
        }
    }

    fn steal_without_state(&self) -> Task<'_, S, T> {
        loop {
            // try steal stateless task

            if let Ok(state) = self.state.try_lock() {
                let context = StatefulContext {
                    state,
                    // TODO reuse from stateless context
                    address: self.address.clone(),
                    submit: self.submit.clone(),
                };
                return self.steal_with_state(context);
            }
        }
    }

    pub fn run_worker(&self) {
        let mut steal = self.steal_without_state();
        loop {
            match steal {
                // TODO stateless task
                //
                Task::Stateful(task, mut context) => {
                    task(&mut context);
                    steal = self.steal_with_state(context);
                }
            }
        }
    }
}
