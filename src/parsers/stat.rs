//! This module contains a sampling parser for /proc/stat

use ::ProcFileReader;
use chrono::{DateTime, TimeZone, Utc};
use libc;
use parsers::SplitSpace;
use std::fmt::Debug;
use std::io::Result;
use std::str::FromStr;
use std::time::Duration;


/// Mechanism for sampling measurements from /proc/stat
pub struct StatSampler {
    /// Reader object for /proc/stat
    reader: ProcFileReader,

    /// Sampled statistical data
    samples: StatData,
}
//
impl StatSampler {
    /// Create a new sampler of /proc/stat
    pub fn new() -> Result<Self> {
        let mut reader = ProcFileReader::open("/proc/stat")?;
        let mut first_readout = String::new();
        reader.sample(|file_contents| first_readout.push_str(file_contents))?;
        Ok(
            Self {
                reader,
                samples: StatData::new(&first_readout),
            }
        )
    }

    /// Acquire a new sample of statistical data
    pub fn sample(&mut self) -> Result<()> {
        let samples = &mut self.samples;
        self.reader.sample(|file_contents: &str| samples.push(file_contents))
    }

    // TODO: Add accessors to the inner stat data + associated tests
}


/// Data samples from /proc/stat, in structure-of-array layout
///
/// Courtesy of Linux's total lack of promises regarding the variability of
/// /proc/stat across hardware architectures, or even on a given system
/// depending on kernel configuration, most entries of this struct are
/// considered optional at this point...
///
#[derive(Debug, Default, PartialEq)]
struct StatData {
    /// Total CPU usage stats, aggregated across all hardware threads
    all_cpus: Option<CPUStatData>,

    /// Per-CPU usage statistics, featuring one entry per hardware thread
    ///
    /// An empty Vec here has the same meaning as a None in other entries: the
    /// per-thread breakdown of CPU usage was not provided by the kernel.
    ///
    each_cpu: Vec<CPUStatData>,

    /// Number of pages that the system paged in and out from disk, overall...
    paging: Option<PagingStatData>,

    /// ...and narrowing it down to swapping activity in particular
    swapping: Option<PagingStatData>,

    /// Statistics on the number of hardware interrupts that were serviced
    interrupts: Option<InterruptStatData>,

    // NOTE: Linux 2.4 used to have disk_io statistics in /proc/stat as well,
    //       but since that is incredibly ancient, we propose not to support it.

    /// Number of context switches that the system underwent since boot
    context_switches: Option<Vec<u64>>,

    /// Boot time (only collected once)
    boot_time: Option<DateTime<Utc>>,

    /// Number of process forks that occurred since boot
    process_forks: Option<Vec<u32>>,

    /// Number of processes in a runnable state (since Linux 2.5.45)
    runnable_processes: Option<Vec<u16>>,

    /// Number of processes blocked waiting for I/O (since Linux 2.5.45)
    blocked_processes: Option<Vec<u16>>,

    /// Statistics on the number of softirqs that were serviced. These use the
    /// same layout as hardware interrupt stats, where softirqs are enumerated
    /// in the same order as in /proc/softirq.
    softirqs: Option<InterruptStatData>,

    /// INTERNAL: This vector indicates how each line of /proc/stat maps to the
    /// members of this struct. It basically is a legal and move-friendly
    /// variant of the obvious Vec<&mut StatDataParser> approach.
    ///
    /// The idea of mapping lines of /proc/stat to struct members builds on the
    /// assumption, which we make in other places in this library, that the
    /// kernel configuration (and thus the layout of /proc/stat) will not change
    /// over the course of a series of sampling measurements.
    ///
    line_target: Vec<StatDataMember>,
}
//
impl StatData {
    /// Create a new statistical data store, using a first sample to know the
    /// structure of /proc/stat on this system
    fn new(initial_contents: &str) -> Self {
        // Our statistical data store will eventually go there
        let mut data: Self = Default::default();

        // The amount of CPU timers will go there once it's known
        let mut num_cpu_timers = 0u8;

        // For each line of the initial contents of /proc/stat...
        for line in initial_contents.lines() {
            // ...decompose according whitespace...
            let mut whitespace_iter = SplitSpace::new(line);

            // ...and check the header
            match whitespace_iter.next().expect("Unexpected empty line") {
                // Statistics on all CPUs (should come first)
                "cpu" => {
                    num_cpu_timers = whitespace_iter.count() as u8;
                    data.all_cpus = Some(CPUStatData::new(num_cpu_timers));
                    data.line_target.push(StatDataMember::AllCPUs);
                }

                // Statistics on a specific CPU thread (should be consistent
                // with the global stats and come after them)
                header if &header[0..3] == "cpu" => {
                    assert_eq!(whitespace_iter.count() as u8, num_cpu_timers,
                               "Inconsistent amount of CPU timers");
                    data.each_cpu.push(CPUStatData::new(num_cpu_timers));
                    data.line_target.push(StatDataMember::EachCPU);
                },

                // Paging statistics
                "page" => {
                    data.paging = Some(PagingStatData::new());
                    data.line_target.push(StatDataMember::Paging);
                },

                // Swapping statistics
                "swap" => {
                    data.swapping = Some(PagingStatData::new());
                    data.line_target.push(StatDataMember::Swapping);
                },

                // Hardware interrupt statistics
                "intr" => {
                    let num_interrupts = (whitespace_iter.count() - 1) as u16;
                    data.interrupts = Some(
                        InterruptStatData::new(num_interrupts)
                    );
                    data.line_target.push(StatDataMember::Interrupts);
                },

                // Context switch statistics
                "ctxt" => {
                    data.context_switches = Some(Vec::new());
                    data.line_target.push(StatDataMember::ContextSwitches);
                },

                // Boot time
                "btime" => {
                    let btime_str =
                        whitespace_iter.next().expect("Missing boot time data");
                    debug_assert_eq!(whitespace_iter.next(), None,
                                     "Unexpected extra boot time data");
                    data.boot_time = Some(
                        Utc.timestamp(
                            btime_str.parse()
                                     .expect("Boot time should be an integer"),
                            0
                        )
                    );
                    data.line_target.push(StatDataMember::BootTime);
                },

                // Number of process forks since boot
                "processes" => {
                    data.process_forks = Some(Vec::new());
                    data.line_target.push(StatDataMember::ProcessForks);
                },

                // Number of processes in the runnable state
                "procs_running" => {
                    data.runnable_processes = Some(Vec::new());
                    data.line_target.push(StatDataMember::RunnableProcesses);
                },

                // Number of processes waiting for I/O
                "procs_blocked" => {
                    data.blocked_processes = Some(Vec::new());
                    data.line_target.push(StatDataMember::BlockedProcesses);
                },

                // Softirq statistics
                "softirq" => {
                    let num_interrupts = (whitespace_iter.count() - 1) as u16;
                    data.softirqs = Some(
                        InterruptStatData::new(num_interrupts)
                    );
                    data.line_target.push(StatDataMember::SoftIRQs);
                },

                // Something we do not support yet? We should!
                unknown_header => {
                    debug_assert!(false,
                                  "Unsupported entry '{}' detected!",
                                  unknown_header);
                    data.line_target.push(StatDataMember::Unsupported);
                }
            }
        }

        // Return our data collection setup
        data
    }

    /// Parse the contents of /proc/stat and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, file_contents: &str) {
        // This is the hardware CPU thread which we are currently considering
        let mut cpu_iter = self.each_cpu.iter_mut();

        // This time, we know how lines of /proc/stat map to our members
        for (line, target) in file_contents.lines()
                                           .zip(self.line_target.iter()) {
            // The beginning of parsing is the same as before: split by spaces.
            // But this time, we discard the header, as we already know it.
            let mut stats = SplitSpace::new(line);
            stats.next();

            // Forward the /proc/stat data to the appropriate parser
            match *target {
                StatDataMember::AllCPUs => {
                    Self::force_push(&mut self.all_cpus, stats);
                },
                StatDataMember::EachCPU => {
                    cpu_iter.next()
                            .expect("Per-cpu stats do not match each_cpu.len()")
                            .push(stats);
                },
                StatDataMember::Paging => {
                    Self::force_push(&mut self.paging, stats);
                },
                StatDataMember::Swapping => {
                    Self::force_push(&mut self.swapping, stats);
                },
                StatDataMember::Interrupts => {
                    Self::force_push(&mut self.interrupts, stats);
                },
                StatDataMember::ContextSwitches => {
                    Self::force_push(&mut self.context_switches, stats);
                },
                StatDataMember::ProcessForks => {
                    Self::force_push(&mut self.process_forks, stats);
                },
                StatDataMember::RunnableProcesses => {
                    Self::force_push(&mut self.runnable_processes, stats);
                },
                StatDataMember::BlockedProcesses => {
                    Self::force_push(&mut self.blocked_processes, stats);
                },
                StatDataMember::SoftIRQs => {
                    Self::force_push(&mut self.softirqs, stats);
                }
                StatDataMember::BootTime | StatDataMember::Unsupported => {},
            }
        }

        // At the end of parsing, all CPU threads should have been considered
        debug_assert!(cpu_iter.next().is_none(),
                      "Per-cpu stats do not match each_cpu.len()");
    }

    // Tell how many samples are present in the data store, and in debug mode
    // check for internal data store consistency
    #[allow(dead_code)]
    fn len(&self) -> usize {
        let mut opt_len = None;
        Self::update_len(&mut opt_len, &self.all_cpus);
        debug_assert!(
            self.each_cpu
                .iter()
                .all(|cpu| {
                    opt_len.expect("each_cpu should come with all_cpus") ==
                        cpu.len()
                })
        );
        Self::update_len(&mut opt_len, &self.paging);
        Self::update_len(&mut opt_len, &self.swapping);
        Self::update_len(&mut opt_len, &self.interrupts);
        Self::update_len(&mut opt_len, &self.context_switches);
        Self::update_len(&mut opt_len, &self.process_forks);
        Self::update_len(&mut opt_len, &self.runnable_processes);
        Self::update_len(&mut opt_len, &self.blocked_processes);
        Self::update_len(&mut opt_len, &self.softirqs);
        opt_len.unwrap_or(0)
    }

    // INTERNAL: Helpful wrapper for pushing into optional containers that we
    //           actually know from additional metadata to be around
    fn force_push<T>(store: &mut Option<T>, stats: SplitSpace)
        where T: StatDataStore
    {
        store.as_mut()
             .expect("Attempted to push into a nonexistent container")
             .push(stats);
    }

    // INTERNAL: Update our prior knowledge of the amount of stored samples
    //           (current_len) according to an optional data source.
    fn update_len<T>(current_len: &mut Option<usize>, opt_store: &Option<T>)
        where T: StatDataStore
    {
        // This closure will get us the amount of samples stored inside of the
        // optional data source as an Option<usize>, if we turn out to need it
        let get_len = || opt_store.as_ref().map(|store| store.len());
        
        // Do we already know the amount of samples that should be in there?
        match *current_len {
            // If so, we only need to check if it is as expected, in debug mode
            Some(old_len) => {
                if let Some(new_len) = get_len() {
                    debug_assert_eq!(new_len, old_len,
                                     "Inconsistent amounts of stored samples");
                }
            },

            // If not, we can safely overwrite our knowledge of the amount of
            // samples with data from our new data source
            None => *current_len = get_len(),
        }
    }
}
//
// This enum should be kept in sync with the definition of StatData
//
#[derive(Debug, PartialEq)]
enum StatDataMember {
    // Data storage elements of StatData
    AllCPUs,
    EachCPU,
    Paging,
    Swapping,
    Interrupts,
    ContextSwitches,
    BootTime,
    ProcessForks,
    RunnableProcesses,
    BlockedProcesses,
    SoftIRQs,

    // Special entry for unsupported fields of /proc/stat
    Unsupported
}


/// Every container of /proc/stat data should implement the following trait,
/// which exposes its ability to be filled from segmented /proc/stat contents.
trait StatDataStore {
    /// Parse and record a sample of data from /proc/stat
    fn push(&mut self, stats: SplitSpace);

    /// Number of data samples that were recorded so far
    fn len(&self) -> usize;
}


/// We implement this trait for primitive types that can be parsed from &str
impl<T, U> StatDataStore for Vec<T>
    where T: FromStr<Err=U>,
          U: Debug
{
    fn push(&mut self, mut stats: SplitSpace) {
        self.push(stats.next().expect("Expected statistical data")
                       .parse().expect("Failed to parse statistical data"));
        debug_assert!(stats.next().is_none(),
                      "No other statistical data should be present");
    }

    fn len(&self) -> usize {
        <Vec<T>>::len(self)
    }
}


/// The amount of CPU time that the system spent in various states
#[derive(Clone, Debug, PartialEq)]
struct CPUStatData {
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
    fn new(num_timers: u8) -> Self {
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
    fn push(&mut self, mut stats: SplitSpace) {
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
                      "A CPU tick counter appeared out of nowhere");
    }

    // Tell how many samples are present in the data store
    #[allow(dead_code)]
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


/// Interrupt statistics from /proc/stat, in structure-of-array layout
#[derive(Debug, PartialEq)]
struct InterruptStatData {
    /// Total number of interrupts that were serviced. May be higher than the
    /// sum of the breakdown below if there are unnumbered interrupt sources.
    total: Vec<u64>,

    /// For each numbered source, details on the amount of serviced interrupt.
    details: Vec<InterruptCounts>
}
//
impl InterruptStatData {
    /// Create new interrupt statistics, given the amount of interrupt sources
    fn new(num_irqs: u16) -> Self {
        Self {
            total: Vec::new(),
            details: vec![InterruptCounts::new(); num_irqs as usize],
        }
    }
}
//
impl StatDataStore for InterruptStatData {
    /// Parse interrupt statistics and add them to the internal data store
    fn push(&mut self, mut stats: SplitSpace) {
        // Load the total interrupt count
        self.total.push(stats.next().expect("Total IRQ count missing")
                             .parse().expect("Failed to parse IRQ count"));

        // Load the detailed interrupt counts from each source
        for detail in self.details.iter_mut() {
            detail.push(stats.next().expect("An IRQ counter went missing"));
        }

        // At this point, we should have loaded all available stats
        debug_assert!(stats.next().is_none(),
                      "An IRQ counter appeared out of nowhere");
    }

    // Tell how many samples are present in the data store
    #[allow(dead_code)]
    fn len(&self) -> usize {
        let length = self.total.len();
        debug_assert!(self.details.iter().all(|vec| vec.len() == length));
        length
    }
}
///
/// On some platforms such as x86, there are a lot of hardware IRQs (~500 on my
/// machines), but most of them are unused and never fire. Parsing and storing
/// the associated zeroes from /proc/stat by normal means wastes CPU time and
/// RAM, so we take a shortcut for this common use case.
///
#[derive(Clone, Debug, PartialEq)]
enum InterruptCounts {
    /// If we've only ever seen zeroes, we only count the number of zeroes
    Zeroes(usize),

    /// Otherwise, we sample the interrupt counts normally
    Samples(Vec<u64>),
}
//
impl InterruptCounts {
    /// Initialize the interrupt count sampler
    fn new() -> Self {
        InterruptCounts::Zeroes(0)
    }

    /// Insert a new interrupt count from /proc/stat
    fn push(&mut self, intr_count: &str) {
        match *self {
            // Have we only seen zeroes so far?
            InterruptCounts::Zeroes(zero_count) => {
                // Are we seeing a zero again?
                if intr_count == "0" {
                    // If yes, just increment the zero counter
                    *self = InterruptCounts::Zeroes(zero_count+1);
                } else {
                    // If not, move to regular interrupt count sampling
                    let mut samples = vec![0; zero_count];
                    samples.push(
                        intr_count.parse().expect("Failed to parse IRQ count")
                    );
                    *self = InterruptCounts::Samples(samples);
                }
            },

            // If the interrupt counter is nonzero, sample it normally
            InterruptCounts::Samples(ref mut vec) => {
                vec.push(intr_count.parse()
                                   .expect("Failed to parse IRQ count"));
            }
        }
    }

    /// Tell how many interrupt counts we have recorded so far
    fn len(&self) -> usize {
        match *self {
            InterruptCounts::Zeroes(zero_count) => zero_count,
            InterruptCounts::Samples(ref vec) => vec.len(),
        }
    }
}


/// Storage paging ativity statistics
#[derive(Debug, PartialEq)]
struct PagingStatData {
    /// Number of RAM pages that were paged in from disk
    incoming: Vec<u64>,

    /// Number of RAM pages that were paged out to disk
    outgoing: Vec<u64>,
}
//
impl PagingStatData {
    /// Create new paging statistics
    fn new() -> Self {
        Self {
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }
    }
}
//
impl StatDataStore for PagingStatData {
    /// Parse paging statistics and add them to the internal data store
    fn push(&mut self, mut stats: SplitSpace) {
        // Load the incoming and outgoing page count
        self.incoming.push(stats.next().expect("Missing incoming page count")
                                .parse().expect("Could not parse page count"));
        self.outgoing.push(stats.next().expect("Missing outgoing page count")
                                .parse().expect("Could not parse page count"));

        // At this point, we should have loaded all available stats
        debug_assert!(stats.next().is_none(),
                      "Unexpected counter in paging statistics");
    }

    // Tell how many samples are present in the data store
    #[allow(dead_code)]
    fn len(&self) -> usize {
        let length = self.incoming.len();
        debug_assert_eq!(length, self.outgoing.len());
        length
    }
}


/// These are the unit tests for this module
#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use std::time::Duration;
    use super::{CPUStatData, InterruptCounts, InterruptStatData, PagingStatData,
                SplitSpace, StatData, StatDataMember, StatDataStore,
                StatSampler, TICKS_PER_SEC};

    // Check that scalar statistics parsing works as expected
    #[test]
    fn parse_scalar_stat() {
        let mut scalar_stats = Vec::<u64>::new();
        assert_eq!(StatDataStore::len(&scalar_stats), 0);
        StatDataStore::push(&mut scalar_stats, SplitSpace::new("123"));
        assert_eq!(scalar_stats, vec![123]);
        assert_eq!(StatDataStore::len(&scalar_stats), 1);
    }

    // Check that CPU statistics initialization works as expected
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

    // Check that parsing CPU statistics works as expected
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
        oldest_stats.push(SplitSpace::new("165 18 96 1"));
        assert_eq!(oldest_stats.user_time,   vec![tick_duration*165]);
        assert_eq!(oldest_stats.nice_time,   vec![tick_duration*18]);
        assert_eq!(oldest_stats.system_time, vec![tick_duration*96]);
        assert_eq!(oldest_stats.idle_time,   vec![tick_duration]);
        assert!(oldest_stats.io_wait_time.is_none());
        assert_eq!(oldest_stats.len(), 1);

        // Check that "extended" stats are parsed as well
        let mut first_ext_stats = CPUStatData::new(5);
        first_ext_stats.push(SplitSpace::new("9 698 6521 151 56"));
        assert_eq!(first_ext_stats.io_wait_time, Some(vec![tick_duration*56]));
        assert!(first_ext_stats.irq_time.is_none());
        assert_eq!(first_ext_stats.len(), 1);

        // Check that "complete" stats are parsed as well
        let mut latest_stats = CPUStatData::new(10);
        latest_stats.push(SplitSpace::new("18 9616 11 941 5 51 9 615 62 14"));
        assert_eq!(latest_stats.io_wait_time,    Some(vec![tick_duration*5]));
        assert_eq!(latest_stats.guest_nice_time, Some(vec![tick_duration*14]));
        assert_eq!(latest_stats.len(), 1);
    }

    // Check that initializing an interrupt count sampler works as expected
    #[test]
    fn init_interrupt_counts() {
        let counts = InterruptCounts::new();
        assert_eq!(counts, InterruptCounts::Zeroes(0));
        assert_eq!(counts.len(), 0);
    }

    // Check that interrupt count sampling works as expected
    #[test]
    fn parse_interrupt_counts() {
        // Adding one zero should keep us in the base "zeroes" state
        let mut counts = InterruptCounts::new();
        counts.push("0");
        assert_eq!(counts, InterruptCounts::Zeroes(1));
        assert_eq!(counts.len(), 1);

        // Adding a nonzero value should get us out of this state
        counts.push("123");
        assert_eq!(counts, InterruptCounts::Samples(vec![0, 123]));
        assert_eq!(counts.len(), 2);

        // After that, sampling should work normally
        counts.push("456");
        assert_eq!(counts, InterruptCounts::Samples(vec![0, 123, 456]));
        assert_eq!(counts.len(), 3);

        // Sampling right from the start should work as well
        let mut counts2 = InterruptCounts::new();
        counts2.push("789");
        assert_eq!(counts2, InterruptCounts::Samples(vec![789]));
        assert_eq!(counts2.len(), 1);
    }

    // Check that interrupt statistics initialization works as expected
    #[test]
    fn init_interrupt_stat() {
        // Check that interrupt statistics without any details work
        let no_details_stats = InterruptStatData::new(0);
        assert_eq!(no_details_stats.total.len(), 0);
        assert_eq!(no_details_stats.details.len(), 0);
        assert_eq!(no_details_stats.len(), 0);

        // Check that interrupt statistics with two detailed counters work
        let two_stats = InterruptStatData::new(2);
        assert_eq!(two_stats.details.len(), 2);
        assert_eq!(two_stats.details[0].len(), 0);
        assert_eq!(two_stats.details[1].len(), 0);
        assert_eq!(two_stats.len(), 0);

        // Check that interrupt statistics with lots of detailed counters work
        let many_stats = InterruptStatData::new(256);
        assert_eq!(many_stats.details.len(), 256);
        assert_eq!(many_stats.details[0].len(), 0);
        assert_eq!(many_stats.details[255].len(), 0);
        assert_eq!(many_stats.len(), 0);
    }

    // Check that parsing interrupt statistics works as expected
    #[test]
    fn parse_interrupt_stat() {
        // Interrupt statistics without any detail
        let mut no_details_stats = InterruptStatData::new(0);
        no_details_stats.push(SplitSpace::new("12345"));
        assert_eq!(no_details_stats.total, vec![12345]);
        assert_eq!(no_details_stats.details.len(), 0);
        assert_eq!(no_details_stats.len(), 1);

        // Interrupt statistics with two detailed counters
        let mut two_stats = InterruptStatData::new(2);
        two_stats.push(SplitSpace::new("12345 678 910"));
        assert_eq!(two_stats.total, vec![12345]);
        assert_eq!(two_stats.details, 
                   vec![InterruptCounts::Samples(vec![678]),
                        InterruptCounts::Samples(vec![910])]);
        assert_eq!(two_stats.len(), 1);
    }

    // Check that paging statistics initialization works as expected
    #[test]
    fn init_paging_stat() {
        let stats = PagingStatData::new();
        assert_eq!(stats.incoming.len(), 0);
        assert_eq!(stats.outgoing.len(), 0);
        assert_eq!(stats.len(), 0);
    }

    // Check that parsing paging statistics works as expected
    #[test]
    fn parse_paging_stat() {
        let mut stats = PagingStatData::new();
        stats.push(SplitSpace::new("123 456"));
        assert_eq!(stats.incoming, vec![123]);
        assert_eq!(stats.outgoing, vec![456]);
        assert_eq!(stats.len(), 1);
    }

    // Check that statistical data initialization works as expected
    #[test]
    fn init_stat_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut stats = String::new();
        let empty_stats = StatData::new(&stats);
        assert!(empty_stats.all_cpus.is_none());
        assert_eq!(empty_stats.each_cpu.len(), 0);
        assert!(empty_stats.paging.is_none());
        assert!(empty_stats.swapping.is_none());
        assert!(empty_stats.interrupts.is_none());
        assert!(empty_stats.context_switches.is_none());
        assert!(empty_stats.boot_time.is_none());
        assert!(empty_stats.process_forks.is_none());
        assert!(empty_stats.runnable_processes.is_none());
        assert!(empty_stats.blocked_processes.is_none());
        assert!(empty_stats.softirqs.is_none());
        let mut expected = empty_stats;

        // ...adding global CPU stats
        stats.push_str("cpu 1 2 3 4");
        let global_cpu_stats = StatData::new(&stats);
        expected.all_cpus = Some(CPUStatData::new(4));
        expected.line_target.push(StatDataMember::AllCPUs);
        assert_eq!(global_cpu_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding dual-core CPU stats
        stats.push_str("\ncpu0 0 1 1 3
                          cpu1 1 1 2 1");
        let local_cpu_stats = StatData::new(&stats);
        expected.each_cpu = vec![CPUStatData::new(4); 2];
        expected.line_target.push(StatDataMember::EachCPU);
        expected.line_target.push(StatDataMember::EachCPU);
        assert_eq!(local_cpu_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding paging stats
        stats.push_str("\npage 42 43");
        let paging_stats = StatData::new(&stats);
        expected.paging = Some(PagingStatData::new());
        expected.line_target.push(StatDataMember::Paging);
        assert_eq!(paging_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding swapping stats
        stats.push_str("\nswap 24 34");
        let swapping_stats = StatData::new(&stats);
        expected.swapping = Some(PagingStatData::new());
        expected.line_target.push(StatDataMember::Swapping);
        assert_eq!(swapping_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding interrupt stats
        stats.push_str("\nintr 12345 678 910");
        let interrupt_stats = StatData::new(&stats);
        expected.interrupts = Some(InterruptStatData::new(2));
        expected.line_target.push(StatDataMember::Interrupts);
        assert_eq!(interrupt_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding context switches
        stats.push_str("\nctxt 654321");
        let context_stats = StatData::new(&stats);
        expected.context_switches = Some(Vec::new());
        expected.line_target.push(StatDataMember::ContextSwitches);
        assert_eq!(context_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding boot time
        stats.push_str("\nbtime 5738295");
        let boot_time_stats = StatData::new(&stats);
        expected.boot_time = Some(Utc.timestamp(5738295, 0));
        expected.line_target.push(StatDataMember::BootTime);
        assert_eq!(boot_time_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding process fork counter
        stats.push_str("\nprocesses 94536551");
        let process_fork_stats = StatData::new(&stats);
        expected.process_forks = Some(Vec::new());
        expected.line_target.push(StatDataMember::ProcessForks);
        assert_eq!(process_fork_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding runnable process counter
        stats.push_str("\nprocs_running 1624");
        let runnable_process_stats = StatData::new(&stats);
        expected.runnable_processes = Some(Vec::new());
        expected.line_target.push(StatDataMember::RunnableProcesses);
        assert_eq!(runnable_process_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding blocked process counter
        stats.push_str("\nprocs_blocked 8948");
        let blocked_process_stats = StatData::new(&stats);
        expected.blocked_processes = Some(Vec::new());
        expected.line_target.push(StatDataMember::BlockedProcesses);
        assert_eq!(blocked_process_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding softirq stats
        stats.push_str("\nsoftirq 94651 1561 21211 12 71867");
        let softirq_stats = StatData::new(&stats);
        expected.softirqs = Some(InterruptStatData::new(4));
        expected.line_target.push(StatDataMember::SoftIRQs);
        assert_eq!(softirq_stats, expected);
        assert_eq!(expected.len(), 0);
    }

    // Check that statistical data parsing works as expected
    #[test]
    fn parse_stat_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut stats = String::new();
        let mut empty_stats = StatData::new(&stats);
        empty_stats.push(&stats);
        let mut expected = StatData::new(&stats);
        assert_eq!(empty_stats, expected);

        // Adding global CPU stats
        stats.push_str("cpu 1 2 3 4");
        let mut global_cpu_stats = StatData::new(&stats);
        global_cpu_stats.push(&stats);
        expected = StatData::new(&stats);
        expected.all_cpus.as_mut()
                         .expect("CPU stats incorrectly marked as missing")
                         .push(SplitSpace::new("1 2 3 4"));
        assert_eq!(global_cpu_stats, expected);
        assert_eq!(expected.len(), 1);

        // Adding dual-core CPU stats
        stats.push_str("\ncpu0 0 1 1 3
                          cpu1 1 1 2 1");
        let mut local_cpu_stats = StatData::new(&stats);
        local_cpu_stats.push(&stats);
        expected = StatData::new(&stats);
        expected.all_cpus.as_mut()
                         .expect("CPU stats incorrectly marked as missing")
                         .push(SplitSpace::new("1 2 3 4"));
        expected.each_cpu[0].push(SplitSpace::new("0 1 1 3"));
        expected.each_cpu[1].push(SplitSpace::new("1 1 2 1"));
        assert_eq!(local_cpu_stats, expected);
        assert_eq!(expected.len(), 1);

        // Starting over from paging stats
        stats = String::from("page 42 43");
        let mut paging_stats = StatData::new(&stats);
        paging_stats.push(&stats);
        expected = StatData::new(&stats);
        expected.paging.as_mut()
                       .expect("Paging stats incorrectly marked as missing")
                       .push(SplitSpace::new("42 43"));
        assert_eq!(paging_stats, expected);
        assert_eq!(expected.len(), 1);

        // Starting over from softirq stats
        stats = String::from("softirq 94651 1561 21211 12 71867");
        let mut softirq_stats = StatData::new(&stats);
        softirq_stats.push(&stats);
        expected = StatData::new(&stats);
        expected.softirqs.as_mut()
                         .expect("Softirq stats incorrectly marked as missing")
                         .push(SplitSpace::new("94651 1561 21211 12 71867"));
        assert_eq!(softirq_stats, expected);
        assert_eq!(expected.len(), 1);
    }

    // Check that sampler initialization works well
    #[test]
    fn init_sampler() {
        let stats =
            StatSampler::new()
                        .expect("Failed to create a /proc/stat sampler");
        assert_eq!(stats.samples.len(), 0);
    }

    // Check that basic sampling works as expected
    #[test]
    fn basic_sampling() {
        let mut stats =
            StatSampler::new()
                        .expect("Failed to create a /proc/stat sampler");
        stats.sample().expect("Failed to sample stats once");
        assert_eq!(stats.samples.len(), 1);
        stats.sample().expect("Failed to sample stats twice");
        assert_eq!(stats.samples.len(), 2);
    }
}


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    use ::ProcFileReader;
    use super::StatSampler;
    use testbench;

    /// Benchmark for the raw stat readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader = ProcFileReader::open("/proc/stat")
                                        .expect("Failed to open /proc/stat");
        testbench::benchmark(100_000, || {
            reader.sample(|_| {}).expect("Failed to read /proc/stat");
        });
    }

    /// Benchmark for the full stat sampling overhead
    #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut stat =
            StatSampler::new()
                        .expect("Failed to create a /proc/stat sampler");
        testbench::benchmark(100_000, || {
            stat.sample().expect("Failed to sample /proc/stat");
        });
    }
}
