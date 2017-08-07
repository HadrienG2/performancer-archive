//! This module contains facilities for parsing and storing the data contained
//! in the "cpu" sections of /proc/stat.

use ::splitter::SplitColumns;
use libc;
use std::time::Duration;
use super::StatDataStore;


/// The amount of CPU time that the system spent in various states
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CPUStatData {
    /// Time spent in user mode
    user_time: Vec<Duration>,

    /// Time spent in user mode with low priority (nice)
    nice_time: Vec<Duration>,

    /// Time spent in system (aka kernel) mode
    system_time: Vec<Duration>,

    /// Time spent in the idle task (should match second entry in /proc/uptime)
    idle_time: Vec<Duration>,

    /// Time spent waiting for IO to complete (since Linux 2.5.41)
    io_wait_time: Option<Vec<Duration>>,

    /// Time spent servicing hardware interrupts (since Linux 2.6.0-test4)
    irq_time: Option<Vec<Duration>>,

    /// Time spent servicing softirqs (since Linux 2.6.0-test4)
    softirq_time: Option<Vec<Duration>>,

    /// "Stolen" time spent in other operating systems when running in a
    /// virtualized environment (since Linux 2.6.11)
    stolen_time: Option<Vec<Duration>>,

    /// Time spent running a virtual CPU for guest OSs (since Linux 2.6.24)
    guest_time: Option<Vec<Duration>>,

    /// Time spent running a niced guest (see above, since Linux 2.6.33)
    guest_nice_time: Option<Vec<Duration>>,
}
//
impl CPUStatData {
    /// Create new CPU statistics
    pub fn new(num_timers: u8) -> Self {
        // Check if we know about all CPU timers
        debug_assert!(num_timers >= 4, "Some expected CPU timers are missing");
        debug_assert!(num_timers <= 10, "Unknown CPU timers detected");

        // Prepare to conditionally create a certain amount of timing Vecs
        let mut created_vecs = 4;
        let mut conditional_vec = || -> Option<Vec<Duration>> {
            created_vecs += 1;
            if created_vecs <= num_timers {
                Some(Vec::new())
            } else {
                None
            }
        };

        // Create the statistics
        Self {
            // These CPU timers should always be there
            user_time: Vec::new(),
            nice_time: Vec::new(),
            system_time: Vec::new(),
            idle_time: Vec::new(),

            // These may or may not be there depending on kernel version
            io_wait_time: conditional_vec(),
            irq_time: conditional_vec(),
            softirq_time: conditional_vec(),
            stolen_time: conditional_vec(),
            guest_time: conditional_vec(),
            guest_nice_time: conditional_vec(),
        }
    }
}
//
impl StatDataStore for CPUStatData {
    /// Parse CPU statistics and add them to the internal data store
    fn push(&mut self, mut stats: SplitColumns) {
        // This scope is needed to please rustc's current borrow checker
        {
            // This is how we parse the next duration from the input (if any)
            let ticks_per_sec = *TICKS_PER_SEC;
            let nanosecs_per_tick = *NANOSECS_PER_TICK;
            let mut next_stat = || -> Option<Duration> {
                stats.next().map(|str_duration| -> Duration {
                    let ticks: u64 =
                        str_duration.parse()
                                    .expect("Failed to parse CPU tick counter");
                    let secs = ticks / ticks_per_sec;
                    let nanosecs = (ticks % ticks_per_sec) * nanosecs_per_tick;
                    Duration::new(secs, nanosecs as u32)
                })
            };

            // Load the "mandatory" CPU statistics
            self.user_time.push(next_stat().expect("User time missing"));
            self.nice_time.push(next_stat().expect("Nice time missing"));
            self.system_time.push(next_stat().expect("System time missing"));
            self.idle_time.push(next_stat().expect("Idle time missing"));

            // Load the "optional" CPU statistics
            let mut load_optional_stat = |stat: &mut Option<Vec<Duration>>| {
                if let Some(ref mut vec) = *stat {
                    vec.push(next_stat().expect("A CPU timer went missing"));
                }
            };
            load_optional_stat(&mut self.io_wait_time);
            load_optional_stat(&mut self.irq_time);
            load_optional_stat(&mut self.softirq_time);
            load_optional_stat(&mut self.stolen_time);
            load_optional_stat(&mut self.guest_time);
            load_optional_stat(&mut self.guest_nice_time);
        }

        // At this point, we should have loaded all available stats
        debug_assert!(stats.next().is_none(),
                      "A CPU timer appeared out of nowhere");
    }

    /// Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        // Check the mandatory CPU timers
        let length = self.user_time.len();
        debug_assert_eq!(length, self.nice_time.len());
        debug_assert_eq!(length, self.system_time.len());
        debug_assert_eq!(length, self.idle_time.len());

        // Check the length of the optional CPU timers for consistency
        let optional_len = |op: &Option<Vec<Duration>>| -> usize {
            op.as_ref().map_or(length, |vec| vec.len())
        };
        debug_assert_eq!(length, optional_len(&self.io_wait_time));
        debug_assert_eq!(length, optional_len(&self.irq_time));
        debug_assert_eq!(length, optional_len(&self.softirq_time));
        debug_assert_eq!(length, optional_len(&self.stolen_time));
        debug_assert_eq!(length, optional_len(&self.guest_time));
        debug_assert_eq!(length, optional_len(&self.guest_nice_time));

        // Return the overall length
        length
    }
}
//
lazy_static! {
    /// Number of CPU ticks from the statistics of /proc/stat in one second
    static ref TICKS_PER_SEC: u64 = unsafe {
        libc::sysconf(libc::_SC_CLK_TCK) as u64
    };

    /// Number of nanoseconds in one CPU tick
    static ref NANOSECS_PER_TICK: u64 = 1_000_000_000 / *TICKS_PER_SEC;
}


/// Unit tests
#[cfg(test)]
mod tests {
    use ::splitter::split_and_run;
    use std::time::Duration;
    use super::{CPUStatData, StatDataStore, TICKS_PER_SEC};

    /// Check that CPU statistics initialization works as expected
    #[test]
    fn init_cpu_stat() {
        // Oldest known CPU stats format from Linux 4.11's man proc
        let oldest_stats = CPUStatData::new(4);
        assert_eq!(oldest_stats.user_time.len(), 0);
        assert_eq!(oldest_stats.nice_time.len(), 0);
        assert_eq!(oldest_stats.system_time.len(), 0);
        assert_eq!(oldest_stats.idle_time.len(), 0);
        assert!(oldest_stats.io_wait_time.is_none());
        assert!(oldest_stats.guest_nice_time.is_none());
        assert_eq!(oldest_stats.len(), 0);

        // First known CPU stats extension from Linux 4.11's man proc
        let first_ext_stats = CPUStatData::new(5);
        assert_eq!(first_ext_stats.io_wait_time, Some(Vec::new()));
        assert!(first_ext_stats.irq_time.is_none());
        assert!(first_ext_stats.guest_nice_time.is_none());
        assert_eq!(first_ext_stats.len(), 0);

        // Newest known CPU stats format from Linux 4.11's man proc
        let latest_stats = CPUStatData::new(10);
        assert_eq!(latest_stats.io_wait_time, Some(Vec::new()));
        assert_eq!(latest_stats.guest_nice_time, Some(Vec::new()));
        assert_eq!(latest_stats.len(), 0);
    }

    /// Check that parsing CPU statistics works as expected
    #[test]
    fn parse_cpu_stat() {
        // Oldest known CPU stats format from Linux 4.11's man proc
        let mut oldest_stats = CPUStatData::new(4);

        // Figure out the duration of a kernel tick
        let tick_duration = Duration::new(
            0,
            (1_000_000_000 / *TICKS_PER_SEC) as u32
        );

        // Check that "old" CPU stats are parsed properly
        oldest_stats.push_str("165 18 96 1");
        assert_eq!(oldest_stats.user_time,   vec![tick_duration*165]);
        assert_eq!(oldest_stats.nice_time,   vec![tick_duration*18]);
        assert_eq!(oldest_stats.system_time, vec![tick_duration*96]);
        assert_eq!(oldest_stats.idle_time,   vec![tick_duration]);
        assert!(oldest_stats.io_wait_time.is_none());
        assert_eq!(oldest_stats.len(), 1);

        // Check that "extended" stats are parsed as well
        let mut first_ext_stats = CPUStatData::new(5);
        first_ext_stats.push_str("9 698 6521 151 56");
        assert_eq!(first_ext_stats.io_wait_time, Some(vec![tick_duration*56]));
        assert!(first_ext_stats.irq_time.is_none());
        assert_eq!(first_ext_stats.len(), 1);

        // Check that "complete" stats are parsed as well
        let mut latest_stats = CPUStatData::new(10);
        latest_stats.push_str("18 9616 11 941 5 51 9 615 62 14");
        assert_eq!(latest_stats.io_wait_time,    Some(vec![tick_duration*5]));
        assert_eq!(latest_stats.guest_nice_time, Some(vec![tick_duration*14]));
        assert_eq!(latest_stats.len(), 1);
    }
}
