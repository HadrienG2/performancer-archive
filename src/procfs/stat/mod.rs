//! This module contains a sampling parser for /proc/stat

mod cpu;
mod interrupt;
mod paging;

use ::reader::ProcFileReader;
use ::splitter::SplitLinesBySpace;
use chrono::{DateTime, TimeZone, Utc};
use self::cpu::CPUStatData;
use self::interrupt::InterruptStatData;
use self::paging::PagingStatData;
use std::fmt::Debug;
use std::io::Result;
use std::str::FromStr;


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
#[derive(Debug, PartialEq)]
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
        let mut data = Self {
            all_cpus: None,
            each_cpu: Vec::new(),
            paging: None,
            swapping: None,
            interrupts: None,
            context_switches: None,
            boot_time: None,
            process_forks: None,
            runnable_processes: None,
            blocked_processes: None,
            softirqs: None,
            line_target: Vec::new(),
        };

        // The amount of CPU timers will go there once it's known
        let mut num_cpu_timers = 0u8;

        // For each line of the initial contents of /proc/stat...
        let mut splitter = SplitLinesBySpace::new(initial_contents);
        while splitter.next_line() {
            // ...and check the header
            match splitter.next().expect("Unexpected empty line") {
                // Statistics on all CPUs (should come first)
                "cpu" => {
                    num_cpu_timers = splitter.col_count() as u8;
                    data.all_cpus = Some(CPUStatData::new(num_cpu_timers));
                    data.line_target.push(StatDataMember::AllCPUs);
                }

                // Statistics on a specific CPU thread (should be consistent
                // with the global stats and come after them)
                header if &header[0..3] == "cpu" => {
                    assert_eq!(splitter.col_count() as u8, num_cpu_timers,
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
                    let num_interrupts = (splitter.col_count() - 1) as u16;
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
                        splitter.next().expect("Missing boot time data");
                    debug_assert_eq!(splitter.next(), None,
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
                    let num_interrupts = (splitter.col_count() - 1) as u16;
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
        let mut splitter = SplitLinesBySpace::new(file_contents);
        for target in self.line_target.iter() {
            // The beginning of parsing is the same as before: split by spaces.
            // But this time, we discard the header, as we already know it.
            assert!(splitter.next_line(), "A stat record has disappeared");
            splitter.next();

            // Forward the /proc/stat data to the appropriate parser
            match *target {
                StatDataMember::AllCPUs => {
                    Self::force_push(&mut self.all_cpus, &mut splitter);
                },
                StatDataMember::EachCPU => {
                    cpu_iter.next()
                            .expect("Per-cpu stats do not match each_cpu.len()")
                            .push(&mut splitter);
                },
                StatDataMember::Paging => {
                    Self::force_push(&mut self.paging, &mut splitter);
                },
                StatDataMember::Swapping => {
                    Self::force_push(&mut self.swapping, &mut splitter);
                },
                StatDataMember::Interrupts => {
                    Self::force_push(&mut self.interrupts, &mut splitter);
                },
                StatDataMember::ContextSwitches => {
                    Self::force_push(&mut self.context_switches, &mut splitter);
                },
                StatDataMember::ProcessForks => {
                    Self::force_push(&mut self.process_forks, &mut splitter);
                },
                StatDataMember::RunnableProcesses => {
                    Self::force_push(&mut self.runnable_processes,
                                     &mut splitter);
                },
                StatDataMember::BlockedProcesses => {
                    Self::force_push(&mut self.blocked_processes,
                                     &mut splitter);
                },
                StatDataMember::SoftIRQs => {
                    Self::force_push(&mut self.softirqs, &mut splitter);
                }
                StatDataMember::BootTime | StatDataMember::Unsupported => {},
            }
        }

        // At the end of parsing, all CPU threads should have been considered
        debug_assert!(cpu_iter.next().is_none(),
                      "Per-cpu stats do not match each_cpu.len()");
    }

    /// Tell how many samples are present in the data store, and in debug mode
    /// check for internal data store consistency
    #[cfg(test)]
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

    /// INTERNAL: Helpful wrapper for pushing into optional containers that we
    ///           actually know from additional metadata to be around
    fn force_push<T>(store: &mut Option<T>, splitter: &mut SplitLinesBySpace)
        where T: StatDataStore
    {
        store.as_mut()
             .expect("Attempted to push into a nonexistent container")
             .push(splitter);
    }

    /// INTERNAL: Update our prior knowledge of the amount of stored samples
    ///           (current_len) according to an optional data source.
    #[cfg(test)]
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
///
/// This enum should be kept in sync with the definition of StatData
///
#[derive(Debug, PartialEq)]
enum StatDataMember {
    /// Data storage elements of StatData
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

    /// Special entry for unsupported fields of /proc/stat
    Unsupported
}


/// Every container of /proc/stat data should implement the following trait,
/// which exposes its ability to be filled from segmented /proc/stat contents.
trait StatDataStore {
    /// Parse and record a sample of data from /proc/stat
    fn push(&mut self, splitter: &mut SplitLinesBySpace);

    /// Number of data samples that were recorded so far
    #[cfg(test)]
    fn len(&self) -> usize;
}


/// We implement this trait for primitive types that can be parsed from &str
impl<T, U> StatDataStore for Vec<T>
    where T: FromStr<Err=U>,
          U: Debug
{
    fn push(&mut self, splitter: &mut SplitLinesBySpace) {
        self.push(splitter.next().expect("Expected statistical data")
                       .parse().expect("Failed to parse statistical data"));
        debug_assert!(splitter.next().is_none(),
                      "No other statistical data should be present");
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        <Vec<T>>::len(self)
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use ::splitter::split_line;
    use chrono::{TimeZone, Utc};
    use super::{CPUStatData, InterruptStatData, PagingStatData, StatData,
                StatDataMember, StatDataStore, StatSampler};

    /// Check that scalar statistics parsing works as expected
    #[test]
    fn parse_scalar_stat() {
        let mut scalar_stats = Vec::<u64>::new();
        assert_eq!(StatDataStore::len(&scalar_stats), 0);
        StatDataStore::push(&mut scalar_stats, &mut split_line("123"));
        assert_eq!(scalar_stats, vec![123]);
        assert_eq!(StatDataStore::len(&scalar_stats), 1);
    }

    /// Check that statistical data initialization works as expected
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

    /// Check that statistical data parsing works as expected
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
                         .push(&mut split_line("1 2 3 4"));
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
                         .push(&mut split_line("1 2 3 4"));
        expected.each_cpu[0].push(&mut split_line("0 1 1 3"));
        expected.each_cpu[1].push(&mut split_line("1 1 2 1"));
        assert_eq!(local_cpu_stats, expected);
        assert_eq!(expected.len(), 1);

        // Starting over from paging stats
        stats = String::from("page 42 43");
        let mut paging_stats = StatData::new(&stats);
        paging_stats.push(&stats);
        expected = StatData::new(&stats);
        expected.paging.as_mut()
                       .expect("Paging stats incorrectly marked as missing")
                       .push(&mut split_line("42 43"));
        assert_eq!(paging_stats, expected);
        assert_eq!(expected.len(), 1);

        // Starting over from softirq stats
        stats = String::from("softirq 94651 1561 21211 12 71867");
        let mut softirq_stats = StatData::new(&stats);
        softirq_stats.push(&stats);
        expected = StatData::new(&stats);
        expected.softirqs.as_mut()
                         .expect("Softirq stats incorrectly marked as missing")
                         .push(&mut split_line("94651 1561 21211 12 71867"));
        assert_eq!(softirq_stats, expected);
        assert_eq!(expected.len(), 1);
    }

    /// Check that sampler initialization works well
    #[test]
    fn init_sampler() {
        let stats =
            StatSampler::new()
                        .expect("Failed to create a /proc/stat sampler");
        assert_eq!(stats.samples.len(), 0);
    }

    /// Check that basic sampling works as expected
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
    use ::reader::ProcFileReader;
    use super::StatSampler;
    use testbench;

    /// Benchmark for the raw stat readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader = ProcFileReader::open("/proc/stat")
                                        .expect("Failed to open CPU stats");
        testbench::benchmark(100_000, || {
            reader.sample(|_| {}).expect("Failed to read CPU stats");
        });
    }

    /// Benchmark for the full stat sampling overhead
    #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut stat =
            StatSampler::new()
                        .expect("Failed to create a CPU stats sampler");
        testbench::benchmark(100_000, || {
            stat.sample().expect("Failed to sample CPU stats");
        });
    }
}
