//! This module contains a sampling parser for /proc/stat

use ::ProcFileReader;
use chrono::{DateTime, TimeZone, Utc};
use parsers::version::LINUX_VERSION;
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
#[derive(Default)]
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

    /// Statistics on the number of soft-irqs that were serviced. These use the
    /// same layout as hardware interrupt stats, where soft-irqs are enumerated
    /// in the same order as in /proc/softirq.
    soft_irqs: Option<InterruptStatData>,
}
//
impl StatData {
    /// Create a new statistical data store, using a first sample to know the
    /// structure of /proc/stat on this system
    fn new(initial_contents: &str) -> Self {
        // Our statistical data store will eventually go there
        let mut data: Self = Default::default();

        // For each line of the initial contents of /proc/stat...
        for line in initial_contents.lines() {
            // ...decompose according whitespace...
            let mut whitespace_iter = line.split_whitespace();

            // ...and check the header
            match whitespace_iter.next().unwrap() {
                // Statistics on all CPUs
                // TODO: Check whether we correctly guess the number of fields
                // TODO: Check that we know about all fields
                "cpu" => unimplemented!(), // data.all_cpus = Some(CPUStatData::new()),

                // Statistics on a specific CPU thread
                header if &header[0..3] == "cpu" => {
                    // If we didn't know, note that we have per-thread data
                    if data.each_cpu.is_none() {
                        data.each_cpu = Some(Vec::new());
                    }

                    // Add one thread-specific entry to the list
                    if let Some(ref mut cpu_vec) = data.each_cpu {
                        cpu_vec.push(CPUStatData::new());
                    }
                },

                // Paging statistics
                "page" => data.paging = Some(PagingStatData::new()),

                // Swapping statistics
                "swap" => data.swapping = Some(PagingStatData::new()),

                // Hardware interrupt statistics
                "intr" => {
                    // TODO: Figure out how many interrupt sources we have
                    // TODO: Initialize data.interrupts accordingly
                    unimplemented!();
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

                // Soft IRQs statistics
                "softirq" => {
                    // TODO: Figure out how many interrupt sources we have
                    // TODO: Initialize data.soft_irqs accordingly
                    unimplemented!();
                },

                // Something we do not support yet? We should!
                unknown_header => {
                    debug_assert!(false,
                                  "Found unsupported entry '{}' in /proc/stat",
                                  unknown_header);
                }
            }
        }

        // TODO: Return our result
        // TODO: Check this against the /proc manual page very carefully and
        //       implement plenty of tests for it. It's complex, so it's wrong.
        unimplemented!();
    }
}


/// CPU statistics from /proc/stat, in structure-of-array layout
struct CPUStatData {
    /// Time spent in user mode
    user_time: Vec<Duration>,

    /// Time spent in user mode with low priority (nice)
    nice_time: Vec<Duration>,

    /// Time spent in system (aka kernel) mode
    system_time: Vec<Duration>,

    /// Time spent in the idle task
    idle_time: Vec<Duration>,

    /// Time spent waiting for IO to complete (since Linux 2.5.41)
    io_time: Option<Vec<Duration>>,

    /// Time spent servicing hardware interrupts (since Linux 2.6.0-test4)
    irq_time: Option<Vec<Duration>>,

    /// Time spent servicing soft-irqs (since Linux 2.6.0-test4)
    soft_irq_time: Option<Vec<Duration>>,

    /// "Stolen" time spent in other operating systems when running in a
    /// virtualized environment (since Linux 2.6.11)
    stolen_time: Option<Vec<Duration>>,

    /// Time spent running in a virtual CPU for guest OSs (since Linux 2.6.24)
    guest_time: Option<Vec<Duration>>,

    /// Time spent running a niced guest (see above, since Linux 2.6.33)
    guest_nice_time: Option<Vec<Duration>>,
}
//
impl CPUStatData {
    /// Create new CPU statistics
    fn new() -> Self {
        // Conditionally create a Vec if a kernel version requirement is met
        let vec_if_kernel_compatible = |major, minor, bugfix| {
            if LINUX_VERSION.greater_eq(major, minor, bugfix) {
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
            io_time: vec_if_kernel_compatible(2, 5, 41),
            irq_time: vec_if_kernel_compatible(2, 6, 1),
            soft_irq_time: vec_if_kernel_compatible(2, 6, 1),
            stolen_time: vec_if_kernel_compatible(2, 6, 11),
            guest_time: vec_if_kernel_compatible(2, 6, 24),
            guest_nice_time: vec_if_kernel_compatible(2, 6, 33),
        }
    }
}


/// Paging statistics from /proc/stat, in structure-of-array layout
struct PagingStatData {
    /// Number of pages that were paged in from disk
    incoming: Vec<u64>,

    /// Number of pages that were paged out to disk
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


/// Interrupt statistics from /proc/stat, in structure-of-array layout
struct InterruptStatData {
    /// Total number of interrupts that were serviced. May be higher than the
    /// sum of the breakdown below if there are unnumbered interrupt sources.
    total: Vec<u64>,

    /// For each interrupt source, details on the amount of serviced interrupt.
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


/// These are the unit tests for this module
#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use super::StatSampler;
    
    /// Check that no samples are initially present
    #[test]
    fn new_sampler() {
        let stat = StatSampler::new().unwrap();
        unimplemented!();
        /* assert_eq!(uptime.samples.wall_clock_uptime.len(), 0);
        assert_eq!(uptime.samples.cpu_idle_time.len(), 0); */
    }

    /* /// Test that basic sampling works as expected
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

    /// Benchmark for the full stat sampling overhead
    #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut stat = StatSampler::new().unwrap();
        testbench::benchmark(100_000, || {
            stat.sample().unwrap();
        });
    }
}
