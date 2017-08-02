//! This module contains a sampling parser for /proc/meminfo

use ::reader::ProcFileReader;
use ::splitter::SplitLinesBySpace;
use bytesize::ByteSize;
use std::collections::HashMap;
use std::io::Result;


/// Mechanism for sampling measurements from /proc/meminfo
pub struct MemInfoSampler {
    /// Reader object for /proc/stat
    reader: ProcFileReader,

    /// Sampled statistical data
    samples: MemInfoData,
}
//
impl MemInfoSampler {
    /// Create a new sampler of /proc/meminfo
    pub fn new() -> Result<Self> {
        let mut reader = ProcFileReader::open("/proc/meminfo")?;
        let mut first_readout = String::new();
        reader.sample(|file_contents| first_readout.push_str(file_contents))?;
        Ok(
            Self {
                reader,
                samples: MemInfoData::new(&first_readout),
            }
        )
    }

    /// Acquire a new sample of memory information data
    pub fn sample(&mut self) -> Result<()> {
        let samples = &mut self.samples;
        self.reader.sample(|file_contents: &str| samples.push(file_contents))
    }

    // TODO: Add accessors to the inner stat data + associated tests
}


/// Data samples from /proc/meminfo, in structure-of-array layout
///
/// As /proc/meminfo is basically a (large) set of named data volumes and
/// performance counters, it maps very well to a homogeneous collection (with
/// just an enum inside to disambiguate between volumes and counters).
///
/// There is, however, a catch: for fast sampling, we want to be able to iterate
/// over the records in the order in which they appear in /proc/meminfo. But for
/// fast lookup, we want to be able to quickly find a certain entry. We resolve
/// this dilemma by using a Vec for fast ordered access to the measurements
/// during sampling, and a HashSet index for fast key lookup.
///
#[derive(Debug, PartialEq)]
struct MemInfoData {
    /// Sampled meminfo records, in the order in which they appear in the file
    records: Vec<MemInfoRecord>,

    /// Hashed index mapping the meminfo keys to the associated records above
    index: HashMap<String, usize>,
}
//
impl MemInfoData {
    /// Create a new memory info data store, using a first sample to know the
    /// structure of /proc/meminfo on this system
    fn new(initial_contents: &str) -> Self {
        // Our data store will eventually go there
        let mut data = Self {
            records: Vec::new(),
            index: HashMap::new(),
        };

        // For each line of the initial content of /proc/meminfo...
        let mut splitter = SplitLinesBySpace::new(initial_contents);
        while splitter.next_line() {
            // ...and check that the header has the expected format. It should
            // consist of a non-empty string key, followed by a colon, which we
            // shall get rid of along the way.
            let mut header = splitter.next()
                                     .expect("Unexpected empty line")
                                     .to_owned();
            assert_eq!(header.pop(), Some(':'),
                       "meminfo headers should end with a colon");

            // Build a record for this line of /proc/meminfo
            let record = MemInfoRecord::new(&mut splitter);

            // Report unsupported records in debug mode
            debug_assert!(record != MemInfoRecord::Unsupported(0),
                          "Missing support for meminfo record named {}",
                          header);

            // Store record in our internal data store and index it
            let record_index = data.records.len();
            data.records.push(record);
            let duplicate_entry = data.index.insert(header, record_index);

            // No pair of entries in /proc/meminfo should have the same name
            assert_eq!(duplicate_entry, None,
                       "Duplicated meminfo entry detected");
        }

        // Return our data collection setup
        data
    }

    /// Parse the contents of /proc/meminfo and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, file_contents: &str) {
        // This time, we know how lines of /proc/meminfo map to our members
        let mut splitter = SplitLinesBySpace::new(file_contents);
        for record in self.records.iter_mut() {
            // The beginning of parsing is the same as before: split by spaces.
            // But this time, we discard the header, as we already know it.
            assert!(splitter.next_line(), "A meminfo record has disappeared");
            splitter.next();

            // Forward the data to the appropriate parser
            record.push(&mut splitter);
        }
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


/// Sampled records from /proc/meminfo, which can measure different things:
#[derive(Debug, PartialEq)]
enum MemInfoRecord {
    /// A volume of data
    DataVolume(Vec<ByteSize>),

    /// A raw counter of something (e.g. free huge pages)
    Counter(Vec<u64>),

    /// Something unsupported by this parser :-(
    ///
    /// When we encounter this case, we just count the amount of samples that we
    /// encountered. It makes things easier, and won't make the enum any larger.
    ///
    Unsupported(usize),
}
//
impl MemInfoRecord {
    /// Create a new record, choosing the type based on some raw data
    fn new(raw_data: &mut SplitLinesBySpace) -> Self {
        // The raw data should start with a numerical field. Make sure that we
        // can parse it. Otherwise, we don't support the associated content.
        let number_parse_result = raw_data.next()
                                          .expect("Unexpected blank record")
                                          .parse::<u64>();

        // The number may or may not come with a suffix which clarifies its
        // semantics: is it just a raw counter, or some volume of data?
        match (number_parse_result, raw_data.next()) {
            // It's a volume of data (in KiB, don't trust the kernel's units...)
            (Ok(_), Some("kB")) => {
                debug_assert_eq!(raw_data.next(), None);
                MemInfoRecord::DataVolume(Vec::new())
            },

            // It's a raw counter without any special semantics attached to it
            (Ok(_), None) => MemInfoRecord::Counter(Vec::new()),

            // It's something we don't know how to parse
            _ => MemInfoRecord::Unsupported(0),
        }
    }

    /// Push new data inside of the record
    fn push(&mut self, raw_data: &mut SplitLinesBySpace) {
        // Use our knowledge from the first parse to tell what this should be
        match *self {
            // A data volume in kibibytes
            MemInfoRecord::DataVolume(ref mut v) => {
                // Parse and record the data volume
                let data_volume = ByteSize::kib(
                    raw_data.next().expect("Unexpected empty record")
                            .parse().expect("Failed to parse data volume")
                );
                v.push(data_volume);

                // Check that meminfo schema hasn't changed in debug mode
                debug_assert_eq!(raw_data.next(), Some("kB"));
                debug_assert_eq!(raw_data.next(), None);
            },

            // A raw counter
            MemInfoRecord::Counter(ref mut v) => {
                // Parse and record the counter's value
                v.push(raw_data.next().expect("Unexpected empty record")
                               .parse().expect("Failed to parse counter"));

                // Check that meminfo schema hasn't changed in debug mode
                debug_assert_eq!(raw_data.next(), None);
            },

            // Something unknown and mysterious
            MemInfoRecord::Unsupported(ref mut count) => {
                *count += 1;
            },
        }
    }

    /// Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        match *self {
            MemInfoRecord::DataVolume(ref v)  => v.len(),
            MemInfoRecord::Counter(ref v)     => v.len(),
            MemInfoRecord::Unsupported(count) => count,
        }
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use ::splitter::split_line;
    use super::{ByteSize, MemInfoData, MemInfoRecord, MemInfoSampler};

    /// Check that meminfo record initialization works well
    #[test]
    fn init_record() {
        // Data volume record
        let data_vol_record = MemInfoRecord::new(&mut split_line("42 kB"));
        assert_eq!(data_vol_record, MemInfoRecord::DataVolume(Vec::new()));
        assert_eq!(data_vol_record.len(), 0);

        // Counter record
        let counter_record = MemInfoRecord::new(&mut split_line("713705"));
        assert_eq!(counter_record, MemInfoRecord::Counter(Vec::new()));
        assert_eq!(counter_record.len(), 0);

        // Unsupported record
        let bad_record = MemInfoRecord::new(&mut split_line("73 MiB"));
        assert_eq!(bad_record, MemInfoRecord::Unsupported(0));
        assert_eq!(bad_record.len(), 0);
    }

    /// Check that meminfo record parsing works well
    #[test]
    fn parse_record() {
        // Data volume record
        let mut size_record = MemInfoRecord::new(&mut split_line("24 kB"));
        size_record.push(&mut split_line("512 kB"));
        assert_eq!(size_record,
                   MemInfoRecord::DataVolume(vec![ByteSize::kib(512)]));
        assert_eq!(size_record.len(), 1);

        // Counter record
        let mut counter_record = MemInfoRecord::new(&mut split_line("1337"));
        counter_record.push(&mut split_line("371830"));
        assert_eq!(counter_record, MemInfoRecord::Counter(vec![371830]));
        assert_eq!(counter_record.len(), 1);

        // Unsupported record
        let mut bad_record = MemInfoRecord::new(&mut split_line("57 TiB"));
        bad_record.push(&mut split_line("332 PiB"));
        assert_eq!(bad_record, MemInfoRecord::Unsupported(1));
        assert_eq!(bad_record.len(), 1);
    }

    /// Check that meminfo data initialization works as expected
    #[test]
    fn init_meminfo_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut info = String::new();
        let empty_info = MemInfoData::new(&info);
        assert_eq!(empty_info.records.len(), 0);
        assert_eq!(empty_info.index.len(), 0);
        assert_eq!(empty_info.len(), 0);
        let mut expected = empty_info;

        // ...adding a first line of memory info...
        info.push_str("MyDataVolume:   1234 kB");
        let single_info = MemInfoData::new(&info);
        expected.records.push(MemInfoRecord::DataVolume(Vec::new()));
        expected.index.insert("MyDataVolume".to_owned(), 0);
        assert_eq!(single_info, expected);
        assert_eq!(expected.len(), 0);

        // ...and a second line of memory info.
        info.push_str("\nMyCounter:   42");
        let double_info = MemInfoData::new(&info);
        expected.records.push(MemInfoRecord::Counter(Vec::new()));
        expected.index.insert("MyCounter".to_owned(), 1);
        assert_eq!(double_info, expected);
        assert_eq!(expected.len(), 0);
    }

    /// Check that meminfo data parsing works well
    #[test]
    fn parse_meminfo_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut info = String::new();
        let mut empty_info = MemInfoData::new(&info);
        empty_info.push(&info);
        let mut expected = MemInfoData::new(&info);
        assert_eq!(empty_info, expected);

        // ...adding a first line of memory info...
        info.push_str("MyDataVolume:   1234 kB");
        let mut single_info = MemInfoData::new(&info);
        single_info.push(&info);
        expected = MemInfoData::new(&info);
        expected.records[0].push(&mut split_line("1234 kB"));
        assert_eq!(single_info, expected);
        assert_eq!(expected.len(), 1);

        // ...and a second line of memory info.
        info.push_str("\nMyCounter:   42");
        let mut double_info = MemInfoData::new(&info);
        double_info.push(&info);
        expected = MemInfoData::new(&info);
        expected.records[0].push(&mut split_line("1234 kB"));
        expected.records[1].push(&mut split_line("42"));
        assert_eq!(double_info, expected);
        assert_eq!(expected.len(), 1);
    }

    /// Check that sampler initialization works well
    #[test]
    fn init_sampler() {
        let stats =
            MemInfoSampler::new()
                           .expect("Failed to create a /proc/meminfo sampler");
        assert_eq!(stats.samples.len(), 0);
    }

    /// Check that basic sampling works as expected
    #[test]
    fn basic_sampling() {
        let mut stats =
            MemInfoSampler::new()
                           .expect("Failed to create a /proc/meminfo sampler");
        stats.sample().expect("Failed to sample meminfo once");
        assert_eq!(stats.samples.len(), 1);
        stats.sample().expect("Failed to sample meminfo twice");
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
    use super::MemInfoSampler;
    use testbench;

    /// Benchmark for the raw meminfo readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader =
            ProcFileReader::open("/proc/meminfo")
                           .expect("Failed to open memory info");
        testbench::benchmark(500_000, || {
            reader.sample(|_| {}).expect("Failed to read memory info");
        });
    }

    /// Benchmark for the full meminfo sampling overhead
    #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut stat =
            MemInfoSampler::new()
                           .expect("Failed to create a memory info sampler");
        testbench::benchmark(500_000, || {
            stat.sample().expect("Failed to sample memory info");
        });
    }
}
