use std::ops::AddAssign;

use hdrhistogram::{sync::Recorder, Histogram, SyncHistogram};
use quanta::Clock;

pub struct Latency(SyncHistogram<u64>);
#[derive(Clone)]
pub struct LocalLatency {
    recorder: Recorder<u64>,
    clock: Clock,
}

impl Default for Latency {
    fn default() -> Self {
        Self(Histogram::new(2).unwrap().into())
    }
}

impl Into<SyncHistogram<u64>> for Latency {
    fn into(self) -> SyncHistogram<u64> {
        let mut hist = self.0;
        hist.refresh();
        hist
    }
}

impl Latency {
    pub fn local(&self) -> LocalLatency {
        LocalLatency {
            recorder: self.0.recorder(),
            clock: Clock::new(),
        }
    }
}

pub struct Measure(u64);
impl LocalLatency {
    pub fn measure(&self) -> Measure {
        Measure(self.clock.start())
    }
}

impl AddAssign<Measure> for LocalLatency {
    fn add_assign(&mut self, measure: Measure) {
        self.recorder += self.clock.delta(measure.0, self.clock.end()).as_nanos() as u64;
    }
}
