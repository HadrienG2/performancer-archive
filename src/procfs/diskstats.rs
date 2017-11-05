///! This module contains a sampling parser for /proc/diskstats

use ::data::SampledData;
use ::parser::PseudoFileParser;
use ::procfs::version::LINUX_VERSION;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use std::collections::HashMap;
use std::time::Duration;


// Implement a /proc/diskstats sampler using DiskStatsData for parsing & storage
/* define_sampler!{ Sampler : "/proc/diskstats" => Parser => Data } */


/// Incremental parser for /proc/diskstats
#[derive(Debug, PartialEq)]
pub struct Parser {
    // Record of previously observed counter values on each device, used for
    // handling of counter overflows on 32-bit platforms.
    previous_counter_vals: HashMap<DeviceNumbers, [u64; 10]>,
}
//
impl PseudoFileParser for Parser {
    /// Build a parser, using an initial file sample. Here, this is used to
    /// perform quick schema validation, just to maximize the odds that failure,
    /// if any, will occur at initialization time rather than run time.
    fn new(initial_contents: &str) -> Self {
        // We rely on the disk stats format that was introduced in Linux 2.6.25
        assert!(LINUX_VERSION.greater_eq(2, 6, 25),
            "Unsupported diskstats format, please use Linux >= 2.6.25");

        // Check that we can parse all records without issues
        let mut parser = Self { previous_counter_vals: HashMap::new() };
        {
            let mut records = parser.parse(initial_contents);
            while let Some(record) = records.next() {
                let _ = record.device_numbers();
                let _ = record.device_name();
                let _ = record.extract_statistics();
            }
        }
        parser
    }
}
//
// TODO: Implement CachingParser once that trait is usable in stable Rust
impl Parser {
    /// Parse a pseudo-file sample into a stream of records
    pub fn parse<'a, 'b>(&'a mut self,
                         file_contents: &'b str) -> RecordStream<'a, 'b>
    {
        RecordStream::new(self, file_contents)
    }
}
///
///
/// Stream of records from /proc/diskstats
///
/// This streaming iterator should yield a stream of disk stats records, each
/// representing a line of /proc/diskstats (i.e. statistics on a block device).
///
pub struct RecordStream<'a, 'b> {
    /// Parent parser struct
    parser: &'a mut Parser,

    /// Iterator into the lines and columns of /proc/diskstats
    file_lines: SplitLinesBySpace<'b>,
}
//
impl<'a, 'b> RecordStream<'a, 'b> {
    /// Parse the next record from /proc/diskstats into a stream of fields
    pub fn next<'c>(&'c mut self) -> Option<Record<'b, 'c>>
        where 'b: 'c
    {
        let parser = &mut self.parser;
        self.file_lines.next().map(move |cols| Record::new(parser, cols))
    }

    /// Create a record stream from raw contents
    fn new(parser: &'a mut Parser, file_contents: &'b str) -> Self {
        Self {
            parser,
            file_lines: SplitLinesBySpace::new(file_contents),
        }
    }
}
///
///
/// Record from /proc/diskstats (activity of one block device)
pub struct Record<'b, 'c> where 'b: 'c {
    /// Parent parser struct
    parser: &'c mut Parser,

    // Device numbers
    device_nums: DeviceNumbers,

    // Device name
    device_str: &'b str,

    // Unparsed device statistics
    stats_columns: SplitColumns<'b, 'c>,
}
//
impl<'b, 'c> Record<'b, 'c> {
    /// Query the device number
    fn device_numbers(&self) -> DeviceNumbers {
        self.device_nums
    }

    /// Query the device name
    fn device_name(&self) -> &str {
        self.device_str
    }

    /// Parse and return the statistics
    fn extract_statistics(self) -> Statistics {
        // First, fetch the last observed counter values from this device. If
        // none was observed, assume a last observed value of "all zeroes".
        let last_counters = self.parser.previous_counter_vals
                                       .entry(self.device_nums)
                                       .or_insert([0u64; 10]);
        Statistics::new(last_counters, self.stats_columns)
    }

    /// Construct a record from associated file columns
    fn new(parser: &'c mut Parser, mut columns: SplitColumns<'b, 'c>) -> Self {
        let major_num = columns.next().expect("Expected major device number")
                               .parse().expect("Could not parse major number");
        let minor_num = columns.next().expect("Expected minor device number")
                               .parse().expect("Could not parse minor number");
        let name = columns.next().expect("Expected device name");
        Self {
            parser,
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
///
///
/// Statistics for a given device
/// TODO: Once the basic thing works, try to make it faster by making it lazier.
struct Statistics {
    /// Total number of reads that completed successfully
    completed_reads: u64,

    /// Total number of adjacent reads that were merged by the kernel
    merged_reads: u64,

    /// Total number of drive sectors that were successfully read
    sector_reads: u64,

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
    total_read_time: Duration,

    /// Total number of writes that completed successfully
    completed_writes: u64,

    /// Total number of adjacent writes that were merged by the kernel
    merged_writes: u64,

    /// Total number of drive sectors that were successfully written
    sector_writes: u64,

    /// Total time spent writing data, as measured by summing the difference
    /// between the end and start time of all writes.
    ///
    /// The warning given about total_read_time also applies here.
    ///
    total_write_time: Duration,

    /// Number of IO operations that are in progress (queued or running)
    io_in_progress: usize,

    /// Total wall clock time spent performing IO
    ///
    /// This a measure of the wall clock time during which a nonzero amount
    /// of IO tasks were in progress (per the indicator above). This maps
    /// quite well to the time spent by the underlying hardware on IO...
    /// given the caveat that the kernel could delay the submission of
    /// queued IO requests for power management or throughput reasons.
    ///
    wall_clock_io_time: Duration,

    /// Weighted time spent performing IO
    ///
    /// On every update (which happens on various IO events), this timer is
    /// incremented by the time spent doing IO since the last update (per
    /// the wall_clock_io_time counter) times the amount of outstanding IO
    /// requests. This can be an indicator of IO pressure in the kernel.
    ///
    weighted_io_time: Duration,
}
//
impl Statistics {
    /// Parse device statistics, using knowledge of previous counter values for
    /// the sake of relatively sane overflow handling.
    fn new<'b, 'c>(last_counters: &'c mut [u64; 10],
                   columns: SplitColumns<'b, 'c>) -> Self {
        // All statistics should be integers of the machine's native word size
        let mut counter_vals_iter = columns.map(|col_str| {
            col_str.parse::<usize>().expect("Expected a native machine word")
        });

        // Some statistics can overflow, and we try to handle it well
        let unwrap_counter = |new_value: usize, last_counter: u64| -> u64 {
            // Find what was the last counter value that we observed
            let last_value = last_counter as usize;

            // If the new counter value is greater than the old one, assume that
            // no overflow occured and add the difference. Otherwise, assume
            // that a single overflow has occured, and add usize::max_value()
            // then substract the absolute difference. This computation can be
            // conveniently expressed using a wrapping substraction.
            last_counter + (new_value.wrapping_sub(last_value) as u64)
        };

        // But where do we get the last counter value, you may ask? Well, we get
        // it from a Parser-provided cache that we will update at the end.
        let mut last_counters_iter = last_counters.iter_mut();

        // In a nutshell, for every "counter" field (i.e. anything but
        // io_in_progress), we are going to do the following:
        let mut process_counter = |counter_val: Option<usize>| -> u64 {
            let new_counter_val =
                counter_val.expect("Missing statistic in /proc/diskstats");
            let last_completed_reads =
                last_counters_iter.next().expect("Missing cache for counter");
            let result = unwrap_counter(new_counter_val, *last_completed_reads);
            *last_completed_reads = result;
            result
        };

        // There are also counters of miliseconds out there, which we will
        // translate into a Duration for type safety and easier consumption.
        let process_duration_ms = |ms_counter: u64| -> Duration {
            let nanosecs = ((ms_counter % 1000) * 1_000_000) as u32;
            let whole_secs = ms_counter / 1000;
            Duration::new(whole_secs, nanosecs)
        };

        // And now, we have everything we need to translate all the statistics
        let completed_reads = process_counter(counter_vals_iter.next());
        let merged_reads = process_counter(counter_vals_iter.next());
        let sector_reads = process_counter(counter_vals_iter.next());
        let total_read_time_ms = process_counter(counter_vals_iter.next());
        let total_read_time = process_duration_ms(total_read_time_ms);
        let completed_writes = process_counter(counter_vals_iter.next());
        let merged_writes = process_counter(counter_vals_iter.next());
        let sector_writes = process_counter(counter_vals_iter.next());
        let total_write_time_ms = process_counter(counter_vals_iter.next());
        let total_write_time = process_duration_ms(total_write_time_ms);
        let io_in_progress =
            counter_vals_iter.next()
                             .expect("Missing statistic in /proc/diskstats");
        let wall_clock_io_time_ms = process_counter(counter_vals_iter.next());
        let wall_clock_io_time = process_duration_ms(wall_clock_io_time_ms);
        let weighted_io_time_ms = process_counter(counter_vals_iter.next());
        let weighted_io_time = process_duration_ms(weighted_io_time_ms);

        // And at the end, we put them all in a struct
        Self {
            completed_reads,
            merged_reads,
            sector_reads,
            total_read_time,
            completed_writes,
            merged_writes,
            sector_writes,
            total_write_time,
            io_in_progress,
            wall_clock_io_time,
            weighted_io_time,
        }
    }
}


/// Data samples from /proc/diskstats, in structure-of-array layout
///
/// TODO: Provide a more detailed description after implementation
///
struct Data {
    /// List of iostat records following original file order (as in MemInfoData)
    records: Vec<SampledStats>,

    /// Device numbers associated with each record, again in file order
    device_numbers: Vec<DeviceNumbers>,

    /// Device names associated with each record, again in file order
    device_names: Vec<String>,
}
//
impl SampledData for Data {
    /// Tell how many samples are present in the data store + check consistency
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
// TODO: Implement SampledData2 once that is usable in stable Rust
impl Data {
    /// Create a new disk stats data store, using a first sample to know the
    /// structure of /proc/diskstats on this system
    fn new(mut stream: RecordStream) -> Self {
        // Our data store will eventually go there
        let mut data = Self {
            records: Vec::new(),
            device_numbers: Vec::new(),
            device_names: Vec::new(),
        };

        // For each initial record of /proc/diskstats...
        while let Some(record) = stream.next() {
            // Extract and memorize the device identifiers
            data.device_numbers.push(record.device_numbers());
            data.device_names.push(record.device_name().to_owned());

            // Build a record associated with this block device
            data.records.push(SampledStats::new(record.extract_statistics()));
        }

        // Return our data collection setup
        data
    }

    /// Parse the contents of /proc/diskstats and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, mut stream: RecordStream) {
        // This time, we know how lines of /proc/diskstats should map to members
        for ((samples, numbers), name) in self.records.iter_mut()
                                              .zip(self.device_numbers.iter())
                                              .zip(self.device_names.iter()) {
            // Make sure that each device record which we observed during
            // initialization is still around (otherwise, an hotplug event has
            // occurred, and that is currently unsupported).
            let record = stream.next()
                               .expect("A device record has disappeared");

            // Extract and check the device identifiers
            // (If they don't match, an unsupported hotplug event occurred)
            assert_eq!(*numbers, record.device_numbers(),
                       "Device numbers do not match");
            assert_eq!(name, record.device_name(),
                       "Device name does not match");

            // Forward the data to the record associated with this device
            samples.push(record.extract_statistics());
        }

        // In debug mode, we also check that records did not appear out of blue
        debug_assert!(stream.next().is_none(),
                      "A device record appeared out of nowhere");
    }
}


/// Sampled records from /proc/diskstats, with a zero-record optimization
/// TODO: Decide whether code sharing with the interrupt sampler is worthwhile
/// TODO: This parser can also be used when parsing /sys/block/<device>/stat.
///       Do we want to implement support for that and make code reuse easy?
enum SampledStats {
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
impl SampledStats {
    /// Create a new record
    fn new(stats: Statistics) -> Self {
        // TODO
        unimplemented!()
    }

    /// Push new data inside of the record
    fn push(&mut self, stats: Statistics) {
        // TODO
        unimplemented!()
    }

    /// Tell how many samples are present in the data store
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
