use std::{
    fmt::{self, Display, Formatter},
    ops::AddAssign,
    time::Duration,
};

use hdrhistogram::{sync::Recorder, Histogram, SyncHistogram};
use quanta::Clock;

pub struct Latency {
    name: String,
    hist: SyncHistogram<u32>,
}
#[derive(Clone)]
pub struct LocalLatency {
    recorder: Recorder<u32>,
    clock: Clock,
}

impl Latency {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            hist: Histogram::new(2).unwrap().into(),
        }
    }
}

impl Latency {
    pub fn refresh(&mut self) {
        self.hist.refresh();
    }
}

impl Into<SyncHistogram<u32>> for Latency {
    fn into(mut self) -> SyncHistogram<u32> {
        self.refresh();
        self.hist
    }
}

impl Latency {
    pub fn local(&self) -> LocalLatency {
        LocalLatency {
            recorder: self.hist.recorder(),
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

impl Display for Latency {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {:?} in total of {} samples",
            self.name,
            Duration::from_nanos(self.hist.mean() as _) * self.hist.len() as _,
            self.hist.len(),
        )?;
        for v in self
            .hist
            .iter_quantiles(1)
            .skip_while(|v| v.quantile() < 0.01)
        {
            // TODO continue

            writeln!(f)?;
            write!(
                f,
                "{:9?} | {:40} | {:4.1}th %-ile",
                Duration::from_nanos(v.value_iterated_to() as _),
                "*".repeat(
                    (v.count_since_last_iteration() as f64 * 40.0 / self.hist.len() as f64).ceil()
                        as usize
                ),
                v.quantile_iterated_to() * 100.0
            )?;
            if v.quantile() >= 0.99 {
                break;
            }
        }
        Ok(())
    }
}
