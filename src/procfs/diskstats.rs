///! This module contains a sampling parser for /proc/diskstats

use ::parser::PseudoFileParser;
use ::procfs::version::LINUX_VERSION;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use std::time::Duration;


// Implement a /proc/diskstats sampler using DiskStatsData for parsing & storage
/* define_sampler!{ Sampler : "/proc/diskstats" => Parser => Data } */


/// Incremental parser for /proc/diskstats
#[derive(Debug, PartialEq)]
pub struct Parser {}
//
impl PseudoFileParser for Parser {
    /// Build a parser, using an initial file sample.
    fn new(initial_contents: &str) -> Self {
        // TODO: Perform initial schema validation, caching
        Self {}
    }
}
//
// TODO: Implement IncrementalParser once that trait is usable in stable Rust
impl Parser {
    /// Parse a pseudo-file sample into a stream of records
    pub fn parse<'a>(&mut self, file_contents: &'a str) -> RecordStream<'a> {
        RecordStream::new(file_contents)
    }
}
///
///
/// Stream of records from /proc/diskstats
///
/// This streaming iterator should yield a stream of disk stats records, each
/// representing a line of /proc/diskstats (i.e. statistics on a block device).
///
pub struct RecordStream<'a> {
    /// Iterator into the lines and columns of /proc/diskstats
    file_lines: SplitLinesBySpace<'a>,
}
//
impl<'a> RecordStream<'a> {
    /// Parse the next record from /proc/diskstats into a stream of fields
    pub fn next<'b>(&'b mut self) -> Option<Record<'a, 'b>>
        where 'a: 'b
    {
        self.file_lines.next().map(Record::new)
    }

    /// Create a record stream from raw contents
    fn new(file_contents: &'a str) -> Self {
        Self {
            file_lines: SplitLinesBySpace::new(file_contents),
        }
    }
}
///
///
/// Record from /proc/diskstats (activity of one block device)
pub struct Record<'a, 'b> where 'a: 'b {
    // Device numbers
    device_nums: DeviceNumbers,

    // Device name
    device_str: &'a str,

    // Unparsed device statistics
    stats_columns: SplitColumns<'a, 'b>,
}
//
impl<'a, 'b> Record<'a, 'b> {
    /// Query the device number
    fn device_numbers(&self) -> DeviceNumbers {
        self.device_nums
    }

    /// Query the device name
    fn device_name(&self) -> &str {
        self.device_str
    }

    // TODO: Query the statistics

    /// Construct a record from associated file columns
    fn new(mut columns: SplitColumns<'a, 'b>) -> Self {
        let major_num = columns.next().expect("Expected major device number")
                               .parse().expect("Could not parse major number");
        let minor_num = columns.next().expect("Expected minor device number")
                               .parse().expect("Could not parse minor number");
        let name = columns.next().expect("Expected device name");
        Self {
            device_nums: DeviceNumbers { major: major_num, minor: minor_num },
            device_str: name,
            stats_columns: columns,
        }
    }
}
///
///
/// Device identifier based on major and minor device numbers
///
/// This maps to the dev_t type from the Linux kernel, but it uses 64 bits
/// instead of 32 bits in order to maximize the odds that this library will
/// still work under future kernel versions.
///
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DeviceNumbers {
    // Major device number, usually (but not always) maps to a kernel driver
    pub major: u32,

    // Minor device number, arbitrarily attributed by drivers to devices
    pub minor: u32,
}


// TODO: Rework storage as a dumb slave of the smart parser


/// Data samples from /proc/diskstats, in structure-of-array layout
///
/// TODO: Provide a more detailed description after implementation
///
struct DiskStatsData {
    /// List of iostat records following original file order (as in MemInfoData)
    records: Vec<DiskStatsRecord>,

    /// Device numbers associated with each record, again in file order
    device_numbers: Vec<DeviceNumbers>,

    /// Device names associated with each record, again in file order
    device_names: Vec<String>,
}
//
impl DiskStatsData {
    /// Create a new disk stats data store, using a first sample to know the
    /// structure of /proc/diskstats on this system
    fn new(initial_contents: &str) -> Self {
        // We only support the disktats format introduced by Linux 2.6.25, where
        // detailed statistics are provided for both disks and partitions
        assert!(LINUX_VERSION.greater_eq(2, 6, 25),
                "Unsupported diskstats format, please use Linux >= 2.6.25");

        // Our data store will eventually go there
        let mut data = Self {
            records: Vec::new(),
            device_numbers: Vec::new(),
            device_names: Vec::new(),
        };

        // For each line of the initial content of /proc/diskstats...
        let mut lines = SplitLinesBySpace::new(initial_contents);
        while let Some(mut columns) = lines.next() {
            // Extract and memorize the device identifiers
            {
                let (numbers, name) = Self::parse_device_ids(&mut columns);
                data.device_numbers.push(numbers);
                data.device_names.push(name.to_owned());
            }

            // Build a record associated with this block device
            data.records.push(DiskStatsRecord::new(columns));
        }

        // Return our data collection setup
        data
    }

    /// Parse the contents of /proc/diskstats and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, file_contents: &str) {
        // This time, we know how lines of /proc/diskstats should map to members
        let mut lines = SplitLinesBySpace::new(file_contents);
        for ((record, numbers), name) in self.records.iter_mut()
                                             .zip(self.device_numbers.iter())
                                             .zip(self.device_names.iter()) {
            // Iterate over lines, checking that each device record which we
            // observed during initialization is still around (otherwise, an
            // unsupported hotplug event has occurred).
            let mut columns = lines.next()
                                   .expect("A device record has disappeared");

            // Extract and check the device identifiers
            // (If they don't match, an unsupported hotplug event occurred)
            {
                let (numbers2, name2) = Self::parse_device_ids(&mut columns);
                assert_eq!(*numbers, numbers2, "Device numbers do not match");
                assert_eq!(name,     name2,    "Device name does not match");
            }

            // Forward the data to the record associated with this device
            record.push(columns);
        }

        // In debug mode, we also check that records did not appear out of blue
        debug_assert_eq!(lines.next(), None,
                         "A device record appeared out of nowhere");
    }

    /// Tell how many samples are present in the data store, and in debug mode
    /// check for internal data store consistency
    #[cfg(test)]
    fn len(&self) -> usize {
        // We'll return the length of the first record, if any, or else zero
        let length = self.records.first().map_or(0, |rec| rec.len());

        // In debug mode, check that all records have the same length
        debug_assert!(self.records.iter().all(|rec| rec.len() == length));

        // Return the number of samples in the data store
        length
    }
}
//
impl DiskStatsData {
    /// Parse the major/minor device numbers and the device name from the
    /// beginning of a record of /proc/diskstats
    fn parse_device_ids<'a>(columns: &'a mut SplitColumns) -> (DeviceNumbers,
                                                               &'a str) {
        // Extract the major device number
        let major = columns.next()
                           .expect("Major device number is missing")
                           .parse::<u32>()
                           .expect("Failed to parse major device number");

        // Extract the minor device number
        let minor = columns.next()
                           .expect("Minor device number is missing")
                           .parse::<u32>()
                           .expect("Failed to parse minor device number");

        // Extract the device name
        let name = columns.next()
                          .expect("Device name is missing");

        // Return all these informations
        (DeviceNumbers { major, minor }, name)
    }
}


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
        // TODO: Also take note of the sysfs facility for per-device stats
    },
}
//
impl DiskStatsRecord {
    /// Create a new record
    fn new(mut raw_data: SplitColumns) -> Self {
        // TODO
        unimplemented!()
    }

    /// Push new data inside of the record
    fn push(&mut self, mut raw_data: SplitColumns) {
        // TODO
        unimplemented!()
    }

    /// Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        // TODO
        unimplemented!()
    }
}


// TODO: Unit tests
// TODO: Including those from define_sampler_tests!


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
/// TODO: Switch to define_sampler_benchs! as soon as possible
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
