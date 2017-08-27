//! This module contains a sampling parser for /proc/meminfo

use ::sampler::PseudoFileParser;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use bytesize::ByteSize;
use std::iter::Fuse;

// Implement a sampler for /proc/meminfo using MemInfoData for parsing & storage
define_sampler!{ MemInfoSampler : "/proc/meminfo" => MemInfoData }


/// Streaming parser for /proc/meminfo
///
/// TODO: Decide whether a more extensive description is needed
///
#[derive(Debug, PartialEq)]
pub struct MemInfoParser {}
//
impl MemInfoParser {
    /// Build a parser, using initial file contents for schema analysis
    pub fn new(_initial_contents: &str) -> Self {
        Self {}
    }

    /// Begin to parse a pseudo-file sample, streaming its data out
    pub fn parse<'a>(&mut self, file_contents: &'a str) -> MemInfoStream<'a> {
        MemInfoStream {
            file_lines: SplitLinesBySpace::new(file_contents),
        }
    }
}
///
///
/// Stream of records from /proc/meminfo
///
/// This iterator should yield a stream of memory info records, each featuring
/// a named counter or data volume.
///
pub struct MemInfoStream<'a> {
    /// Iterator into the lines and columns of /proc/meminfo
    file_lines: SplitLinesBySpace<'a>,
}
//
impl<'a> MemInfoStream<'a> {
    /// Parse the next record from /proc/meminfo
    pub fn next<'b>(&'b mut self) -> Option<MemInfoRecordStream<'a, 'b>>
        where 'a: 'b
    {
        self.file_lines.next().map(MemInfoRecordStream::new)
    }
}
///
///
/// Streamed reader for a record from /proc/meminfo
///
/// This streaming reader should successively yield...
///
/// * A string label, identifying this record
/// * A payload, which is either a data volume or a counter
///
/// Unsupported payload formats are detected and reported appropriately
///
pub struct MemInfoRecordStream<'a, 'b> where 'a: 'b {
    /// Fused iterator into the columns of the active record
    fused_columns: Fuse<SplitColumns<'a, 'b>>,

    /// Buffer of previously iterated columns
    last_columns: [Option<&'a str>; 2],

    /// State of the meminfo record iterator
    state: MemInfoRecordState,
}
//
impl<'a, 'b> MemInfoRecordStream<'a, 'b> {
    /// Read the next field of the meminfo record
    ///
    /// This method is designed so that it can be immediately chained with
    /// kind() in order to analyze what the new field contains, or with the
    /// appropriate parse_xyz() method in order to parse the freshly received
    /// data into a type that is already known.
    ///
    /// Since in the case of /proc/meminfo, the number of fields in a record
    /// is known at compile time, past the end iteration is considered to be
    /// a usage error and not supported in the interface.
    ///
    pub fn fetch(&mut self) -> &mut Self {
        match self.state {
            // Fetch the textual label of the record
            MemInfoRecordState::AtStart => {
                self.last_columns[0] = self.fused_columns.next();
                self.state = MemInfoRecordState::AtLabel;
            },

            // Fetch the payload of the record (quantity being measured)
            MemInfoRecordState::AtLabel => {
                self.last_columns[0] = self.fused_columns.next();
                self.last_columns[1] = self.fused_columns.next();
                self.state = MemInfoRecordState::AtPayload;
            },

            // There should be nothing after the record's payload
            MemInfoRecordState::AtPayload => {
                panic!("No record field expected after the payload");
            },
        }
        self
    }

    /// Analyze the active meminfo record field
    ///
    /// Run this method after a fetch() in order to validate your input data
    /// and eliminate schema ambiguities. Once you know about the contents of a
    /// certain meminfo record, you can skip this step and go for the
    /// appropriate parse_xyz method directly for better performance.
    ///
    pub fn kind(&self) -> MemInfoFieldKind {
        match self.state {
            // No data was loaded yet, this call is mistaken
            MemInfoRecordState::AtStart => panic!("Please call fetch() first"),

            // A meminfo record label was just loaded, validate it
            MemInfoRecordState::AtLabel => {
                // A valid label (with a trailing colon) should be present
                let has_valid_label =
                    self.last_columns[0]
                        .as_ref()
                        .map_or(false,
                                |lbl| lbl.bytes().next_back() == Some(b':'));

                // Tell whether a valid label was present in the input
                if has_valid_label {
                    MemInfoFieldKind::Label
                } else {
                    MemInfoFieldKind::Unsupported
                }
            },

            // A payload was just loaded, validate it and disambiguate what kind
            // of payload we're dealing with (data volume or raw counter?)
            MemInfoRecordState::AtPayload => {
                // A valid payload should start with a positive integer
                let has_valid_ctr = self.last_columns[0]
                                        .as_ref()
                                        .map_or(false,
                                                |s| s.parse::<u64>().is_ok());

                // Payload types are further disambiguated by the presence or
                // absence of a supported unit suffix
                match (has_valid_ctr, self.last_columns[1]) {
                    (true, Some("kB")) => MemInfoFieldKind::DataVolume,
                    (true, None)       => MemInfoFieldKind::Counter,
                    _                  => MemInfoFieldKind::Unsupported,
                }
            },
        }
    }

    /// Parse the current meminfo record field as a label
    pub fn parse_label(&mut self) -> &'a str {
        // In debug mode, validate that we are indeed on a label
        debug_assert_eq!(self.kind(), MemInfoFieldKind::Label);

        // Fetch the label from our column buffer (and reset the buffer)
        let label = self.last_columns[0]
                        .take()
                        .expect("No input value. Did you call fetch()?");

        // Eliminate the trailing colon of the label from our output
        &label[..label.len()-1]
    }

    /// Parse the current meminfo record field as a data volume
    pub fn parse_data_volume(&mut self) -> ByteSize {
        // In debug mode, validate that we are indeed on a data volume
        debug_assert_eq!(self.kind(), MemInfoFieldKind::DataVolume);

        // If we truly are on a data volume, it should be in our buffers
        let kibs_str_opt = self.last_columns[0].take();

        // Parse data volume, which is in kibibytes (no matter what Linux says)
        let data_volume = ByteSize::kib(
            kibs_str_opt.expect("No input value. Did you call fetch()?")
                        .parse::<usize>()
                        .expect("Could not parse the kibibytes counter.")
        );

        // Return the parsed data volume to our caller
        data_volume
    }

    /// Parse the current meminfo record field as a raw counter
    pub fn parse_counter(&mut self) -> u64 {
        // In debug mode, validate that we are indeed on a data volume
        debug_assert_eq!(self.kind(), MemInfoFieldKind::Counter);

        // If we truly are on a counter, it should be in our buffers
        let counter_str_opt = self.last_columns[0].take();

        // Parse the counter's value
        let counter =
            counter_str_opt.expect("No input value. Did you call fetch()?")
                           .parse::<u64>()
                           .expect("Could not parse the raw counter");

        // Return the parsed counter value to our client
        counter
    }

    /// Constructor a new record stream from associated file columns
    fn new(file_columns: SplitColumns<'a, 'b>) -> Self {
        Self {
            fused_columns: file_columns.fuse(),
            last_columns: [None; 2],
            state: MemInfoRecordState::AtStart,
        }
    }
}
///
/// Fields of a meminfo record can feature different kinds of data
#[derive(Debug, PartialEq)]
pub enum MemInfoFieldKind {
    /// Textual identifier of the record
    Label,

    /// Volume of data
    DataVolume,

    /// Raw integer counter
    Counter,

    /// Some payload unsupported by this parser :-(
    Unsupported,
}
///
/// State of a meminfo record streamer
enum MemInfoRecordState { AtStart, AtLabel, AtPayload }


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
    /// Sampled meminfo payloads, in the order in which it appears in the file
    data: Vec<MemInfoPayloads>,

    /// Keys associated with each record, again in file order
    keys: Vec<String>,

    /// New-style meminfo parser
    /// TODO: Just a check that it works, move to new-style sampler after that
    parser: MemInfoParser,
}
//
impl PseudoFileParser for MemInfoData {
    /// Create a new memory info data store, using a first sample to know the
    /// structure of /proc/meminfo on this system
    fn new(initial_contents: &str) -> Self {
        // Our data store will eventually go there
        let mut store = Self {
            data: Vec::new(),
            keys: Vec::new(),
            parser: MemInfoParser::new(initial_contents),
        };

        // For initial record of /proc/meminfo...
        let mut stream = store.parser.parse(initial_contents);
        while let Some(mut record) = stream.next() {
            // Fetch the record's label
            record.fetch();
            assert_eq!(record.kind(), MemInfoFieldKind::Label,
                       "Expected a meminfo record label");
            let label = record.parse_label();

            // Analyze the record's data payload
            let data = MemInfoPayloads::new(record);

            // Report unsupported payloads in debug mode
            debug_assert!(data != MemInfoPayloads::Unsupported(0),
                          "Missing support for a meminfo record named {}",
                          label);

            // Memorize the key and payload store in our data store
            store.keys.push(label.to_owned());
            store.data.push(data);
        }

        // Return our data collection setup
        store
    }

    /// Parse the contents of /proc/meminfo and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, file_contents: &str) {
        // This time, we know how lines of /proc/meminfo map to our members
        let mut stream = self.parser.parse(file_contents);
        for (data, key) in self.data.iter_mut().zip(self.keys.iter()) {
            // We start by iterating over records and checking that each record
            // that we observed during initialization is still around
            let mut record = stream.next()
                                   .expect("A meminfo record has disappeared");
            let label = record.fetch().parse_label();

            // In release mode, we use the length of the header as a checksum
            // to make sure that the internal structure did not change during
            // sampling. In debug mode, we fully check the header.
            assert_eq!(label.len(), key.len(),
                       "Unsupported structural meminfo change during sampling");
            debug_assert_eq!(label, key,
                             "Unsupported meminfo change during sampling");

            // Forward the payload to its target
            data.push(record);
        }

        // In debug mode, we also check that records did not appear out of blue
        debug_assert!(stream.next().is_none(),
                      "A meminfo record appeared out of nowhere");
    }

    /// Tell how many samples are present in the data store, and in debug mode
    /// check for internal data store consistency
    #[cfg(test)]
    fn len(&self) -> usize {
        // We'll return the length of the first record, if any, or else zero
        let length = self.data.first().map_or(0, |rec| rec.len());

        // In debug mode, check that all records have the same length
        debug_assert!(self.data.iter().all(|rec| rec.len() == length));

        // Return the number of samples in the data store
        length
    }
}


/// Sampled payloads from /proc/meminfo, which can measure different things:
#[derive(Debug, PartialEq)]
enum MemInfoPayloads {
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
impl MemInfoPayloads {
    /// Create a new payload, choosing the type based on some sample record
    fn new(mut record: MemInfoRecordStream) -> Self {
        match record.fetch().kind() {
            // Parser yielded a volume of data
            MemInfoFieldKind::DataVolume => {
                MemInfoPayloads::DataVolume(Vec::new())
            },

            // Parser yielded a raw counter without special semantics
            MemInfoFieldKind::Counter => {
                MemInfoPayloads::Counter(Vec::new())
            },

            // Parser failed to recognize the inner data type
            MemInfoFieldKind::Unsupported => {
                MemInfoPayloads::Unsupported(0)
            },

            // Parser yielded a label (=> upstream MemInfoData messed up)
            MemInfoFieldKind::Label => {
                panic!("Meminfo record label should already have been fetched")
            },
        }
    }

    /// Push new data inside of the payload table
    fn push(&mut self, mut record: MemInfoRecordStream) {
        // Use our knowledge from the first parse to tell what this should be
        record.fetch();
        match *self {
            // A data volume in kibibytes
            MemInfoPayloads::DataVolume(ref mut v) => {
                v.push(record.parse_data_volume());
            },

            // A raw counter
            MemInfoPayloads::Counter(ref mut v) => {
                v.push(record.parse_counter());
            },

            // Something unknown and mysterious
            MemInfoPayloads::Unsupported(ref mut count) => {
                *count += 1;
            },
        }
    }

    /* /// For testing purposes, pushing in a string can be more convenient
    #[cfg(test)]
    fn push_str(&mut self, raw_data: &str) {
        use ::splitter::split_line_and_run;
        split_line_and_run(raw_data, |columns| self.push(columns))
    } */

    /// Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        match *self {
            MemInfoPayloads::DataVolume(ref v)  => v.len(),
            MemInfoPayloads::Counter(ref v)     => v.len(),
            MemInfoPayloads::Unsupported(count) => count,
        }
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use ::splitter::split_line_and_run;
    use super::{ByteSize, MemInfoData, MemInfoPayloads, MemInfoRecordStream,
                MemInfoRecordState, PseudoFileParser};

    // TODO: Tests need to be completely reviewed :(

    /* /// Check that meminfo record initialization works well
    #[test]
    fn init_record() {
        // Data volume record
        let data_vol_record = build_record("42 kB");
        assert_eq!(data_vol_record, MemInfoPayloads::DataVolume(Vec::new()));
        assert_eq!(data_vol_record.len(), 0);

        // Counter record
        let counter_record = build_record("713705");
        assert_eq!(counter_record, MemInfoPayloads::Counter(Vec::new()));
        assert_eq!(counter_record.len(), 0);

        // Unsupported record
        let bad_record = build_record("73 MiB");
        assert_eq!(bad_record, MemInfoPayloads::Unsupported(0));
        assert_eq!(bad_record.len(), 0);
    }

    /// Check that meminfo record parsing works well
    #[test]
    fn parse_record() {
        // Data volume record
        let mut size_record = build_record("24 kB");
        size_record.push_str("512 kB");
        assert_eq!(size_record,
                   MemInfoPayloads::DataVolume(vec![ByteSize::kib(512)]));
        assert_eq!(size_record.len(), 1);

        // Counter record
        let mut counter_record = build_record("1337");
        counter_record.push_str("371830");
        assert_eq!(counter_record,
                   MemInfoPayloads::Counter(vec![371830]));
        assert_eq!(counter_record.len(), 1);

        // Unsupported record
        let mut bad_record = build_record("57 TiB");
        bad_record.push_str("332 PiB");
        assert_eq!(bad_record, MemInfoPayloads::Unsupported(1));
        assert_eq!(bad_record.len(), 1);
    }

    /// Check that meminfo data initialization works as expected
    #[test]
    fn init_meminfo_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut info = String::new();
        let empty_info = MemInfoData::new(&info);
        assert_eq!(empty_info.data.len(), 0);
        assert_eq!(empty_info.keys.len(), 0);
        assert_eq!(empty_info.len(), 0);
        let mut expected = empty_info;

        // ...adding a first line of memory info...
        info.push_str("MyDataVolume:   1234 kB");
        let single_info = MemInfoData::new(&info);
        expected.data.push(MemInfoPayloads::DataVolume(Vec::new()));
        expected.keys.push("MyDataVolume".to_owned());
        assert_eq!(single_info, expected);
        assert_eq!(expected.len(), 0);

        // ...and a second line of memory info.
        info.push_str("\nMyCounter:   42");
        let double_info = MemInfoData::new(&info);
        expected.data.push(MemInfoPayloads::Counter(Vec::new()));
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
        expected.data[0].push_str("1234 kB");
        assert_eq!(single_info, expected);
        assert_eq!(expected.len(), 1);

        // ...and a second line of memory info.
        info.push_str("\nMyCounter:   42");
        let mut double_info = MemInfoData::new(&info);
        double_info.push(&info);
        expected = MemInfoData::new(&info);
        expected.data[0].push_str("1234 kB");
        expected.data[1].push_str("42");
        assert_eq!(double_info, expected);
        assert_eq!(expected.len(), 1);
    } */

    /// Check that the sampler works well
    define_sampler_tests!{ super::MemInfoSampler }

    /* /// INTERNAL: Build a MemInfoPayloads using columns from a certain string
    fn build_record(input: &str) -> MemInfoPayloads {
        split_line_and_run(input, |columns| {
            let mut stream = MemInfoRecordStream::new(columns);
            stream.fetch();
            MemInfoPayloads::new(stream)
        })
    } */
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
