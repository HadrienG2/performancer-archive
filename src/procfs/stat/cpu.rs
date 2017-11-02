//! This module contains facilities for parsing and storing the data contained
//! in the "cpu" sections of /proc/stat.

use ::data::SampledData;
use ::splitter::SplitColumns;
use libc;
use std::time::Duration;


/// CPU statistics record from /proc/stat
///
/// This will yield the amount of CPU time that the system (or one of its
/// hardware CPU threads) spent in various states.
///
/// Some timings were added in a certain Linux release and will only be provided
/// by sufficiently recent kernels. You will find the ordered list of the
/// expected timings and associated kernel version requirements below, and can
/// use the "version" module of this crate in order to check what should be
/// expected from the host kernel.
///
/// 1. user time (spent in a user mode process)
/// 2. nice time (spent in a user mode process, running with low priority)
/// 3. system time (spent in system mode, running kernel code)
/// 4. idle time (spent doing nothing, "in the idle task")
/// 5. iowait time (mostly deprecated and meaningless today, used to be a
///    measure of the time spent waiting for I/O to complete) **Linux 2.5.41+**
/// 6. irq time (spent servicing hardware interrupts) **Linux 2.6.0-test4+**
/// 7. softirq time (spent servicing software interrupts) **Linux 2.6.0-test4+**
/// 8. steal time (spent in other OSs, when virtualized) **Linux 2.6.11+**
/// 9. guest time (spent running a guest virtualized OS) **Linux 2.6.24+**
/// 10. guest_nice (spent running a guast, with low priority) **Linux 2.6.33+**
///
pub(super) struct RecordFields<'a, 'b> where 'a: 'b {
    /// Data columns of the record, interpreted as CPU timings
    data_columns: SplitColumns<'a, 'b>,

    /// Number of clock ticks in one second (cached from TICKS_PER_SEC)
    ticks_per_sec: u64,

    /// Number of nanoseconds in one clock tick (cached from NANOSECS_PER_TICK)
    nanosecs_per_tick: u64,
}
//
impl<'a, 'b> Iterator for RecordFields<'a, 'b> {
    /// We're outputting real time durations
    type Item = Duration;

    /// This is how we generate them from file columns
    fn next(&mut self) -> Option<Self::Item> {
        self.data_columns.next().map(|str_duration| {
            let ticks: u64 =
                str_duration.parse()
                            .expect("Failed to parse CPU tick counter");
            let secs = ticks / self.ticks_per_sec;
            let nanosecs =
                (ticks % self.ticks_per_sec) * self.nanosecs_per_tick;
            Duration::new(secs, nanosecs as u32)
        })
    }
}
//
impl<'a, 'b> RecordFields<'a, 'b> {
    /// Build a new parser for CPU record fields
    pub fn new(data_columns: SplitColumns<'a, 'b>) -> Self {
        Self {
            data_columns,
            ticks_per_sec: *TICKS_PER_SEC,
            nanosecs_per_tick: *NANOSECS_PER_TICK,
        }
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


/// The amount of CPU time that the system spent in various states
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Data {
    /// Time spent in user mode
    user_time: Vec<Duration>,

    /// Time spent in user mode with low priority (nice)
    nice_time: Vec<Duration>,

    /// Time spent in system (aka kernel) mode
    system_time: Vec<Duration>,

    /// Time spent in the idle task (should match second entry in /proc/uptime)
    idle_time: Vec<Duration>,

    /// Time spent waiting for IO to complete (since Linux 2.5.41)
    /// BEWARE: This measure is mostly meaningless on modern kernels
    io_wait_time: Option<Vec<Duration>>,

    /// Time spent servicing hardware interrupts (since Linux 2.6.0-test4)
    irq_time: Option<Vec<Duration>>,

    /// Time spent servicing software interrupts (since Linux 2.6.0-test4)
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
impl SampledData for Data {
    /// Tell how many samples are present in the data store + check consistency
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
// TODO: Implement SampledData2 once that is usable in stable Rust
impl Data {
    /// Create new CPU statistics
    pub fn new(fields: RecordFields) -> Self {
        // Check if we know about all CPU timers
        let num_timers = fields.count();
        assert!(num_timers >= 4, "Some expected CPU timers are missing");
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

    /// Parse CPU statistics and add them to the internal data store
    pub fn push(&mut self, mut fields: RecordFields) {
        // This scope is needed to please rustc's current borrow checker
        {
            // Load the "mandatory" CPU statistics
            self.user_time.push(fields.next().expect("User time missing"));
            self.nice_time.push(fields.next().expect("Nice time missing"));
            self.system_time.push(fields.next().expect("System time missing"));
            self.idle_time.push(fields.next().expect("Idle time missing"));

            // Load the "optional" CPU statistics
            let mut optional_load = |stat: &mut Option<Vec<Duration>>| {
                if let Some(ref mut vec) = *stat {
                    vec.push(fields.next().expect("A CPU timer went missing"));
                }
            };
            optional_load(&mut self.io_wait_time);
            optional_load(&mut self.irq_time);
            optional_load(&mut self.softirq_time);
            optional_load(&mut self.stolen_time);
            optional_load(&mut self.guest_time);
            optional_load(&mut self.guest_nice_time);
        }

        // At this point, we should have loaded all available stats
        debug_assert!(fields.next().is_none(),
                      "A CPU timer appeared out of nowhere");
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use std::time::Duration;
    use ::splitter::split_line_and_run;
    use super::{Data, RecordFields, SampledData, NANOSECS_PER_TICK};

    /// Test the parsing of valid CPU stats
    #[test]
    fn record_field_parsing() {
        // Figure out the duration of a kernel tick
        let tick_duration = *TICK_DURATION;

        // Check that the oldest supported CPU stats format is parsed properly
        with_record_fields("165 18 96 1", |mut fields| {
            assert_eq!(fields.next(), Some(tick_duration*165));
            assert_eq!(fields.next(), Some(tick_duration*18));
            assert_eq!(fields.next(), Some(tick_duration*96));
            assert_eq!(fields.next(), Some(tick_duration));
            assert_eq!(fields.next(), None);
        });

        // Check that a slightly extended version parses just as well
        with_record_fields("9 678 6521 151 56", |mut fields| {
            assert_eq!(fields.next(), Some(tick_duration*9));
            assert_eq!(fields.next(), Some(tick_duration*678));
            assert_eq!(fields.next(), Some(tick_duration*6521));
            assert_eq!(fields.next(), Some(tick_duration*151));
            assert_eq!(fields.next(), Some(tick_duration*56));
            assert_eq!(fields.next(), None);
        });

        // Check that the newest supported CPU stats format parses as well
        with_record_fields("18 9613 11 941 5 51 9 615 62 14", |mut fields| {
            assert_eq!(fields.next(), Some(tick_duration*18));
            assert_eq!(fields.next(), Some(tick_duration*9613));
            assert_eq!(fields.next(), Some(tick_duration*11));
            assert_eq!(fields.next(), Some(tick_duration*941));
            assert_eq!(fields.next(), Some(tick_duration*5));
            assert_eq!(fields.next(), Some(tick_duration*51));
            assert_eq!(fields.next(), Some(tick_duration*9));
            assert_eq!(fields.next(), Some(tick_duration*615));
            assert_eq!(fields.next(), Some(tick_duration*62));
            assert_eq!(fields.next(), Some(tick_duration*14));
            assert_eq!(fields.next(), None);
        });
    }

    /// Check that CPU stats containers work well for the oldest stat format
    #[test]
    fn oldest_stats() {
        // Figure out the duration of a kernel tick
        let tick_duration = *TICK_DURATION;

        // Check that building a container for the oldest stats format works
        let mut data = with_record_fields("94 6316 64 2", Data::new);
        assert_eq!(data.user_time,          Vec::new());
        assert_eq!(data.nice_time,          Vec::new());
        assert_eq!(data.system_time,        Vec::new());
        assert_eq!(data.idle_time,          Vec::new());
        assert_eq!(data.io_wait_time,       None);
        assert_eq!(data.irq_time,           None);
        assert_eq!(data.softirq_time,       None);
        assert_eq!(data.stolen_time,        None);
        assert_eq!(data.guest_time,         None);
        assert_eq!(data.guest_nice_time,    None);
        assert_eq!(data.len(),              0);

        // Check that pushing data into it works as well
        with_record_fields("46 421 3 7866", |fields| data.push(fields));
        assert_eq!(data.user_time,          vec![tick_duration*46]);
        assert_eq!(data.nice_time,          vec![tick_duration*421]);
        assert_eq!(data.system_time,        vec![tick_duration*3]);
        assert_eq!(data.idle_time,          vec![tick_duration*7866]);
        assert_eq!(data.io_wait_time,       None);
        assert_eq!(data.irq_time,           None);
        assert_eq!(data.softirq_time,       None);
        assert_eq!(data.stolen_time,        None);
        assert_eq!(data.guest_time,         None);
        assert_eq!(data.guest_nice_time,    None);
        assert_eq!(data.len(),              1);
    }

    /// Check that the first historical "extented" stats format works as well
    #[test]
    fn extended_stats() {
        // Figure out the duration of a kernel tick
        let tick_duration = *TICK_DURATION;

        // Check that building a container for the extended stats format works
        let mut data = with_record_fields("66 321 795 12 32", Data::new);
        assert_eq!(data.user_time,          Vec::new());
        assert_eq!(data.nice_time,          Vec::new());
        assert_eq!(data.system_time,        Vec::new());
        assert_eq!(data.idle_time,          Vec::new());
        assert_eq!(data.io_wait_time,       Some(Vec::new()));
        assert_eq!(data.irq_time,           None);
        assert_eq!(data.softirq_time,       None);
        assert_eq!(data.stolen_time,        None);
        assert_eq!(data.guest_time,         None);
        assert_eq!(data.guest_nice_time,    None);
        assert_eq!(data.len(),              0);

        // Check that pushing data into it works as well
        with_record_fields("3122 21 9 46 32", |fields| data.push(fields));
        assert_eq!(data.user_time,          vec![tick_duration*3122]);
        assert_eq!(data.nice_time,          vec![tick_duration*21]);
        assert_eq!(data.system_time,        vec![tick_duration*9]);
        assert_eq!(data.idle_time,          vec![tick_duration*46]);
        assert_eq!(data.io_wait_time,       Some(vec![tick_duration*32]));
        assert_eq!(data.irq_time,           None);
        assert_eq!(data.softirq_time,       None);
        assert_eq!(data.stolen_time,        None);
        assert_eq!(data.guest_time,         None);
        assert_eq!(data.guest_nice_time,    None);
        assert_eq!(data.len(),              1);
    }

    /// Check that the latest supported stats format works as well
    #[test]
    fn latest_stats() {
        // Figure out the duration of a kernel tick
        let tick_duration = *TICK_DURATION;

        // Check that building a container for the extended stats format works
        let mut data = with_record_fields("31 854 361 32 6 8 21 9 3 2",
                                          Data::new);
        assert_eq!(data.user_time,          Vec::new());
        assert_eq!(data.nice_time,          Vec::new());
        assert_eq!(data.system_time,        Vec::new());
        assert_eq!(data.idle_time,          Vec::new());
        assert_eq!(data.io_wait_time,       Some(Vec::new()));
        assert_eq!(data.irq_time,           Some(Vec::new()));
        assert_eq!(data.softirq_time,       Some(Vec::new()));
        assert_eq!(data.stolen_time,        Some(Vec::new()));
        assert_eq!(data.guest_time,         Some(Vec::new()));
        assert_eq!(data.guest_nice_time,    Some(Vec::new()));
        assert_eq!(data.len(),              0);

        // Check that pushing data into it works as well
        with_record_fields("21 61 8 5 9 3 1 7 0 4", |fields| data.push(fields));
        assert_eq!(data.user_time,          vec![tick_duration*21]);
        assert_eq!(data.nice_time,          vec![tick_duration*61]);
        assert_eq!(data.system_time,        vec![tick_duration*8]);
        assert_eq!(data.idle_time,          vec![tick_duration*5]);
        assert_eq!(data.io_wait_time,       Some(vec![tick_duration*9]));
        assert_eq!(data.irq_time,           Some(vec![tick_duration*3]));
        assert_eq!(data.softirq_time,       Some(vec![tick_duration*1]));
        assert_eq!(data.stolen_time,        Some(vec![tick_duration*7]));
        assert_eq!(data.guest_time,         Some(vec![tick_duration*0]));
        assert_eq!(data.guest_nice_time,    Some(vec![tick_duration*4]));
        assert_eq!(data.len(),              1);
    }

    /// Build the CPU record fields associated with a certain line of text, and
    /// run code taking that as a parameter
    fn with_record_fields<F, R>(line_of_text: &str, functor: F) -> R
        where F: FnOnce(RecordFields) -> R
    {
        split_line_and_run(line_of_text, |columns| {
            let fields = RecordFields::new(columns);
            functor(fields)
        })
    }

    lazy_static! {
        /// Duration of one CPU tick, only suitable for debugging use at the
        /// moment since Duration has no multiplication operator for u64 (alas!)
        static ref TICK_DURATION: Duration = Duration::new(
            0,
            *NANOSECS_PER_TICK as u32
        );
    }
}
