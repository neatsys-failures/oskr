use std::{
    mem::replace,
    ops::AddAssign,
    sync::{Arc, Mutex},
};

use hdrhistogram::Histogram;
use quanta::Clock;

#[derive(Clone)]
pub struct Latency(Arc<Mutex<Histogram<u64>>>);
pub struct SubLatency {
    total: Arc<Mutex<Histogram<u64>>>,
    latency: Histogram<u64>,
    clock: Clock,
}

impl Default for Latency {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Histogram::new(2).unwrap())))
    }
}

impl Into<Histogram<u64>> for Latency {
    fn into(self) -> Histogram<u64> {
        Arc::try_unwrap(self.0).unwrap().into_inner().unwrap()
    }
}

impl Latency {
    pub fn create_local(&self) -> SubLatency {
        SubLatency {
            total: self.0.clone(),
            latency: Histogram::new(2).unwrap(),
            clock: Clock::new(),
        }
    }
}

pub struct Measure(u64);
impl SubLatency {
    pub fn measure(&self) -> Measure {
        Measure(self.clock.start())
    }
}

impl AddAssign<Measure> for SubLatency {
    fn add_assign(&mut self, measure: Measure) {
        self.latency += self.clock.delta(measure.0, self.clock.end()).as_nanos() as u64;
    }
}

impl Drop for SubLatency {
    fn drop(&mut self) {
        *self.total.lock().unwrap() += replace(&mut self.latency, Histogram::new(0).unwrap());
    }
}
