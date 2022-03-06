use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
};

use crate::transport::{Receiver, Transport};

pub struct Director<State, T: Transport> {
    state: Mutex<State>,
    transport: T::TxAgent,
    address: T::Address,
    submit: Arc<Submit<State, T>>,
}

pub struct StatefulContext<'a, State, T: Transport> {
    state: MutexGuard<'a, State>,
    pub transport: T::TxAgent,
    address: T::Address,
    pub submit: Arc<Submit<State, T>>,
}

pub struct StatelessContext<State, T: Transport> {
    pub transport: T::TxAgent,
    address: T::Address,
    pub submit: Arc<Submit<State, T>>,
}

pub struct Submit<State, T: Transport> {
    stateful_list: Mutex<Vec<StatefulTask<State, T>>>,
    stateless_list: Mutex<Vec<StatelessTask<State, T>>>,
}
type StatefulTask<S, T> = Box<dyn for<'a> FnOnce(&mut StatefulContext<'a, S, T>) + Send>;
type StatelessTask<S, T> = Box<dyn FnOnce(&StatelessContext<S, T>) + Send>;

impl<S, T: Transport> Director<S, T> {
    pub fn new(transport: T::TxAgent, address: T::Address, state: S) -> Self {
        Self {
            state: Mutex::new(state),
            transport,
            address,
            submit: Arc::new(Submit {
                stateful_list: Mutex::new(Vec::new()),
                stateless_list: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn with_state(&self, f: impl FnOnce(&StatefulContext<'_, S, T>)) {
        f(&StatefulContext {
            state: self.state.lock().unwrap(),
            transport: self.transport.clone(),
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

impl<S, T: Transport> Receiver<T> for StatelessContext<S, T> {
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

    pub fn stateless(&self, task: impl FnOnce(&StatelessContext<S, T>) + Send + 'static) {
        loop {
            if let Ok(stateless_list) = self.stateless_list.try_lock() {
                break stateless_list;
            }
        }
        .push(Box::new(task));
    }
}

enum Task<'a, S, T: Transport> {
    Stateful(StatefulTask<S, T>, StatefulContext<'a, S, T>),
    Stateless(StatelessTask<S, T>, StatelessContext<S, T>),
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
            drop(stateful_list);

            let mut stateless_list = loop {
                if let Ok(stateless_list) = self.submit.stateless_list.try_lock() {
                    break stateless_list;
                }
            };
            if let Some(task) = stateless_list.pop() {
                let context = StatelessContext {
                    transport: context.transport,
                    address: context.address,
                    submit: context.submit,
                    // state dropped
                };
                return Task::Stateless(task, context);
            }
            drop(stateless_list);

            // park when stealed nothing?
        }
    }

    fn steal_without_state(&self, context: StatelessContext<S, T>) -> Task<'_, S, T> {
        loop {
            let mut stateless_list = loop {
                if let Ok(stateless_list) = self.submit.stateless_list.try_lock() {
                    break stateless_list;
                }
            };
            if let Some(task) = stateless_list.pop() {
                return Task::Stateless(task, context);
            }
            drop(stateless_list);

            if let Ok(state) = self.state.try_lock() {
                let context = StatefulContext {
                    state,
                    transport: context.transport,
                    address: context.address,
                    submit: context.submit,
                };
                return self.steal_with_state(context);
            }

            // park when nothing to steal?
        }
    }

    pub fn run_worker(&self) {
        let context = StatelessContext {
            transport: self.transport.clone(),
            address: self.address.clone(),
            submit: self.submit.clone(),
        };
        let mut steal = self.steal_without_state(context);
        loop {
            match steal {
                Task::Stateful(task, mut context) => {
                    task(&mut context);
                    steal = self.steal_with_state(context);
                }
                Task::Stateless(task, context) => {
                    task(&context);
                    steal = self.steal_without_state(context);
                }
            }
        }
    }
}
