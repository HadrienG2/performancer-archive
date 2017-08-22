//! This module contains a sampling parser for /proc/meminfo

use ::sampler::PseudoFileParser;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use bytesize::ByteSize;

// Implement a sampler for /proc/meminfo using MemInfoData for parsing & storage
define_sampler!{ MemInfoSampler : "/proc/meminfo" => MemInfoData }


/// Streaming parser for /proc/meminfo
///
/// TODO: Decide whether a more extensive description is needed
///
struct MemInfoParser {}
//
impl MemInfoParser {
    /// Build a parser, using initial file contents for schema analysis
    fn new(initial_contents: &str) -> Self {
        unimplemented!()
    }

    /// Begin to parse a pseudo-file sample, streaming its data out
    fn parse<'a>(&mut self, file_contents: &'a str) -> MemInfoStream<'a> {
        unimplemented!()
    }
}
///
///
/// Stream of records from /proc/meminfo
///
/// This iterator should yield a stream of memory info records, each featuring
/// a named counter or data volume.
///
struct MemInfoStream<'a> {
    /// Iterator into the lines and columns of /proc/meminfo
    file_lines: SplitLinesBySpace<'a>,
}
//
impl<'a> MemInfoStream<'a> {
    /// Parse the next record from /proc/meminfo
    fn next<'b>(&'b mut self) -> Option<MemInfoRecordStream<'a, 'b>>
        where 'a: 'b
    {
        unimplemented!()
    }
}
///
///
/// Streamed record from /proc/meminfo
///
/// This iterator should successively yield...
///
/// * A string label identifying this record
/// * Either a data volume, a counter, or an unsupported data marker
/// * A None terminator
///
struct MemInfoRecordStream<'a, 'b> where 'a: 'b {
    /// Iterator into the columns of the active record
    file_columns: SplitColumns<'a, 'b>,
}
//
impl<'a, 'b> MemInfoRecordStream<'a, 'b> {
    /// Analyze the next field of the meminfo record
    fn next(&mut self) -> Option<MemInfoRecordField<'a>> {
        unimplemented!()
    }
}
///
///
/// Streamed field from a meminfo record
enum MemInfoRecordField<'a> {
    /// Header of the record
    Header(&'a str),

    /// Data volume
    DataVolume(ByteSize),

    /// Raw integer counter
    Counter(u64),

    /// Some payload unsupported by this parser :-(
    UnsupportedPayload,
}


/// Data samples from /proc/meminfo, in structure-of-array layout
///
/// As /proc/meminfo is just a (large) set of named data volumes with a few
/// performance counters sprinkled in the middle, it maps very well to a
/// vector of enums.
///
/// When it comes to keys, the current layout is optimized for fast sampling
/// with key checking, rather than fast lookup of a specific key. If clients
/// expect to frequently need a mapping of key to records, they are encouraged
/// to build and use a HashMap for this purpose.
///
#[derive(Debug, PartialEq)]
struct MemInfoData {
    /// Sampled meminfo records, in the order in which they appear in the file
    records: Vec<MemInfoRecord>,

    /// Keys associated with each record, again in file order
    keys: Vec<String>,
}
//
impl PseudoFileParser for MemInfoData {
    /// Create a new memory info data store, using a first sample to know the
    /// structure of /proc/meminfo on this system
    fn new(initial_contents: &str) -> Self {
        // Our data store will eventually go there
        let mut data = Self {
            records: Vec::new(),
            keys: Vec::new(),
        };

        // For each line of the initial content of /proc/meminfo...
        let mut lines = SplitLinesBySpace::new(initial_contents);
        while let Some(mut columns) = lines.next() {
            // ...and check that the header has the expected format. It should
            // consist of a non-empty string key, followed by a colon, which we
            // shall get rid of along the way.
            let mut header = columns.next()
                                    .expect("Unexpected empty line")
                                    .to_owned();
            assert_eq!(header.pop(), Some(':'),
                       "Headers from meminfo should end with a colon");

            // Build a record for this line of /proc/meminfo
            let record = MemInfoRecord::new(columns);

            // Report unsupported records in debug mode
            debug_assert!(record != MemInfoRecord::Unsupported(0),
                          "Missing support for a meminfo record named {}",
                          header);

            // Memorize the record and its key in our data store
            data.records.push(record);
            data.keys.push(header);
        }

        // Return our data collection setup
        data
    }

    /// Parse the contents of /proc/meminfo and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, file_contents: &str) {
        // This time, we know how lines of /proc/meminfo map to our members
        let mut lines = SplitLinesBySpace::new(file_contents);
        for (record, key) in self.records.iter_mut().zip(self.keys.iter()) {
            // We start by iterating over lines and checking that each line
            // that we observed during initialization is still around
            let mut columns = lines.next()
                                   .expect("A meminfo record has disappeared");
            let header = columns.next().expect("Unexpected empty line");

            // In release mode, we use the length of the header as a checksum
            // to make sure that the internal structure did not change during
            // sampling. In debug mode, we fully check the header.
            assert_eq!(header.len()-1, key.len(),
                       "Unsupported structural meminfo change during sampling");
            debug_assert_eq!(&header[..header.len()-1], key,
                             "Unsupported meminfo change during sampling");

            // Forward the data to the appropriate parser
            record.push(columns);
        }

        // In debug mode, we also check that records did not appear out of blue
        debug_assert_eq!(lines.next(), None,
                         "A meminfo record appeared out of nowhere");
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
    fn new(mut raw_data: SplitColumns) -> Self {
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
    fn push(&mut self, mut raw_data: SplitColumns) {
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

    /// For testing purposes, pushing in a string can be more convenient
    #[cfg(test)]
    fn push_str(&mut self, raw_data: &str) {
        use ::splitter::split_line_and_run;
        split_line_and_run(raw_data, |columns| self.push(columns))
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
    use ::splitter::split_line_and_run;
    use super::{ByteSize, MemInfoData, MemInfoRecord, PseudoFileParser};

    /// Check that meminfo record initialization works well
    #[test]
    fn init_record() {
        // Data volume record
        let data_vol_record = build_record("42 kB");
        assert_eq!(data_vol_record, MemInfoRecord::DataVolume(Vec::new()));
        assert_eq!(data_vol_record.len(), 0);

        // Counter record
        let counter_record = build_record("713705");
        assert_eq!(counter_record, MemInfoRecord::Counter(Vec::new()));
        assert_eq!(counter_record.len(), 0);

        // Unsupported record
        let bad_record = build_record("73 MiB");
        assert_eq!(bad_record, MemInfoRecord::Unsupported(0));
        assert_eq!(bad_record.len(), 0);
    }

    /// Check that meminfo record parsing works well
    #[test]
    fn parse_record() {
        // Data volume record
        let mut size_record = build_record("24 kB");
        size_record.push_str("512 kB");
        assert_eq!(size_record,
                   MemInfoRecord::DataVolume(vec![ByteSize::kib(512)]));
        assert_eq!(size_record.len(), 1);

        // Counter record
        let mut counter_record = build_record("1337");
        counter_record.push_str("371830");
        assert_eq!(counter_record,
                   MemInfoRecord::Counter(vec![371830]));
        assert_eq!(counter_record.len(), 1);

        // Unsupported record
        let mut bad_record = build_record("57 TiB");
        bad_record.push_str("332 PiB");
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
        assert_eq!(empty_info.keys.len(), 0);
        assert_eq!(empty_info.len(), 0);
        let mut expected = empty_info;

        // ...adding a first line of memory info...
        info.push_str("MyDataVolume:   1234 kB");
        let single_info = MemInfoData::new(&info);
        expected.records.push(MemInfoRecord::DataVolume(Vec::new()));
        expected.keys.push("MyDataVolume".to_owned());
        assert_eq!(single_info, expected);
        assert_eq!(expected.len(), 0);

        // ...and a second line of memory info.
        info.push_str("\nMyCounter:   42");
        let double_info = MemInfoData::new(&info);
        expected.records.push(MemInfoRecord::Counter(Vec::new()));
        expected.keys.push("MyCounter".to_owned());
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
        expected.records[0].push_str("1234 kB");
        assert_eq!(single_info, expected);
        assert_eq!(expected.len(), 1);

        // ...and a second line of memory info.
        info.push_str("\nMyCounter:   42");
        let mut double_info = MemInfoData::new(&info);
        double_info.push(&info);
        expected = MemInfoData::new(&info);
        expected.records[0].push_str("1234 kB");
        expected.records[1].push_str("42");
        assert_eq!(double_info, expected);
        assert_eq!(expected.len(), 1);
    }

    /// Check that the sampler works well
    define_sampler_tests!{ super::MemInfoSampler }

    /// INTERNAL: Build a MemInfoRecord using columns from a certain string
    fn build_record(input: &str) -> MemInfoRecord {
        split_line_and_run(input, |columns| MemInfoRecord::new(columns))
    }
}


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    define_sampler_benchs!{ super::MemInfoSampler,
                            "/proc/meminfo",
                            500_000 }
}
