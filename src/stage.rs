use std::{
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

use crate::latency::Latency;

pub trait State {
    type Shared: Clone + Send;
    fn shared(&self) -> Self::Shared;
}

pub struct Handle<S: State> {
    state: Mutex<S>,
    shared: S::Shared,
    submit: Arc<Submit<S>>,
    metric: Metric,
}

struct Metric {
    stateful: Latency,
    stateless: Latency,
    park: Latency,
}

struct SubmitMetric {
    //
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
    n_worker: AtomicU32,
    park_token: AtomicU32,
    metric: SubmitMetric,
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
                n_worker: AtomicU32::new(0),
                park_token: AtomicU32::new(0),
                metric: SubmitMetric {},
            }),
            metric: Metric {
                stateful: Latency::new("stateful"),
                stateless: Latency::new("stateless"),
                park: Latency::new("park"),
            },
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
        self.unpark_one();
    }

    pub fn stateless(&self, task: impl FnOnce(&StatelessContext<S>) + Send + 'static) {
        loop {
            if let Ok(stateless_list) = self.stateless_list.try_lock() {
                break stateless_list;
            }
        }
        .push(Box::new(task));
        self.unpark_one();
    }

    fn park(&self) {
        let token = self.park_token.fetch_sub(1, Ordering::SeqCst);
        if token > 0 {
            return;
        }
        while self.park_token.load(Ordering::SeqCst) < token {}
    }

    fn unpark_one(&self) {
        let n_worker = self.n_worker.load(Ordering::SeqCst);
        assert_ne!(n_worker, 0);
        self.park_token
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |t| {
                Some((t + 1).min(n_worker))
            })
            .unwrap();
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

            // park
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
        }
        Task::Shutdown
    }

    pub fn set_worker_count(&mut self, n_worker: u32) {
        self.submit.n_worker.store(n_worker, Ordering::SeqCst);
    }

    pub fn run_worker(&self, mut shutdown: impl FnMut() -> bool) {
        let context = StatelessContext {
            shared: self.shared.clone(),
            submit: self.submit.clone(),
        };

        let mut stateful_latency = self.metric.stateful.local();
        let mut stateless_latency = self.metric.stateless.local();

        let mut steal = self.steal_without_state(context, &mut shutdown);
        loop {
            match steal {
                Task::Stateful(task, mut context) => {
                    let measure = stateful_latency.measure();
                    task(&mut context);
                    stateful_latency += measure;
                    steal = self.steal_with_state(context, &mut shutdown);
                }
                Task::Stateless(task, context) => {
                    let measure = stateless_latency.measure();
                    task(&context);
                    stateless_latency += measure;
                    steal = self.steal_without_state(context, &mut shutdown);
                }
                Task::Shutdown => return,
            }
        }
    }

    pub fn unpark_all(&self) {
        for _ in 0..self.submit.n_worker.load(Ordering::SeqCst) {
            self.submit.unpark_one();
        }
    }
}

impl<S: State> Drop for Handle<S> {
    fn drop(&mut self) {
        self.metric.stateful.refresh();
        println!("{}", self.metric.stateful);
        self.metric.stateless.refresh();
        println!("{}", self.metric.stateless);
        self.metric.park.refresh();
        println!("{}", self.metric.park);
    }
}
