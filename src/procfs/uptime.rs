//! This module contains a sampling parser for /proc/uptime

use ::procfs;
use ::reader::ProcFileReader;
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
        let samples = UptimeData::new();
        Ok(
            Self {
                reader,
                samples,
            }
        )
    }

    /// Acquire a new sample of uptime data
    pub fn sample(&mut self) -> Result<()> {
        let samples = &mut self.samples;
        self.reader.sample(|file_contents: &str| samples.push(file_contents))
    }
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

    /// Parse a sample from /proc/uptime and add it to the internal data store
    fn push(&mut self, file_contents: &str) {
        // Load machine uptime and idle time
        let mut numbers_iter = file_contents.split_whitespace();
        self.wall_clock_uptime.push(
            procfs::parse_duration_secs(
                numbers_iter.next().expect("Machine uptime is missing")
            )
        );
        self.cpu_idle_time.push(
            procfs::parse_duration_secs(
                numbers_iter.next().expect("Machine idle time is missing")
            )
        );

        // If this debug assert fails, the contents of the file have been
        // extended by a kernel revision, and the parser should be updated
        debug_assert!(numbers_iter.next().is_none(),
                      "Unsupported entry found in /proc/uptime");
    }

    /// Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        let length = self.wall_clock_uptime.len();
        debug_assert_eq!(length, self.cpu_idle_time.len());
        length
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use super::{UptimeData, UptimeSampler};

    /// Check that creating an uptime data store works
    #[test]
    fn init_uptime_data() {
        let data = UptimeData::new();
        assert_eq!(data.wall_clock_uptime.len(), 0);
        assert_eq!(data.cpu_idle_time.len(), 0);
        assert_eq!(data.len(), 0);
    }

    /// Check that parsing uptime data works
    #[test]
    fn parse_uptime_data() {
        let mut data = UptimeData::new();
        data.push("13.52 50.34");
        assert_eq!(data.wall_clock_uptime,
                   vec![Duration::new(13, 520_000_000)]);
        assert_eq!(data.cpu_idle_time,
                   vec![Duration::new(50, 340_000_000)]);
        assert_eq!(data.len(), 1);
    }
    
    /// Check that initializing a sampler works
    #[test]
    fn init_sampler() {
        let uptime = UptimeSampler::new()
                                   .expect("Failed to create uptime sampler");
        assert_eq!(uptime.samples.len(), 0);
    }

    /// Test that basic sampling works as expected
    #[test]
    fn basic_sampling() {
        // Create an uptime sampler
        let mut uptime =
            UptimeSampler::new().expect("Failed to create uptime sampler");

        // Acquire a first sample
        uptime.sample().expect("Failed to sample uptime once");
        assert_eq!(uptime.samples.len(), 1);

        // Wait a bit
        thread::sleep(Duration::from_millis(50));

        // Acquire another sample
        uptime.sample().expect("Failed to sample uptime twice");
        assert_eq!(uptime.samples.len(), 2);

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
    use ::reader::ProcFileReader;
    use super::UptimeSampler;
    use testbench;

    /// Benchmark for the raw uptime readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader =
            ProcFileReader::open("/proc/uptime")
                           .expect("Failed to access uptime");
        testbench::benchmark(3_000_000, || {
            reader.sample(|_| {}).expect("Failed to sample uptime");
        });
    }

    /// Benchmark for the full uptime sampling overhead
    #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut uptime =
            UptimeSampler::new().expect("Failed to create uptime sampler");
        testbench::benchmark(3_000_000, || {
            uptime.sample().expect("Failed to sample uptime");
        });
    }
}
