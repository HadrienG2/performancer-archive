//! This module contains a sampling parser for /proc/diskstats

use std::collections::HashMap;
use std::time::Duration;


// TODO: Mechanism for sampling measurements from /proc/meminfo


/// Data samples from /proc/diskstats, in structure-of-array layout
///
/// TODO: Provide a more detailed description after implementation
///
struct DiskStatsData {
    /// List of iostat records following original file order (as in MemInfoData)
    records: Vec<DiskStatsRecord>,

    /// Index mapping device numbers to the index of the associated record
    device_numbers_index: HashMap<DeviceNumbers, usize>,

    /// Index mapping device names to the index of the associated record
    device_name_index: HashMap<String, usize>,
}
//
// TODO: Implement this
// TODO: Think carefully about hotplug and how we want to handle it


/// Sampled records from /proc/diskstats, with a zero-record optimization
/// TODO: Decide whether code sharing with the interrupt sampler is worthwhile
/// TODO: This parser can also be used when parsing /sys/block/<device>/stat.
///       Do we want to implement support for that and make code reuse easy?
enum DiskStatsRecord {
    /// If we've only ever seen zeroes, we only count the number of zeroes
    Zeroes(usize),

    /// Otherwise we record various statistics in structure-of-array layout
    /// TODO: During implementation, take care that Linux stores these counters
    ///       as usize and allows them to overflow.
    Samples {
        /// Total number of reads that completed successfully
        completed_reads: Vec<u64>,

        /// Total number of adjacent reads that were merged by the kernel
        merged_reads: Vec<u64>,

        /// Total number of drive sectors that were successfully read
        sector_reads: Vec<u64>,

        /// Total time spent reading data, as measured by summing the difference
        /// between the end and start time of all reads.
        ///
        /// **WARNING**
        ///
        /// Use a lot of care when interpreting this statistic. It is easy to
        /// misunderstand it for something that it is not:
        ///
        /// - The clock starts when reads are queued in the Linux kernel, not
        ///   when they are actually processed. This is thus an indicator of how
        ///   long all threads cumulatively blocked for IO, rather than of how
        ///   much time the underlying hardware spent at servicing IO requests.
        /// - Such an indicator can be quite meaningless in applications with
        ///   optimized IO patterns which rely on asynchronous APIs or dedicated
        ///   IO threads to avoid wasting CPU time during IO requests.
        ///
        total_read_time: Vec<Duration>,

        /// Total number of writes that completed successfully
        completed_writes: Vec<u64>,

        /// Total number of adjacent writes that were merged by the kernel
        merged_writes: Vec<u64>,

        /// Total number of drive sectors that were successfully written
        sector_writes: Vec<u64>,

        /// Total time spent writing data, as measured by summing the difference
        /// between the end and start time of all writes.
        ///
        /// The warning given about total_read_time also applies here.
        ///
        total_write_time: Vec<Duration>,

        /// Number of IO operations that are in progress (queued or running)
        io_in_progress: Vec<usize>,

        /// Total wall clock time spent performing IO
        ///
        /// This a measure of the wall clock time during which a nonzero amount
        /// of IO tasks were in progress (per the indicator above). This maps
        /// quite well to the time spent by the underlying hardware on IO...
        /// given the caveat that the kernel could delay the submission of
        /// queued IO requests for power management or throughput reasons.
        ///
        wall_clock_io_time: Vec<Duration>,

        /// Weighted time spent performing IO
        ///
        /// On every update (which happens on various IO events), this timer is
        /// incremented by the time spent doing IO since the last update (per
        /// the wall_clock_io_time counter) times the amount of outstanding IO
        /// requests. This can be an indicator of IO pressure in the kernel.
        ///
        weighted_io_time: Vec<Duration>,
        
        // TODO: Check for unknown fields in the implementation
        // TODO: Take note of the warning given by the kernel iostats
        //       documentation concerning kernel versions between 2.4 and 2.6.25
        //       and partition-specific metadata.
        // TODO: Also take note of the sysfs facility for per-device stats
    },
}
//
// TODO: Implement this


/// Device identifier based on major and minor device numbers
///
/// This maps to the dev_t type from the Linux kernel, but it uses 64 bits
/// instead of 32 bits in order to maximize the odds that this library will
/// still work under future kernel versions.
///
#[derive(Eq, Hash, PartialEq)]
struct DeviceNumbers {
    // Major device number, usually (but not always) maps to a kernel driver
    major: u32,

    // Minor device number, arbitrarily attributed by drivers to devices
    minor: u32,
}
//
impl DeviceNumbers {
    /// Create a new device number from a (major, minor) device id pair
    fn new(major: u32, minor: u32) -> Self {
        Self {
            major,
            minor,
        }
    }
}


// TODO: Unit tests


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    use ::reader::ProcFileReader;
    use testbench;

    /// Benchmark for the raw diskstats readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader =
            ProcFileReader::open("/proc/diskstats")
                           .expect("Failed to open disk stats");
        testbench::benchmark(90_000, || {
            reader.sample(|_| {}).expect("Failed to read disk stats");
        });
    }

    // TODO: Benchmark for the full diskstats sampling overhead
    /* #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut stat =
            DiskStatsSampler::new()
                             .expect("Failed to create a disk stats sampler");
        testbench::benchmark(400_000, || {
            stat.sample().expect("Failed to sample disk stats");
        });
    } */
}
