use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, Thread},
};

use crossbeam::{
    queue::{ArrayQueue, SegQueue},
    utils::Backoff,
};

use crate::framework::latency::{Latency, MeasureClock};

pub trait State {
    type Shared;
    fn shared(&self) -> Self::Shared;
}

pub struct Handle<S: State> {
    state: Mutex<S>,
    submit: Arc<Submit<S>>,
    metric: Metric,
}

struct Metric {
    stateful: Latency,
    stateless: Latency,
}

pub struct StatefulContext<'a, S: State> {
    state: MutexGuard<'a, S>,
    pub submit: Arc<Submit<S>>,
}

pub struct StatelessContext<S: State> {
    shared: S::Shared,
    pub submit: Arc<Submit<S>>,
}

pub struct Submit<S: State> {
    stateful_list: SegQueue<StatefulTask<S>>,
    stateless_list: SegQueue<StatelessTask<S>>,
    will_park_list: ArrayQueue<Thread>,
}
type StatefulTask<S> = Box<dyn for<'a> FnOnce(&mut StatefulContext<'a, S>) + Send>;
type StatelessTask<S> = Box<dyn FnOnce(&StatelessContext<S>) + Send>;

impl<S: State> From<S> for Handle<S> {
    fn from(state: S) -> Self {
        Self {
            state: Mutex::new(state),
            submit: Arc::new(Submit {
                stateful_list: SegQueue::new(),
                stateless_list: SegQueue::new(),
                will_park_list: ArrayQueue::new(64), // configurable?
            }),
            metric: Metric {
                stateful: Latency::new("stateful"),
                stateless: Latency::new("stateless"),
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
            shared: self.state.lock().unwrap().shared(),
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
        self.stateful_list.push(Box::new(task));
        self.unpark_one();
    }

    pub fn stateless(&self, task: impl FnOnce(&StatelessContext<S>) + Send + 'static) {
        self.stateless_list.push(Box::new(task));
        self.unpark_one();
    }

    #[allow(dead_code)] // currently no plan to park, use Backoff instead
                        // however it is good to save it for the future
    fn park(&self) {
        thread::park();
        self.will_park_list.push(thread::current()).unwrap();
    }

    fn unpark_one(&self) {
        if let Some(thread) = self.will_park_list.pop() {
            thread.unpark();
        }
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
        let backoff = Backoff::new();
        while !shutdown() {
            if let Some(task) = self.submit.stateful_list.pop() {
                return Task::Stateful(task, context);
            }

            if let Some(task) = self.submit.stateless_list.pop() {
                let context = StatelessContext {
                    shared: context.shared(),
                    submit: context.submit,
                };
                // state dropped
                return Task::Stateless(task, context);
            }

            backoff.snooze();
        }
        Task::Shutdown
    }

    fn steal_without_state(
        &self,
        context: StatelessContext<S>,
        shutdown: &mut impl FnMut() -> bool,
    ) -> Task<'_, S> {
        let backoff = Backoff::new();
        while !shutdown() {
            if let Some(task) = self.submit.stateless_list.pop() {
                return Task::Stateless(task, context);
            }

            if let Ok(state) = self.state.try_lock() {
                let context = StatefulContext {
                    state,
                    submit: context.submit,
                };
                return self.steal_with_state(context, shutdown);
            }

            backoff.snooze();
        }
        Task::Shutdown
    }

    pub fn run_worker(&self, mut shutdown: impl FnMut() -> bool) {
        let context = StatelessContext {
            shared: self.state.lock().unwrap().shared(),
            submit: self.submit.clone(),
        };

        let mut stateful_latency = self.metric.stateful.local();
        let mut stateless_latency = self.metric.stateless.local();
        let clock = MeasureClock::default();

        let mut steal = self.steal_without_state(context, &mut shutdown);
        loop {
            match steal {
                Task::Stateful(task, mut context) => {
                    let measure = clock.measure();
                    task(&mut context);
                    stateful_latency += measure;
                    steal = self.steal_with_state(context, &mut shutdown);
                }
                Task::Stateless(task, context) => {
                    let measure = clock.measure();
                    task(&context);
                    stateless_latency += measure;
                    steal = self.steal_without_state(context, &mut shutdown);
                }
                Task::Shutdown => return,
            }
        }
    }

    pub fn run_stateless_worker(&self, mut shutdown: impl FnMut() -> bool) {
        let context = StatelessContext {
            shared: self.state.lock().unwrap().shared(),
            submit: self.submit.clone(),
        };
        let mut stateless_latency = self.metric.stateless.local();
        let clock = MeasureClock::default();
        while !shutdown() {
            if let Some(task) = self.submit.stateless_list.pop() {
                let measure = clock.measure();
                task(&context);
                stateless_latency += measure;
            }
        }
    }

    pub fn unpark_all(&self) {
        while let Some(thread) = self.submit.will_park_list.pop() {
            thread.unpark();
        }
    }
}

impl<S: State> Drop for Handle<S> {
    fn drop(&mut self) {
        self.metric.stateful.refresh();
        println!("{}", self.metric.stateful);
        self.metric.stateless.refresh();
        println!("{}", self.metric.stateless);
    }
}
