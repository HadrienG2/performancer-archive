//! This module contains a sampling parser for /proc/meminfo

use ::parser::PseudoFileParser;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use bytesize::ByteSize;
use std::iter::Fuse;

// Implement a sampler for /proc/meminfo
define_sampler!{ Sampler : "/proc/meminfo" => Parser => SampledData }


/// Incremental parser for /proc/meminfo
#[derive(Debug, PartialEq)]
pub struct Parser {}
//
impl PseudoFileParser for Parser {
    /// Build a parser, using initial file contents for schema analysis
    fn new(_initial_contents: &str) -> Self {
        // TODO: Perform initial file format validation?
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
    pub fn next<'b>(&'b mut self) -> Option<FieldStream<'a, 'b>>
        where 'a: 'b
    {
        self.file_lines.next().map(FieldStream::new)
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
/// Stream of fields from a /proc/meminfo record
///
/// This streaming iterator should successively yield the fields of a memory
/// info record, namely:
///
/// * A string label, identifying this record
/// * A payload, which is either a data volume or a counter
///
/// Unsupported payload formats are detected and reported appropriately
///
pub struct FieldStream<'a, 'b> where 'a: 'b {
    /// Fused iterator into the columns of the active record
    fused_columns: Fuse<SplitColumns<'a, 'b>>,

    /// Supplementary state indicating on which field we should be
    state: FieldStreamState,
}
//
impl<'a, 'b> FieldStream<'a, 'b> {
    /// Read the next field of the meminfo record
    ///
    /// Since in the case of /proc/meminfo, the number of fields in a record
    /// is known at compile time, past the end iteration is considered to be
    /// a usage error and not supported in the interface.
    ///
    pub fn next(&mut self) -> Field<'a> {
        // Fetch the appropriate data from the underlying columns iterator
        let stream_state = self.state;
        match self.state {
            // Fetch the textual label of the record
            FieldStreamState::OnLabel => {
                self.state = FieldStreamState::OnPayload;
                Field {
                    file_columns: [self.fused_columns.next(), None],
                    stream_state,
                }
            },

            // Fetch the payload of the record (quantity being measured)
            FieldStreamState::OnPayload => {
                self.state = FieldStreamState::AtEnd;
                Field {
                    file_columns: [self.fused_columns.next(),
                                   self.fused_columns.next()],
                    stream_state,
                }
            },

            // There should be nothing after the record's payload
            FieldStreamState::AtEnd => {
                panic!("No record field expected after the payload")
            },
        }
    }

    /// Construct a new record stream from associated file columns
    fn new(file_columns: SplitColumns<'a, 'b>) -> Self {
        Self {
            fused_columns: file_columns.fuse(),
            state: FieldStreamState::OnLabel,
        }
    }
}
///
/// State of a meminfo field stream
#[derive(Clone, Copy, Debug, PartialEq)]
enum FieldStreamState { OnLabel, OnPayload, AtEnd }
///
///
/// Parseable field from a /proc/meminfo record
///
/// Use the kind() method in order to analyze the /proc/meminfo schema, check
/// the parser's assumptions, and eliminate the data volume vs counter parsing
/// ambiguity.
///
/// After the first sample, you can safely switch to calling the appropriate
/// parse_xyz() method directly, since new meminfo records are always added at
/// the end of the file, and records are never removed.
///
#[derive(Clone, Debug, PartialEq)]
pub struct Field<'a> {
    /// Buffer for the record column(s) associated with this field
    file_columns: [Option<&'a str>; 2],

    /// What kind of field was expected by the parent stream
    stream_state: FieldStreamState,
}
///
impl<'a> Field<'a> {
    /// Tell how the active meminfo record field should be parsed (if at all)
    fn kind(&self) -> FieldKind {
        match self.stream_state {
            // This field should be a meminfo record label, validate it
            FieldStreamState::OnLabel => {
                // A valid label (with a trailing colon) should be present
                let has_valid_label =
                    self.file_columns[0]
                        .as_ref()
                        .map_or(false,
                                |lbl| lbl.bytes().next_back() == Some(b':'));

                // Tell whether a valid label was present in the input
                if has_valid_label {
                    FieldKind::Label
                } else {
                    FieldKind::Unsupported
                }
            },

            // This field should be a meminfo record payload, validate it and
            // disambiguate between data volumes and raw counter payloads.
            FieldStreamState::OnPayload => {
                // A valid payload should start with a positive integer
                let has_valid_ctr = self.file_columns[0]
                                        .as_ref()
                                        .map_or(false,
                                                |s| s.parse::<u64>().is_ok());

                // Payload types are further disambiguated by the presence or
                // absence of a supported unit suffix
                match (has_valid_ctr, self.file_columns[1]) {
                    (true, Some("kB")) => FieldKind::DataVolume,
                    (true, None)       => FieldKind::Counter,
                    _                  => FieldKind::Unsupported,
                }
            },

            // This field should not exist. The parent stream has failed at its
            // task of panicking in case past-the-end iteration is attempted.
            FieldStreamState::AtEnd => {
                panic!("Parent stream should have panicked")
            }
        }
    }

    /// Parse the current meminfo record field as a label
    fn parse_label(self) -> &'a str {
        // In debug mode, validate that we are indeed on a label
        debug_assert_eq!(self.kind(), FieldKind::Label);

        // Fetch the label from our column buffer (and reset the buffer)
        let label = self.file_columns[0]
                        .expect("Missing label in /proc/meminfo");

        // Eliminate the trailing colon of the label from our output
        &label[..label.len()-1]
    }

    /// Parse the current meminfo record field as a data volume
    fn parse_data_volume(self) -> ByteSize {
        // In debug mode, validate that we are indeed on a data volume
        debug_assert_eq!(self.kind(), FieldKind::DataVolume);

        // Parse data volume, which is in kibibytes (no matter what Linux says)
        let data_volume = ByteSize::kib(
            self.file_columns[0]
                .expect("Missing data counter in /proc/meminfo")
                .parse::<usize>()
                .expect("Could not parse data counter.")
        );

        // Return the parsed data volume to our caller
        data_volume
    }

    /// Parse the current meminfo record field as a raw counter
    fn parse_counter(self) -> u64 {
        // In debug mode, validate that we are indeed on a data volume
        debug_assert_eq!(self.kind(), FieldKind::Counter);

        // Parse the counter's value
        let counter = self.file_columns[0]
                          .expect("Missing raw counter in /proc/meminfo")
                          .parse::<u64>()
                          .expect("Could not parse raw counter");

        // Return the parsed counter value to our client
        counter
    }
}
///
/// Fields of a meminfo record can feature different kinds of data
#[derive(Debug, PartialEq)]
pub enum FieldKind {
    /// Textual identifier of the record
    Label,

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
struct SampledData {
    /// Sampled meminfo payloads, in the order in which it appears in the file
    data: Vec<SampledPayloads>,

    /// Keys associated with each record, again in file order
    keys: Vec<String>,
}
//
impl SampledData {
    /// Create a new memory info data store, using a first sample to know the
    /// structure of /proc/meminfo on this system
    fn new(mut stream: RecordStream) -> Self {
        // Our data store will eventually go there
        let mut store = Self {
            data: Vec::new(),
            keys: Vec::new(),
        };

        // For initial record of /proc/meminfo...
        while let Some(mut record) = stream.next() {
            // Fetch and parse the record's label
            let label = {
                let label_field = record.next();
                assert_eq!(label_field.kind(), FieldKind::Label,
                           "Expected a meminfo record label");
                label_field.parse_label()
            };

            // Analyze the record's data payload
            let data = SampledPayloads::new(record.next());

            // Report unsupported payloads in debug mode
            debug_assert!(data != SampledPayloads::Unsupported(0),
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
    fn push(&mut self, mut stream: RecordStream) {
        // This time, we know how lines of /proc/meminfo map to our members
        for (data, key) in self.data.iter_mut().zip(self.keys.iter()) {
            // We start by iterating over records and checking that each record
            // that we observed during initialization is still around
            let mut record = stream.next()
                                   .expect("A meminfo record has disappeared");
            let label = record.next().parse_label();

            // In release mode, we use the length of the header as a checksum
            // to make sure that the internal structure did not change during
            // sampling. In debug mode, we fully check the header.
            assert_eq!(label.len(), key.len(),
                       "Unsupported structural meminfo change during sampling");
            debug_assert_eq!(label, key,
                             "Unsupported meminfo change during sampling");

            // Forward the payload to its target
            data.push(record.next());
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
    fn new(field: Field) -> Self {
        match field.kind() {
            // Parser yielded a volume of data
            FieldKind::DataVolume => {
                SampledPayloads::DataVolume(Vec::new())
            },

            // Parser yielded a raw counter without special semantics
            FieldKind::Counter => {
                SampledPayloads::Counter(Vec::new())
            },

            // Parser failed to recognize the inner data type
            FieldKind::Unsupported => {
                SampledPayloads::Unsupported(0)
            },

            // Parser yielded a label (i.e. upstream SampledData messed up)
            FieldKind::Label => {
                panic!("meminfo record label should already have been fetched")
            },
        }
    }

    /// Push new data inside of the payload table
    fn push(&mut self, field: Field) {
        // Use our knowledge from the first parse to tell what this should be
        match *self {
            // A data volume in kibibytes
            SampledPayloads::DataVolume(ref mut v) => {
                v.push(field.parse_data_volume());
            },

            // A raw counter
            SampledPayloads::Counter(ref mut v) => {
                v.push(field.parse_counter());
            },

            // Something unknown and mysterious
            SampledPayloads::Unsupported(ref mut count) => {
                *count += 1;
            },
        }
    }

    /// Tell how many samples are present in the data store
    #[cfg(test)]
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
    use super::{ByteSize, Field, FieldStream, FieldKind, FieldStreamState,
                Parser, PseudoFileParser, RecordStream, SampledData,
                SampledPayloads};

    /// Check that label field parsing works as expected
    #[test]
    fn label_field_parsing() {
        // Supported label field
        with_label_field("MyLabel", |valid_label| {
            assert_eq!(valid_label.kind(), FieldKind::Label);
            assert_eq!(valid_label.parse_label(), "MyLabel");
        });

        // Missing colon
        let missing_colon = Field {
            file_columns: [Some("MyOtherLabel"), None],
            stream_state: FieldStreamState::OnLabel,
        };
        assert_eq!(missing_colon.kind(), FieldKind::Unsupported);

        // Missing data
        let missing_data = Field {
            file_columns: [None, None],
            stream_state: FieldStreamState::OnLabel,
        };
        assert_eq!(missing_data.kind(), FieldKind::Unsupported);
    }

    /// Check that payload field parsing works as expected
    #[test]
    fn payload_field_parsing() {
        // Valid data volume payload
        with_data_volume_field(ByteSize::mib(713705), |valid_data_volume| {
            assert_eq!(valid_data_volume.kind(), FieldKind::DataVolume);
            assert_eq!(valid_data_volume.parse_data_volume(),
                       ByteSize::mib(713705));
        });

        // Invalid data volume unit
        let invalid_unit = Field {
            file_columns: [Some("1337"), Some("zorglub")],
            stream_state: FieldStreamState::OnPayload,
        };
        assert_eq!(invalid_unit.kind(), FieldKind::Unsupported);

        // Invalid data volume counter
        let invalid_data_count = Field {
            file_columns: [Some("quarante-deux"), Some("kB")],
            stream_state: FieldStreamState::OnPayload,
        };
        assert_eq!(invalid_data_count.kind(), FieldKind::Unsupported);

        // Valid raw counter
        with_counter_field(911, |valid_counter| {
            assert_eq!(valid_counter.kind(), FieldKind::Counter);
            assert_eq!(valid_counter.parse_counter(), 911);
        });

        // Invalid raw counter
        let invalid_counter = Field {
            file_columns: [Some("Robespierre"), None],
            stream_state: FieldStreamState::OnPayload,
        };
        assert_eq!(invalid_counter.kind(), FieldKind::Unsupported);
    }

    /// Check that sampled payloads container works as expected...
    #[test]
    fn sampled_payloads() {
        /// ...with data volume payloads
        let mut data_payloads = with_data_volume_field(ByteSize::kib(768),
                                                       SampledPayloads::new);
        assert_eq!(data_payloads,
                   SampledPayloads::DataVolume(Vec::new()));
        assert_eq!(data_payloads.len(), 0);
        let sample_data = ByteSize::gib(2);
        with_data_volume_field(sample_data, |field| data_payloads.push(field));
        assert_eq!(data_payloads,
                   SampledPayloads::DataVolume(vec![sample_data]));
        assert_eq!(data_payloads.len(), 1);

        // ...with raw counter payloads
        let mut counter_payloads = with_counter_field(42, SampledPayloads::new);
        assert_eq!(counter_payloads,
                   SampledPayloads::Counter(Vec::new()));
        assert_eq!(counter_payloads.len(), 0);
        let sample_count = 6463;
        with_counter_field(sample_count, |field| counter_payloads.push(field));
        assert_eq!(counter_payloads,
                   SampledPayloads::Counter(vec![sample_count]));
        assert_eq!(counter_payloads.len(), 1);
        
        // ...and with unsupported payloads
        let unsupported_field = Field {
            file_columns: [None, None],
            stream_state: FieldStreamState::OnPayload,
        };
        let mut unsupported_payloads =
            SampledPayloads::new(unsupported_field.clone());
        assert_eq!(unsupported_payloads, SampledPayloads::Unsupported(0));
        assert_eq!(unsupported_payloads.len(), 0);
        unsupported_payloads.push(unsupported_field);
        assert_eq!(unsupported_payloads, SampledPayloads::Unsupported(1));
        assert_eq!(unsupported_payloads.len(), 1);
    }

    /// Check that field streams work as expected...
    #[test]
    fn field_stream() {
        // ...on streamed data volumes...
        with_field_stream("Test: 42 kB", |mut field_stream| {
            assert_eq!(field_stream.next(),
                       Field {
                           file_columns: [Some("Test:"), None],
                           stream_state: FieldStreamState::OnLabel,
                       });
            assert_eq!(field_stream.next(),
                       Field {
                           file_columns: [Some("42"), Some("kB")],
                           stream_state: FieldStreamState::OnPayload,
                       });
        });

        // ...on streamed raw counters...
        with_field_stream("OtherTest: 1984", |mut field_stream| {
            assert_eq!(field_stream.next(),
                       Field {
                           file_columns: [Some("OtherTest:"), None],
                           stream_state: FieldStreamState::OnLabel,
                       });
            assert_eq!(field_stream.next(),
                       Field {
                           file_columns: [Some("1984"), None],
                           stream_state: FieldStreamState::OnPayload,
                       });
        });

        // ...and even on blank lines, because who knows what's going to happen
        // to meminfo's format in the future? I sure don't. That's one of the
        // problems with human-readable text files as an OS kernel API.
        with_field_stream(" ", |mut field_stream| {
            assert_eq!(field_stream.next(),
                       Field {
                           file_columns: [None, None],
                           stream_state: FieldStreamState::OnLabel,
                       });
            assert_eq!(field_stream.next(),
                       Field {
                           file_columns: [None, None],
                           stream_state: FieldStreamState::OnPayload,
                       });
        });
    }

    /// Check that record streams work as expected
    #[test]
    fn record_stream() {
        // Build a pseudo-file from a set of records
        let pseudo_file = ["OneRecord: 321 kB",
                           "TwoRecords: 9786",
                           " ",
                           "Dafuk?"].join("\n");

        // This is the associated record stream
        let record_stream = RecordStream::new(&pseudo_file);

        // Check that our test record stream looks as expected
        check_record_stream(record_stream, &pseudo_file);
    }

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
        let mut sampled_data = SampledData::new(initial_records);
        assert_eq!(sampled_data, SampledData {
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
        assert_eq!(sampled_data, SampledData {
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
    }

    /// Get a field that parses into a label and do something with it
    fn with_label_field<F, R>(label: &str, operation: F) -> R
        where F: FnOnce(Field) -> R
    {
        // Build the label's tag
        let mut label_tag = String::with_capacity(label.len()+1);
        label_tag.push_str(label);
        label_tag.push(':');

        // Create a corresponding field struct
        let field = Field {
            file_columns: [Some(&label_tag), None],
            stream_state: FieldStreamState::OnLabel,
        };

        // Run the user-provided functor on that field and return the result
        operation(field)
    }

    /// Get a field that parses into a data volume and do something with it
    fn with_data_volume_field<F, R>(data_volume: ByteSize, operation: F) -> R
        where F: FnOnce(Field) -> R
    {
        // Build the counter
        let kib_counter = (data_volume.as_usize() / bytesize::KIB).to_string();

        // Create a corresponding field struct
        let field = Field {
            file_columns: [Some(&kib_counter), Some("kB")],
            stream_state: FieldStreamState::OnPayload,
        };

        // Run the user-provided functor on that field and return the result
        operation(field)
    }

    /// Get a field that parses into a raw counter and do something with it
    fn with_counter_field<F, R>(counter: u64, operation: F) -> R
        where F: FnOnce(Field) -> R
    {
        // Build the counter
        let raw_counter = counter.to_string();

        // Create a corresponding field struct
        let field = Field {
            file_columns: [Some(&raw_counter), None],
            stream_state: FieldStreamState::OnPayload,
        };

        // Run the user-provided functor on that field and return the result
        operation(field)
    }

    /// Build the field stream associated with a certain line of text, and run
    /// code taking it as a parameter
    fn with_field_stream<F, R>(line_of_text: &str, functor: F) -> R
        where F: FnOnce(FieldStream) -> R
    {
        split_line_and_run(line_of_text, |columns| {
            let field_stream = FieldStream::new(columns);
            functor(field_stream)
        })
    }

    /// Test that the output of a record stream is right for a given input file
    fn check_record_stream(mut stream: RecordStream, file_contents: &str) {
        for record in file_contents.lines() {
            with_field_stream(record, |mut expected_fields| {
                let mut actual_fields = stream.next().unwrap();
                assert_eq!(actual_fields.next(), expected_fields.next());
                assert_eq!(actual_fields.next(), expected_fields.next());
            });
        }
    }

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
