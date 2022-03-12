use std::{
    mem::take,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use hdrhistogram::Histogram;

use crate::latency::Latency;

pub trait State {
    type Shared: Clone + Send;
    fn shared(&self) -> Self::Shared;
}

pub struct Handle<S: State> {
    state: Mutex<S>,
    shared: S::Shared,
    submit: Arc<Submit<S>>,
    latency: Latency,
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
            latency: Latency::default(),
        }
    }
}

impl<S: State> Handle<S> {
    pub fn with_stateful(&self, f: impl FnOnce(&mut StatefulContext<'_, S>)) {
        f(&mut StatefulContext {
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
    Shutdown,
}

impl<S: State> Handle<S> {
    fn steal_with_state<'a>(
        &'a self,
        context: StatefulContext<'a, S>,
        shutdown: &mut impl FnMut() -> bool,
    ) -> Task<'_, S> {
        while !shutdown() {
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
        Task::Shutdown
    }

    fn steal_without_state(
        &self,
        context: StatelessContext<S>,
        shutdown: &mut impl FnMut() -> bool,
    ) -> Task<'_, S> {
        while !shutdown() {
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
                return self.steal_with_state(context, shutdown);
            }

            // park when nothing to steal?
        }
        Task::Shutdown
    }

    pub fn run_worker(&self, mut shutdown: impl FnMut() -> bool) {
        let context = StatelessContext {
            shared: self.shared.clone(),
            submit: self.submit.clone(),
        };
        let mut latency = self.latency.create_local();

        let mut steal = self.steal_without_state(context, &mut shutdown);
        loop {
            match steal {
                Task::Stateful(task, mut context) => {
                    let measure = latency.measure();
                    task(&mut context);
                    latency += measure;
                    steal = self.steal_with_state(context, &mut shutdown);
                }
                Task::Stateless(task, context) => {
                    // TODO measure latency
                    task(&context);
                    steal = self.steal_without_state(context, &mut shutdown);
                }
                Task::Shutdown => return,
            }
        }
    }
}

impl<S: State> Drop for Handle<S> {
    fn drop(&mut self) {
        let hist: Histogram<_> = take(&mut self.latency).into();
        println!(
            "stateful: {:?}, {} samples",
            Duration::from_nanos(hist.mean() as _) * hist.len() as _,
            hist.len(),
        );
    }
}
