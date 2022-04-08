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

#[derive(Clone, Default)]
pub struct MeasureClock(Clock);

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

impl From<Latency> for SyncHistogram<u32> {
    fn from(mut latency: Latency) -> Self {
        latency.hist.refresh();
        latency.hist
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
impl MeasureClock {
    pub fn measure(&self) -> Measure {
        Measure(self.0.start())
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
        for v in self.hist.iter_quantiles(1).skip(1) {
            writeln!(f)?;
            if v.count_since_last_iteration() == 0 {
                write!(f, "...")?;
                continue;
            }
            write!(
                f,
                "{:10?} | {:40} | {:4.1}th %-ile",
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
