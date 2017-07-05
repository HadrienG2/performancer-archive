//! This module contains a sampling parser for /proc/stat

use ::ProcFileReader;
use chrono::{DateTime, TimeZone, Utc};
use std::io::Result;
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
        self.reader.sample(|file_contents: &str| {
            // TODO: Parse the contents of /proc/stat, add debug_asserts for unknown entries
            unimplemented!();
        })
    }

    // TODO: Add accessors to the inner stat data + associated tests
}


/// Data samples from /proc/stat, in structure-of-array layout
///
/// Courtesy of Linux's total lack of promises regarding the variability of
/// /proc/stat across hardware architectures, or even on a given system
/// depending on kernel configuration, every entry of this struct is considered
/// optional at this point.
///
#[derive(Debug, Default, PartialEq)]
struct StatData {
    /// Total CPU usage stats, aggregated across all hardware threads
    all_cpus: Option<CPUStatData>,

    /// Per-CPU usage statistics, featuring one entry per hardware thread
    each_cpu: Option<Vec<CPUStatData>>,

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
}
//
impl StatData {
    /// Create a new statistical data store, using a first sample to know the
    /// structure of /proc/stat on this system
    fn new(initial_contents: &str) -> Self {
        // Our statistical data store will eventually go there
        let mut data: Self = Default::default();

        // For each line of the initial contents of /proc/stat...
        let mut num_cpu_timers = 0u8;
        for line in initial_contents.lines() {
            // ...decompose according whitespace...
            let mut whitespace_iter = line.split_whitespace();

            // ...and check the header
            match whitespace_iter.next().unwrap() {
                // Statistics on all CPUs, should come first
                "cpu" => {
                    num_cpu_timers = whitespace_iter.count() as u8;
                    data.all_cpus = Some(CPUStatData::new(num_cpu_timers));
                }

                // Statistics on a specific CPU thread
                header if &header[0..3] == "cpu" => {
                    // If we didn't know, note that we have per-thread data and
                    // check for data format consistency with global CPU stats
                    if data.each_cpu.is_none() {
                        assert_eq!(whitespace_iter.count() as u8,
                                   num_cpu_timers);
                        data.each_cpu = Some(Vec::new());
                    }

                    // Add one thread-specific entry to the list
                    if let Some(ref mut cpu_vec) = data.each_cpu {
                        cpu_vec.push(CPUStatData::new(num_cpu_timers));
                    }
                },

                // Paging statistics
                "page" => data.paging = Some(PagingStatData::new()),

                // Swapping statistics
                "swap" => data.swapping = Some(PagingStatData::new()),

                // Hardware interrupt statistics
                "intr" => {
                    let num_interrupts = (whitespace_iter.count() - 1) as u16;
                    data.interrupts = Some(
                        InterruptStatData::new(num_interrupts)
                    );
                },

                // Context switch statistics
                "ctxt" => data.context_switches = Some(Vec::new()),

                // Boot time
                "btime" => {
                    let btime_str = whitespace_iter.next().unwrap();
                    debug_assert_eq!(whitespace_iter.next(), None);
                    data.boot_time = Some(
                        Utc.timestamp(btime_str.parse().unwrap(), 0)
                    );
                },

                // Number of process forks since boot
                "processes" => data.process_forks = Some(Vec::new()),

                // Number of processes in the runnable state
                "procs_running" => data.runnable_processes = Some(Vec::new()),

                // Number of processes waiting for I/O
                "procs_blocked" => data.blocked_processes = Some(Vec::new()),

                // Softirq statistics
                "softirq" => {
                    let num_interrupts = (whitespace_iter.count() - 1) as u16;
                    data.softirqs = Some(
                        InterruptStatData::new(num_interrupts)
                    );
                },

                // Something we do not support yet? We should!
                unknown_header => {
                    debug_assert!(false,
                                  "Unsupported entry '{}' detected!",
                                  unknown_header);
                }
            }
        }

        // Return our data collection setup
        data
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
        debug_assert!(num_timers >= 4, "Not expected from man 5 proc!");
        debug_assert!(num_timers <= 10, "Unknown CPU timers detected!");

        // Conditionally create a certain amount of timing Vecs
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
            user_time: Vec::new(),
            nice_time: Vec::new(),
            system_time: Vec::new(),
            idle_time: Vec::new(),
            io_wait_time: conditional_vec(),
            irq_time: conditional_vec(),
            softirq_time: conditional_vec(),
            stolen_time: conditional_vec(),
            guest_time: conditional_vec(),
            guest_nice_time: conditional_vec(),
        }
    }
}


/// Interrupt statistics from /proc/stat, in structure-of-array layout
#[derive(Debug, PartialEq)]
struct InterruptStatData {
    /// Total number of interrupts that were serviced. May be higher than the
    /// sum of the breakdown below if there are unnumbered interrupt sources.
    total: Vec<u64>,

    /// For each numbered source, details on the amount of serviced interrupt.
    details: Vec<Vec<u64>>
}
//
impl InterruptStatData {
    /// Create new interrupt statistics, given the amount of interrupt sources
    fn new(num_irqs: u16) -> Self {
        Self {
            total: Vec::new(),
            details: vec![Vec::new(); num_irqs as usize],
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


/// These are the unit tests for this module
#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use super::{CPUStatData, InterruptStatData, PagingStatData, StatData};

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

        // First known CPU stats extension from Linux 4.11's man proc
        let first_ext_stats = CPUStatData::new(5);
        assert_eq!(first_ext_stats.io_wait_time, Some(Vec::new()));
        assert!(first_ext_stats.irq_time.is_none());
        assert!(first_ext_stats.guest_nice_time.is_none());

        // Newest known CPU stats format from Linux 4.11's man proc
        let latest_stats = CPUStatData::new(10);
        assert_eq!(latest_stats.io_wait_time, Some(Vec::new()));
        assert_eq!(latest_stats.guest_nice_time, Some(Vec::new()));
    }

    // Check that interrupt statistics initialization works as expected
    #[test]
    fn init_interrupt_stat() {
        // Check that interrupt statistics without any details work
        let no_details_stats = InterruptStatData::new(0);
        assert_eq!(no_details_stats.total.len(), 0);
        assert_eq!(no_details_stats.details.len(), 0);

        // Check that interrupt statistics with two detailed counters work
        let no_details_stats = InterruptStatData::new(2);
        assert_eq!(no_details_stats.details.len(), 2);
        assert_eq!(no_details_stats.details[0].len(), 0);
        assert_eq!(no_details_stats.details[1].len(), 0);

        // Check that interrupt statistics with lots of detailed counters work
        let no_details_stats = InterruptStatData::new(256);
        assert_eq!(no_details_stats.details.len(), 256);
        assert_eq!(no_details_stats.details[0].len(), 0);
        assert_eq!(no_details_stats.details[255].len(), 0);
    }

    // Check that paging statistics initialization works as expected
    #[test]
    fn init_paging_stat() {
        // Check that paging statistics initialization works
        let stats = PagingStatData::new();
        assert_eq!(stats.incoming.len(), 0);
        assert_eq!(stats.outgoing.len(), 0);
    }

    // Check that statistical data initialization works as expected
    #[test]
    fn init_stat_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut stats = String::new();
        let empty_stats = StatData::new(&stats);
        assert!(empty_stats.all_cpus.is_none());
        assert!(empty_stats.each_cpu.is_none());
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
        assert_eq!(global_cpu_stats, expected);

        // ...adding dual-core CPU stats
        stats.push_str("\ncpu0 0 1 1 3
                          cpu1 1 1 2 1");
        let local_cpu_stats = StatData::new(&stats);
        expected.each_cpu = Some(vec![CPUStatData::new(4); 2]);
        assert_eq!(local_cpu_stats, expected);

        // ...adding paging stats
        stats.push_str("\npage 42 43");
        let paging_stats = StatData::new(&stats);
        expected.paging = Some(PagingStatData::new());
        assert_eq!(paging_stats, expected);

        // ...adding swapping stats
        stats.push_str("\nswap 24 34");
        let swapping_stats = StatData::new(&stats);
        expected.swapping = Some(PagingStatData::new());
        assert_eq!(swapping_stats, expected);

        // ...adding interrupt stats
        stats.push_str("\nintr 12345 678 910");
        let interrupt_stats = StatData::new(&stats);
        expected.interrupts = Some(InterruptStatData::new(2));
        assert_eq!(interrupt_stats, expected);

        // ...adding context switches
        stats.push_str("\nctxt 654321");
        let context_stats = StatData::new(&stats);
        expected.context_switches = Some(Vec::new());
        assert_eq!(context_stats, expected);

        // ...adding boot time
        stats.push_str("\nbtime 5738295");
        let boot_time_stats = StatData::new(&stats);
        expected.boot_time = Some(Utc.timestamp(5738295, 0));
        assert_eq!(boot_time_stats, expected);

        // ...adding process fork counter
        stats.push_str("\nprocesses 94536551");
        let process_fork_stats = StatData::new(&stats);
        expected.process_forks = Some(Vec::new());
        assert_eq!(process_fork_stats, expected);

        // ...adding runnable process counter
        stats.push_str("\nprocs_running 1624");
        let runnable_process_stats = StatData::new(&stats);
        expected.runnable_processes = Some(Vec::new());
        assert_eq!(runnable_process_stats, expected);

        // ...adding blocked process counter
        stats.push_str("\nprocs_blocked 8948");
        let blocked_process_stats = StatData::new(&stats);
        expected.blocked_processes = Some(Vec::new());
        assert_eq!(blocked_process_stats, expected);

        // ...adding softirq stats
        stats.push_str("\nsoftirq 94651 1561 21211 12 71867");
        let softirq_stats = StatData::new(&stats);
        expected.softirqs = Some(InterruptStatData::new(4));
        assert_eq!(softirq_stats, expected);
    }

    /* TODO: Restore this
    /// Check that no samples are initially present
    #[test]
    fn new_sampler() {
        let stat = StatSampler::new().unwrap();
        unimplemented!();
        /* assert_eq!(uptime.samples.wall_clock_uptime.len(), 0);
        assert_eq!(uptime.samples.cpu_idle_time.len(), 0); */
    } */

    /* TODO: Restore this 
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
    } */
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
        let mut reader = ProcFileReader::open("/proc/stat").unwrap();
        testbench::benchmark(100_000, || {
            reader.sample(|_| {}).unwrap();
        });
    }

    /* TODO: Restore this
    /// Benchmark for the full stat sampling overhead
    #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut stat = StatSampler::new().unwrap();
        testbench::benchmark(100_000, || {
            stat.sample().unwrap();
        });
    } */
}
