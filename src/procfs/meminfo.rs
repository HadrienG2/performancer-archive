//! This module contains a sampling parser for /proc/meminfo

use ::data::SampledData;
use ::parser::PseudoFileParser;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use bytesize::ByteSize;

// Implement a sampler for /proc/meminfo
define_sampler!{ Sampler : "/proc/meminfo" => Parser => Data }


/// Incremental parser for /proc/meminfo
#[derive(Debug, PartialEq)]
pub struct Parser {}
//
impl PseudoFileParser for Parser {
    /// Build a parser, using an initial file sample. Here, this is used to
    /// perform quick schema validation, just to maximize the odds that failure,
    /// if any, will occur at initialization time rather than run time.
    fn new(initial_contents: &str) -> Self {
        let mut validation_stream = RecordStream::new(initial_contents);
        while let Some(record) = validation_stream.next() {
            let label = record.label();
            let payload = record.extract_payload();
            debug_assert!(payload.kind() != PayloadKind::Unsupported,
                          "Missing support for record {}", label);
        }
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
/// Stream of records from /proc/meminfo
///
/// This streaming iterator should yield a stream of memory info records, each
/// representing a line of /proc/meminfo (i.e. a named counter or data volume).
///
pub struct RecordStream<'a> {
    /// Iterator into the lines and columns of /proc/meminfo
    file_lines: SplitLinesBySpace<'a>,
}
//
impl<'a> RecordStream<'a> {
    /// Parse the next record from /proc/meminfo into a stream of fields
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
/// Record from /proc/meminfo (labeled data volume or counter)
pub struct Record<'a, 'b> where 'a: 'b {
    /// Label of the active record
    label_field: &'a str,

    /// Iterator into the payload's columns
    payload_columns: SplitColumns<'a, 'b>,
}
//
impl<'a, 'b> Record<'a, 'b> {
    /// Tell how this record is labeled
    pub fn label(&self) -> &'a str {
        // The label field of a meminfo record should end with a colon
        debug_assert_eq!(self.label_field.bytes().next_back(), Some(b':'),
                         "Incorrectly formatted meminfo label");

        // The text before that colon is the label itself
        let label_length = self.label_field.len();
        assert!(label_length > 2, "Unexpected empty meminfo label");
        &self.label_field[..label_length-1]
    }

    /// Extract the payload from the active /proc/meminfo record
    pub fn extract_payload(self) -> Payload<'a> {
        Payload::new(self.payload_columns)
    }

    /// Construct a record from associated file columns
    fn new(mut record_columns: SplitColumns<'a, 'b>) -> Self {
        let label_field = record_columns.next().expect("Record label missing");
        Self {
            label_field,
            payload_columns: record_columns,
        }
    }
}
///
///
/// Payload from a /proc/meminfo record (data volume or counter)
#[derive(Debug, PartialEq)]
pub struct Payload<'a> {
    /// Amount of the quantity being measured (data or a count of something)
    amount: u64,

    /// Optional unit suffix
    unit: Option<&'a str>,
}
///
impl<'a> Payload<'a> {
    /// Tell whether this is a data volume or a raw counter
    pub fn kind(&self) -> PayloadKind {
        match self.unit {
            Some("kB") => PayloadKind::DataVolume,
            None       => PayloadKind::Counter,
            _          => PayloadKind::Unsupported,
        }
    }

    /// Parse as a data volume
    pub fn parse_data_volume(self) -> ByteSize {
        // In debug mode, validate that we are indeed on a data volume
        debug_assert_eq!(self.kind(), PayloadKind::DataVolume);

        // Parse data volume, which is in kibibytes (no matter what Linux says)
        ByteSize::kib(self.amount as usize)
    }

    /// Parse as a raw counter
    pub fn parse_counter(self) -> u64 {
        // In debug mode, validate that we are indeed on a counter
        debug_assert_eq!(self.kind(), PayloadKind::Counter);

        // Nothing special to do in this case
        self.amount
    }

    /// Construct a payload from associated file columns
    fn new<'b>(mut payload_columns: SplitColumns<'a, 'b>) -> Self {
        let amount = payload_columns.next().expect("Missing amount field")
                                    .parse().expect("Expected a number");
        Self {
            amount,
            unit: payload_columns.next(),
        }
    }
}
///
#[derive(Debug, PartialEq)]
pub enum PayloadKind {
    /// Volume of data
    DataVolume,

    /// Raw integer counter
    Counter,

    /// Some payload unsupported by this parser :-(
    Unsupported,
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
pub struct Data {
    /// Sampled meminfo payloads, in the order in which it appears in the file
    data: Vec<SampledPayloads>,

    /// Keys associated with each record, again in file order
    keys: Vec<String>,
}
//
impl SampledData for Data {
    /// Tell how many samples are present in the data store + check consistency
    fn len(&self) -> usize {
        // We'll return the length of the first record, if any, or else zero
        let length = self.data.first().map_or(0, |rec| rec.len());

        // In debug mode, check that all records have the same length
        debug_assert!(self.data.iter().all(|rec| rec.len() == length));

        // Return the number of samples in the data store
        length
    }
}
//
// TODO: Implement SampledDataIncremental once that is usable in stable Rust
impl Data {
    /// Create a new memory info data store, using a first sample to know the
    /// structure of /proc/meminfo on this system
    fn new(mut stream: RecordStream) -> Self {
        // Our data store will eventually go there
        let mut store = Self {
            data: Vec::new(),
            keys: Vec::new(),
        };

        // For initial record of /proc/meminfo...
        while let Some(record) = stream.next() {
            // Fetch and parse the record's label
            let label = record.label();

            // Analyze the record's data payload
            let data = SampledPayloads::new(record.extract_payload());

            // Memorize the key and payload store in our data store
            store.keys.push(label.to_owned());
            store.data.push(data);
        }

        // Return our data collection setup
        store
    }

    /// Parse the contents of /proc/meminfo and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, mut stream: RecordStream) {
        // This time, we know how lines of /proc/meminfo map to our members
        for (data, key) in self.data.iter_mut().zip(self.keys.iter()) {
            // We start by iterating over records and checking that each record
            // that we observed during initialization is still around
            let record = stream.next().expect("A record has disappeared");
            let label = record.label();

            // In release mode, we use the length of the header as a checksum
            // to make sure that the internal structure did not change during
            // sampling. In debug mode, we fully check the header.
            assert_eq!(label.len(), key.len(),
                       "Unsupported structural meminfo change during sampling");
            debug_assert_eq!(label, key,
                             "Unsupported meminfo change during sampling");

            // Forward the payload to its target
            data.push(record.extract_payload());
        }

        // In debug mode, we also check that records did not appear out of blue
        debug_assert!(stream.next().is_none(),
                      "A meminfo record appeared out of nowhere");
    }
}


/// Sampled payloads from /proc/meminfo, which can measure different things:
#[derive(Debug, PartialEq)]
enum SampledPayloads {
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
impl SampledPayloads {
    /// Create a new payload, choosing the type based on some sample record
    fn new(payload: Payload) -> Self {
        match payload.kind() {
            // Parser yielded a volume of data
            PayloadKind::DataVolume => {
                SampledPayloads::DataVolume(Vec::new())
            },

            // Parser yielded a raw counter without special semantics
            PayloadKind::Counter => {
                SampledPayloads::Counter(Vec::new())
            },

            // Parser failed to recognize the inner data type
            PayloadKind::Unsupported => {
                SampledPayloads::Unsupported(0)
            },
        }
    }

    /// Push new data inside of the payload table
    fn push(&mut self, payload: Payload) {
        // Use our knowledge from the first parse to tell what this should be
        match *self {
            // A data volume in kibibytes
            SampledPayloads::DataVolume(ref mut v) => {
                v.push(payload.parse_data_volume());
            },

            // A raw counter
            SampledPayloads::Counter(ref mut v) => {
                v.push(payload.parse_counter());
            },

            // Something unknown and mysterious
            SampledPayloads::Unsupported(ref mut count) => {
                *count += 1;
            },
        }
    }

    /// Tell how many samples are present in the data store
    fn len(&self) -> usize {
        match *self {
            SampledPayloads::DataVolume(ref v)  => v.len(),
            SampledPayloads::Counter(ref v)     => v.len(),
            SampledPayloads::Unsupported(count) => count,
        }
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use bytesize;
    use ::splitter::split_line_and_run;
    use super::{ByteSize, Data, Parser, Payload, PayloadKind, PseudoFileParser,
                Record, RecordStream, SampledData, SampledPayloads};

    /// Check that payload parsing works as expected
    #[test]
    fn payload_parsing() {
        // Valid data volume payload
        with_data_volume_payload(ByteSize::mib(713705), |valid_data_payload| {
            assert_eq!(valid_data_payload.kind(), PayloadKind::DataVolume);
            assert_eq!(valid_data_payload.parse_data_volume(),
                       ByteSize::mib(713705));
        });

        // Valid raw counter
        with_counter_payload(911, |valid_counter_payload| {
            assert_eq!(valid_counter_payload.kind(), PayloadKind::Counter);
            assert_eq!(valid_counter_payload.parse_counter(), 911);
        });

        // Unsupported payload type
        with_unsupported_payload(|unsupported_payload| {
            assert_eq!(unsupported_payload.kind(), PayloadKind::Unsupported);
        });
    }

    /// Check that sampled payload containers work as expected...
    #[test]
    fn sampled_payloads() {
        // ...with data volume payloads
        let mut data_payloads = with_data_volume_payload(ByteSize::kib(768),
                                                         SampledPayloads::new);
        assert_eq!(data_payloads,
                   SampledPayloads::DataVolume(Vec::new()));
        assert_eq!(data_payloads.len(), 0);
        let sample_data = ByteSize::gib(2);
        with_data_volume_payload(sample_data,
                                 |payload| data_payloads.push(payload));
        assert_eq!(data_payloads,
                   SampledPayloads::DataVolume(vec![sample_data]));
        assert_eq!(data_payloads.len(), 1);

        // ...with raw counter payloads
        let mut counter_payloads = with_counter_payload(42,
                                                        SampledPayloads::new);
        assert_eq!(counter_payloads,
                   SampledPayloads::Counter(Vec::new()));
        assert_eq!(counter_payloads.len(), 0);
        let sample_count = 6463;
        with_counter_payload(sample_count,
                             |payload| counter_payloads.push(payload));
        assert_eq!(counter_payloads,
                   SampledPayloads::Counter(vec![sample_count]));
        assert_eq!(counter_payloads.len(), 1);
        
        // ...and with unsupported payloads
        let mut unsupported_payloads =
            with_unsupported_payload(SampledPayloads::new);
        assert_eq!(unsupported_payloads, SampledPayloads::Unsupported(0));
        assert_eq!(unsupported_payloads.len(), 0);
        with_unsupported_payload(|unsupported_payload| {
            unsupported_payloads.push(unsupported_payload)
        });
        assert_eq!(unsupported_payloads, SampledPayloads::Unsupported(1));
        assert_eq!(unsupported_payloads.len(), 1);
    }

    /// Check that record parsing works as expected
    #[test]
    fn record_parsing() {
        with_record("MyCrazyLabel: 10248 kB", |record| {
            assert_eq!(record.label(), "MyCrazyLabel");
            let payload = record.extract_payload();
            assert_eq!(payload.kind(), PayloadKind::DataVolume);
            assert_eq!(payload.parse_data_volume(), ByteSize::kib(10248));
        });
    }

    /// Check that record streams work as expected
    #[test]
    fn record_stream() {
        // Build a pseudo-file from a set of records
        let pseudo_file = ["OneRecord: 321 kB",
                           "TwoRecords: 9786",
                           "StupidRecord: 47 MeV"].join("\n");

        // This is the associated record stream
        let record_stream = RecordStream::new(&pseudo_file);

        // Check that our test record stream looks as expected
        check_record_stream(record_stream, &pseudo_file);
    }

    /// Call a function with a payload that parses into a certain data volume
    fn with_data_volume_payload<F, R>(data_volume: ByteSize, operation: F) -> R
        where F: FnOnce(Payload) -> R
    {
        // Translate the data volume into meminfo-like text
        let mut text = (data_volume.as_usize() / bytesize::KIB).to_string();
        text.push_str(" kB");

        // Create a corresponding payload
        let payload = split_line_and_run(&text, Payload::new);

        // Run the user-provided functor on that field and return the result
        operation(payload)
    }

    /// Call a function with a payload that parses into a certain raw count
    fn with_counter_payload<F, R>(counter: u64, operation: F) -> R
        where F: FnOnce(Payload) -> R
    {
        // Translate the counter into text
        let text = counter.to_string();

        // Create a corresponding payload
        let payload = split_line_and_run(&text, Payload::new);

        // Run the user-provided functor on that field and return the result
        operation(payload)
    }

    /// Call a function with an unsupported payload
    fn with_unsupported_payload<F, R>(operation: F) -> R
        where F: FnOnce(Payload) -> R
    {
        // Create an unsupported payload
        let payload = split_line_and_run(&"1337 zorglub", Payload::new);

        // Run the user-provided functor on that field and return the result
        operation(payload)
    }

    /// Call a function with a record matching a certain line of meminfo text
    fn with_record<F, R>(record_str: &str, operation: F) -> R
        where F: FnOnce(Record) -> R
    {
        split_line_and_run(record_str, |record_columns| {
            let record = Record::new(record_columns);
            operation(record)
        })
    }

    /// Test that the output of a record stream is right for a given input file
    fn check_record_stream(mut stream: RecordStream, file_contents: &str) {
        for record_str in file_contents.lines() {
            with_record(record_str, |expected_record| {
                let actual_record = stream.next().unwrap();
                assert_eq!(actual_record.label(), expected_record.label());
                assert_eq!(actual_record.extract_payload(),
                           expected_record.extract_payload());
            });
        }
    }

    /*

    /// Check that parsers work as expected
    #[test]
    fn parser() {
        // Build a pseudo-file from a set of records, use that to init a parser
        let initial_file = ["TwoPlusTwo: 5",
                            "Abc123",
                            " ",
                            "ThreeRecords: 42 kB"].join("\n");
        let mut parser = Parser::new(&initial_file);

        // Now, build another file which is a variant of the first one, and
        // check that the parser can ingest it just fine
        let file_contents = ["TwoPlusTwo: 9486",
                             "Abc123",
                             " ",
                             "ThreeRecords: 76415 kB"].join("\n");
        let record_stream = parser.parse(&file_contents);

        // Check that our test record stream looks as expected
        check_record_stream(record_stream, &file_contents);
    }

    /// Check that sampled data works as expected
    #[test]
    fn sampled_data() {
        // Let's build ourselves a fake meminfo file
        let initial_contents = ["What:     9876",
                                "Could:    6513 kB",
                                "Possibly: 98743 kB",
                                "Go:       48961",
                                "Wrong:    5474"].join("\n");

        // Build a data sampler for that file
        let initial_records = RecordStream::new(&initial_contents);
        let mut sampled_data = Data::new(initial_records);
        assert_eq!(sampled_data, Data {
            data: vec![SampledPayloads::Counter(Vec::new()),
                       SampledPayloads::DataVolume(Vec::new()),
                       SampledPayloads::DataVolume(Vec::new()),
                       SampledPayloads::Counter(Vec::new()),
                       SampledPayloads::Counter(Vec::new())],
            keys: vec!["What".to_string(),
                       "Could".to_string(),
                       "Possibly".to_string(),
                       "Go".to_string(),
                       "Wrong".to_string()]
        });
        assert_eq!(sampled_data.len(), 0);

        // Try to acquire one data sample and see how well that works out
        let file_contents = ["What:     9876",
                             "Could:    6514 kB",
                             "Possibly: 98753 kB",
                             "Go:       50161",
                             "Wrong:    6484"].join("\n");
        let file_records = RecordStream::new(&file_contents);
        sampled_data.push(file_records);
        assert_eq!(sampled_data, Data {
            data: vec![SampledPayloads::Counter(vec![9876]),
                       SampledPayloads::DataVolume(vec![ByteSize::kib(6514)]),
                       SampledPayloads::DataVolume(vec![ByteSize::kib(98753)]),
                       SampledPayloads::Counter(vec![50161]),
                       SampledPayloads::Counter(vec![6484])],
            keys: vec!["What".to_string(),
                       "Could".to_string(),
                       "Possibly".to_string(),
                       "Go".to_string(),
                       "Wrong".to_string()]
        });
        assert_eq!(sampled_data.len(), 1);
    }*/

    /// Check that the sampler works well
    define_sampler_tests!{ super::Sampler }
}


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    define_sampler_benchs!{ super::Sampler,
                            "/proc/meminfo",
                            500_000 }
}
