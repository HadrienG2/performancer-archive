//! This module contains a sampling parser for /proc/uptime

use ::{parsers, ProcFileReader};
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
                parsers::parse_duration_secs(numbers_iter.next().unwrap())
            );
            samples.cpu_idle_time.push(
                parsers::parse_duration_secs(numbers_iter.next().unwrap())
            );

            // If this debug assert fails, the contents of the file have been
            // extended by a kernel revision, and the parser should be updated
            debug_assert!(numbers_iter.next() == None);
        })
    }

    // TODO: Add accessors to the inner uptime data + associated tests
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


/// These are the unit tests for this module
#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use super::UptimeSampler;
    
    /// Check that no samples are initially present
    #[test]
    fn new_sampler() {
        let uptime = UptimeSampler::new().unwrap();
        assert_eq!(uptime.samples.wall_clock_uptime.len(), 0);
        assert_eq!(uptime.samples.cpu_idle_time.len(), 0);
    }

    /// Test that basic sampling works as expected
    #[test]
    fn basic_sampling() {
        // Create an uptime sampler
        let mut uptime = UptimeSampler::new().unwrap();

        // Acquire a first sample
        uptime.sample().unwrap();
        assert_eq!(uptime.samples.wall_clock_uptime.len(), 1);
        assert_eq!(uptime.samples.cpu_idle_time.len(), 1);

        // Wait a bit
        thread::sleep(Duration::from_millis(50));

        // Acquire another sample
        uptime.sample().unwrap();
        assert_eq!(uptime.samples.wall_clock_uptime.len(), 2);
        assert_eq!(uptime.samples.cpu_idle_time.len(), 2);

        // The uptime and idle time should have increased
        assert!(uptime.samples.wall_clock_uptime[1] >
                    uptime.samples.wall_clock_uptime[0]);
        assert!(uptime.samples.cpu_idle_time[1] >
                    uptime.samples.cpu_idle_time[0]);
    }
}



/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    use testbench;
    use super::UptimeSampler;

    /// Benchmark for the full uptime sampling overhead
    #[test]
    #[ignore]
    fn uptime_sampling_overhead() {
        let mut uptime = UptimeSampler::new().unwrap();
        testbench::benchmark(3_000_000, || {
            uptime.sample().unwrap();
        });
    }
}
