//! TODO: This file needs documentation

use super::{ProcFileReader};
use std::io::Result;
use std::time::Duration;


/// Mechanism for sampling measurements from /proc/uptime
pub struct UptimeSampler {
    /// Reader object for /proc/uptime
    reader: ProcFileReader,

    /// Sampled uptime data
    samples: UptimeData,
}
//
impl UptimeSampler {
    /// Create a new sampler of /proc/uptime
    pub fn new() -> Result<Self> {
        let reader = ProcFileReader::open("/proc/uptime")?;
        Ok(
            Self {
                reader,
                samples: UptimeData::new(),
            }
        )
    }

    /// Acquire a new sample of uptime data
    pub fn sample(&mut self) -> Result<()> {
        let samples = &mut self.samples;
        self.reader.sample(|file_contents: &str| {
            // Parse all known file contents (simple enough for /proc/uptime :))
            let mut numbers_iter = file_contents.split_whitespace();
            samples.wall_clock_uptime.push(
                super::parse_duration_secs(numbers_iter.next().unwrap())
            );
            samples.cpu_idle_time.push(
                super::parse_duration_secs(numbers_iter.next().unwrap())
            );

            // If this debug assert fails, the contents of the file have been
            // extended by a kernel revision, and the parser should be updated
            debug_assert!(numbers_iter.next() == None);
        })
    }

    // TODO: Add accessors to the inner uptime data
}


/// Data samples from /proc/uptime, in structure-of-array layout
struct UptimeData {
    /// Elapsed wall clock time since the system was started
    wall_clock_uptime: Vec<Duration>,

    /// Cumulative amount of time spent by all CPUs in the idle state
    cpu_idle_time: Vec<Duration>,
}
//
impl UptimeData {
    /// Create a new uptime data store
    fn new() -> Self {
        Self {
            wall_clock_uptime: Vec::new(),
            cpu_idle_time: Vec::new(),
        }
    }
}
