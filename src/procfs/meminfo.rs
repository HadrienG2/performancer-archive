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
        self.file_lines.next().map(|file_columns| {
            MemInfoRecordStream {
                fused_columns: file_columns.fuse(),
                last_columns: [None; 2],
                state: MemInfoRecordState::AtStart,
            }
        })
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
    /// Move to the next field of the meminfo record.
    ///
    /// This method is designed so that it can be immediately chained with
    /// kind() in order to analyze what the new field contains, or with the
    /// appropriate parse_xyz() method in order to parse the freshly received
    /// data into a type that is already known.
    ///
    /// Since in the case of /proc/meminfo, the number of fields in a record
    /// is known at compile time, past the end iteration is not supported in
    /// the interface.
    ///
    #[inline(always)]
    pub fn next(&mut self) -> &mut Self {
        match self.state {
            // This is the textual label of the record
            MemInfoRecordState::AtStart => {
                self.state = MemInfoRecordState::AfterLabel;
                self.last_columns[0] = self.fused_columns.next();
            },

            // This is the payload of the record (quantity being measured)
            MemInfoRecordState::AfterLabel => {
                self.state = MemInfoRecordState::AfterPayload;
                self.last_columns[0] = self.fused_columns.next();
                self.last_columns[1] = self.fused_columns.next();
            },

            // This is the end of the record, nothing to do
            MemInfoRecordState::AfterPayload => {
                panic!("No record expected after the payload");
            },
        }
        self
    }

    /// What kind of record field did next() yield? Run this method to find out.
    pub fn kind(&self) -> MemInfoFieldKind {
        match self.state {
            // No data was loaded yet, this call is mistaken
            MemInfoRecordState::AtStart => panic!("Please call next() first"),

            // A meminfo record label was just loaded
            MemInfoRecordState::AfterLabel => MemInfoFieldKind::Label,

            // A payload was just loaded. Let's determine what kind of payload.
            MemInfoRecordState::AfterPayload => {
                match (self.last_columns[0], self.last_columns[1]) {
                    (Some(_), Some("kB")) => MemInfoFieldKind::DataVolume,
                    (Some(_), None)       => MemInfoFieldKind::Counter,
                    _                     => MemInfoFieldKind::Unsupported,
                }
            },
        }
    }

    /// Parse the current meminfo record field as a label
    pub fn parse_label(&mut self) -> &'a str {
        // Fetch the label for our column buffer (and reset the buffer)
        let label = self.last_columns[0]
                        .take()
                        .expect("No input value. Did you call next()?");

        // The label of a meminfo record should end with a trailing colon
        assert_eq!(label.bytes().next_back(), Some(b':'),
                   "Invalid meminfo label terminator");

        // We should not include that colon in the final output
        &label[..label.len()-1]
    }

    /// Parse the current meminfo record field as a data volume
    pub fn parse_data_volume(&mut self) -> ByteSize {
        // If we truly are on a data volume, it should be in our buffers
        let kibs_str_opt = self.last_columns[0].take();
        let unit_opt     = self.last_columns[1].take();

        // Parse data volume, which is in kibibytes (no matter what Linux says)
        let data_volume = ByteSize::kib(
            kibs_str_opt.expect("No input value. Did you call next()?")
                        .parse::<usize>()
                        .expect("Could not parse data volume as an integer")
        );

        // Make sure that the unit is correct
        assert_eq!(unit_opt, Some("kB"));

        // Return the parsed data volume to our caller
        data_volume
    }

    /// Parse the current meminfo record field as a raw counter
    pub fn parse_counter(&mut self) -> u64 {
        // If we truly are on a counter, it should be in our buffers
        let counter_str_opt = self.last_columns[0].take();
        let should_be_none  = self.last_columns[1].take();

        // Parse the counter's value
        let counter =
            counter_str_opt.expect("No input value. Did you call next()?")
                           .parse::<u64>()
                           .expect("Failed to parse the counter's value");

        // Make sure that this truly was a raw counter, with no suffix
        assert_eq!(should_be_none, None);

        // Return the parsed counter value to our client
        counter
    }
}
///
/// Fields of a meminfo record can feature different kinds of data
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
enum MemInfoRecordState { AtStart, AfterLabel, AfterPayload }


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
            let label_field = record.next().expect("Missing meminfo label");
            let label = label_field.as_label();

            // Build storage for the associated quantity (data volume/counter)
            let payload_field = record.next().expect("Missing meminfo data");
            let data = MemInfoPayloads::new(payload_field.as_payload());

            // Report unsupported records in debug mode
            debug_assert!(data != MemInfoPayloads::Unsupported(0),
                          "Missing support for a meminfo record named {}",
                          label);

            // Memorize the record and its key in our data store
            store.data.push(data);
            store.keys.push(label.to_owned());
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
            let label = record.next().expect("Missing meminfo label")
                              .as_label();

            // In release mode, we use the length of the header as a checksum
            // to make sure that the internal structure did not change during
            // sampling. In debug mode, we fully check the header.
            assert_eq!(label.len(), key.len(),
                       "Unsupported structural meminfo change during sampling");
            debug_assert_eq!(label, key,
                             "Unsupported meminfo change during sampling");

            // Forward the data to the appropriate parser
            data.push(record.file_columns);
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


/// Sampled records from /proc/meminfo, which can measure different things:
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
    /// Create a new record, choosing the type based on some raw data
    fn new(raw_data: MemInfoRecordPayload) -> Self {
        match raw_data {
            // Parser yielded a volume of data
            MemInfoRecordPayload::DataVolume(_) => {
                MemInfoPayloads::DataVolume(Vec::new())
            },

            // Parser yielded a raw counter without special semantics
            MemInfoRecordPayload::Counter(_) => {
                MemInfoPayloads::Counter(Vec::new())
            },

            // Parser failed to recognize the inner data type
            MemInfoRecordPayload::Unsupported => {
                MemInfoPayloads::Unsupported(0)
            },
        }
    }

    /// Push new data inside of the record
    fn push(&mut self, mut raw_data: SplitColumns) {
        // Use our knowledge from the first parse to tell what this should be
        match *self {
            // A data volume in kibibytes
            MemInfoPayloads::DataVolume(ref mut v) => {
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
            MemInfoPayloads::Counter(ref mut v) => {
                // Parse and record the counter's value
                v.push(raw_data.next().expect("Unexpected empty record")
                               .parse().expect("Failed to parse counter"));

                // Check that meminfo schema hasn't changed in debug mode
                debug_assert_eq!(raw_data.next(), None);
            },

            // Something unknown and mysterious
            MemInfoPayloads::Unsupported(ref mut count) => {
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

    /// Check that meminfo record initialization works well
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
    }

    /// Check that the sampler works well
    define_sampler_tests!{ super::MemInfoSampler }

    /// INTERNAL: Build a MemInfoPayloads using columns from a certain string
    fn build_record(input: &str) -> MemInfoPayloads {
        split_line_and_run(input, |columns| {
            let mut stream = MemInfoRecordStream {
                file_columns: columns,
                state: MemInfoRecordState::AtPayload,
            };
            MemInfoPayloads::new(stream.next().unwrap().as_payload())
        })
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
