//! This module contains a sampling parser for /proc/meminfo

use ::ProcFileReader;
use bytesize::ByteSize;
use parsers::SplitSpace;
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
    /// Create a new sampler of /proc/stat
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

    // TODO: Acquire a new sample of statistical data
    /* pub fn sample(&mut self) -> Result<()> {
        let samples = &mut self.samples;
        self.reader.sample(|file_contents: &str| samples.push(file_contents))
    } */

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
    // Sampled meminfo records, in the order in which they appear in the file
    records: Vec<MemInfoRecord>,

    // Hashed index mapping the meminfo keys to the associated records above
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
        for line in initial_contents.lines() {
            // ...decompose according to whitespace...
            let mut whitespace_iter = SplitSpace::new(line);

            // ...and check that the header has the expected format. It should
            // consist of a non-empty string key, followed by a colon, which we
            // shall get rid of along the way.
            let mut header = whitespace_iter.next()
                                            .expect("Unexpected empty line")
                                            .to_owned();
            assert_eq!(header.pop(), Some(':'),
                       "meminfo headers should end with a colon");

            // Build a record for this line of /proc/meminfo
            let record = MemInfoRecord::new(whitespace_iter);

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

    // TODO: Add a way to push data in

    // Tell how many samples are present in the data store, and in debug mode
    // check for internal data store consistency
    #[allow(dead_code)]
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
    // A volume of data
    DataVolume(Vec<ByteSize>),

    // A raw counter of something (e.g. free huge pages)
    Counter(Vec<u64>),

    // Something unsupported by this parser :-(
    //
    // When we encounter this case, we just count the amount of samples that we
    // encountered. It makes things easier, and won't make the enum any larger.
    //
    Unsupported(usize),
}
//
impl MemInfoRecord {
    // Create a new record, choosing the type based on some raw data
    fn new(mut raw_data: SplitSpace) -> Self {
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

    // TODO: Add a way to push data in

    /// Tell how many samples are present in the data store
    #[allow(dead_code)]
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
    use super::{MemInfoRecord, MemInfoSampler, SplitSpace};

    // Check that meminfo record initialization works well
    #[test]
    fn init_record() {
        // Data volume record
        let data_volume_record = MemInfoRecord::new(SplitSpace::new("42 kB"));
        assert_eq!(data_volume_record, MemInfoRecord::DataVolume(Vec::new()));
        assert_eq!(data_volume_record.len(), 0);

        // Counter record
        let counter_record = MemInfoRecord::new(SplitSpace::new("713705"));
        assert_eq!(counter_record, MemInfoRecord::Counter(Vec::new()));
        assert_eq!(counter_record.len(), 0);

        // Unsupported record
        let unsupported_record = MemInfoRecord::new(SplitSpace::new("73 MiB"));
        assert_eq!(unsupported_record, MemInfoRecord::Unsupported(0));
        assert_eq!(unsupported_record.len(), 0);
    }

    // TODO: Check that meminfo record parsing works well
    // TODO: Check that meminfo data initialization works well
    // TODO: Check that meminfo data parsing works well

    // Check that sampler initialization works well
    #[test]
    fn init_sampler() {
        let stats =
            MemInfoSampler::new()
                           .expect("Failed to create a /proc/meminfo sampler");
        assert_eq!(stats.samples.len(), 0);
    }

    // TODO: Check that basic sampling works as expected
}


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    use ::ProcFileReader;
    use testbench;

    /// Benchmark for the raw meminfo readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader =
            ProcFileReader::open("/proc/meminfo")
                           .expect("Failed to open /proc/meminfo");
        testbench::benchmark(400_000, || {
            reader.sample(|_| {}).expect("Failed to read /proc/meminfo");
        });
    }

    // TODO: Benchmark for the full meminfo sampling overhead
}
