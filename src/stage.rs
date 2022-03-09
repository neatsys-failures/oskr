use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
};

pub trait State {
    type Shared: Clone + Send;
    fn shared(&self) -> Self::Shared;
}

pub struct Handle<S: State> {
    state: Mutex<S>,
    shared: S::Shared,
    submit: Arc<Submit<S>>,
}

pub struct StatefulContext<'a, S: State> {
    state: MutexGuard<'a, S>,
    pub submit: Arc<Submit<S>>,
}

pub struct StatelessContext<S: State> {
    shared: S::Shared,
    pub submit: Arc<Submit<S>>,
}

impl<S: State> Clone for StatelessContext<S> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            submit: self.submit.clone(),
        }
    }
}

pub struct Submit<S: State> {
    stateful_list: Mutex<Vec<StatefulTask<S>>>,
    stateless_list: Mutex<Vec<StatelessTask<S>>>,
}
type StatefulTask<S> = Box<dyn for<'a> FnOnce(&mut StatefulContext<'a, S>) + Send>;
type StatelessTask<S> = Box<dyn FnOnce(&StatelessContext<S>) + Send>;

impl<S: State> From<S> for Handle<S> {
    fn from(state: S) -> Self {
        Self {
            shared: state.shared(),
            state: Mutex::new(state),
            submit: Arc::new(Submit {
                stateful_list: Mutex::new(Vec::new()),
                stateless_list: Mutex::new(Vec::new()),
            }),
        }
    }
}

impl<S: State> Handle<S> {
    pub fn with_stateful(&self, f: impl FnOnce(&StatefulContext<'_, S>)) {
        f(&StatefulContext {
            state: self.state.lock().unwrap(),
            submit: self.submit.clone(),
        })
    }

    pub fn with_stateless(&self, f: impl FnOnce(&StatelessContext<S>)) {
        f(&StatelessContext {
            shared: self.shared.clone(),
            submit: self.submit.clone(),
        })
    }
}

impl<'a, S: State> Deref for StatefulContext<'a, S> {
    type Target = S;
    fn deref(&self) -> &Self::Target {
        &*self.state
    }
}

impl<'a, S: State> DerefMut for StatefulContext<'a, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.state
    }
}

impl<S: State> Deref for StatelessContext<S> {
    type Target = S::Shared;
    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

impl<S: State> Submit<S> {
    pub fn stateful(
        &self,
        task: impl for<'a> FnOnce(&mut StatefulContext<'a, S>) + Send + 'static,
    ) {
        loop {
            if let Ok(stateful_list) = self.stateful_list.try_lock() {
                break stateful_list;
            }
        }
        .push(Box::new(task));
    }

    pub fn stateless(&self, task: impl FnOnce(&StatelessContext<S>) + Send + 'static) {
        loop {
            if let Ok(stateless_list) = self.stateless_list.try_lock() {
                break stateless_list;
            }
        }
        .push(Box::new(task));
    }
}

enum Task<'a, S: State> {
    Stateful(StatefulTask<S>, StatefulContext<'a, S>),
    Stateless(StatelessTask<S>, StatelessContext<S>),
}

impl<S: State> Handle<S> {
    fn steal_with_state<'a>(&'a self, context: StatefulContext<'a, S>) -> Task<'_, S> {
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
                    shared: self.shared.clone(),
                    submit: context.submit,
                };
                // state dropped
                return Task::Stateless(task, context);
            }
            drop(stateless_list);

            // park when stealed nothing?
        }
    }

    fn steal_without_state(&self, context: StatelessContext<S>) -> Task<'_, S> {
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
                    submit: context.submit,
                };
                return self.steal_with_state(context);
            }

            // park when nothing to steal?
        }
    }

    pub fn run_worker(&self) {
        let context = StatelessContext {
            shared: self.shared.clone(),
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
