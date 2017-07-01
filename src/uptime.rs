use super::ProcFileReader;
use std::io::Result;

/// A fractional amount of seconds
/// TODO: Use Duration instead
type Seconds = f64;


/// Data samples from /proc/uptime. This struct must be separated from the main
/// sampler object in order to clearly separate multiple mutable borrows.
/// TODO: Autogenerate this kind of vector sample struct from a scalar template
struct UptimeData {
    /// Elapsed wall clock time in seconds since the system was started
    wall_clock_uptime: Vec<Seconds>,

    /// Cumulative amount of seconds spent by all CPUs in the idle state
    cpu_idle_time: Vec<Seconds>,
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
            // Parse all known file contents (simple enough for uptime :))
            let mut numbers_iter = file_contents.split_whitespace();
            samples.wall_clock_uptime.push(numbers_iter.next().unwrap()
                                                       .parse().unwrap());
            samples.cpu_idle_time.push(numbers_iter.next().unwrap()
                                                   .parse().unwrap());

            // If this assert fails, the contents of the file have been extended
            // by a kernel revision, and the parser should be updated
            debug_assert!(numbers_iter.next() == None);
        })
    }

    // TODO: Add accessors to the inner uptime data
}
