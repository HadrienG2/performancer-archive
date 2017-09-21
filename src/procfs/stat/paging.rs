//! This module contains facilities for parsing and storing the data contained
//! in the paging statistics of /proc/stat (page and swap).

use splitter::SplitColumns;
use super::StatDataStore;


/// Paging statistics record from /proc/stat
pub(super) struct RecordFields {
    /// Number of memory pages that were brought in from disk
    pub incoming: u64,

    /// Number of memory pages that were sent out to disk
    pub outgoing: u64,
}
//
impl RecordFields {
    /// Decode the paging data
    pub fn new<'a, 'b>(mut data_columns: SplitColumns<'a, 'b>) -> Self {
        // Scope added to address current borrow checker limitation
        let (incoming, outgoing) = {
            /// This is how we decode one field from the input
            let mut parse_counter = || -> u64 {
                data_columns.next().expect("Missing paging counter")
                            .parse().expect("Failed to parse paging counter")
            };

            /// Parse the counters of incoming and outgoing pages
            (parse_counter(), parse_counter())
        };

        // In debug mode, check that nothing weird appeared in the input
        debug_assert_eq!(data_columns.next(), None,
                         "Unexpected additional paging counter");

        /// Return the paging counters
        Self {
            incoming,
            outgoing,
        }
    }
}


/// Storage paging ativity statistics
#[derive(Debug, PartialEq)]
pub(super) struct SampledData {
    /// Number of RAM pages that were paged in from disk
    incoming: Vec<u64>,

    /// Number of RAM pages that were paged out to disk
    outgoing: Vec<u64>,
}
//
impl SampledData {
    /// Create new paging statistics
    pub fn new(_fields: RecordFields) -> Self {
        Self {
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }
    }

    /// Parse paging statistics and add them to the internal data store
    pub fn push(&mut self, fields: RecordFields) {
        self.incoming.push(fields.incoming);
        self.outgoing.push(fields.outgoing);
    }
}
//
impl StatDataStore for SampledData {
    /// Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        let length = self.incoming.len();
        debug_assert_eq!(length, self.outgoing.len());
        length
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use ::splitter::split_line_and_run;
    use super::{RecordFields, SampledData, StatDataStore};

    /// Check that paging statistics parsing works as expected
    #[test]
    fn record_fields() {
        with_record_fields("865 43", |fields| {
            assert_eq!(fields.incoming, 865);
            assert_eq!(fields.outgoing, 43);
        });
    }

    /// Check that paging statistics are stored as expected
    #[test]
    fn sampled_data() {
        // The initial state should be right
        let mut data = with_record_fields("4 312", SampledData::new);
        assert_eq!(data.incoming, Vec::new());
        assert_eq!(data.outgoing, Vec::new());
        assert_eq!(data.len(),    0);

        // Pushing data in should work correctly
        with_record_fields("600 598", |fields| data.push(fields));
        assert_eq!(data.incoming, vec![600]);
        assert_eq!(data.outgoing, vec![598]);
        assert_eq!(data.len(),    1);
        with_record_fields("666 4097", |fields| data.push(fields));
        assert_eq!(data.incoming, vec![600, 666]);
        assert_eq!(data.outgoing, vec![598, 4097]);
        assert_eq!(data.len(),    2);
    }

    /// Build the paging record fields associated with a certain line of text,
    /// and run code taking that as a parameter
    fn with_record_fields<F, R>(line_of_text: &str, functor: F) -> R
        where F: FnOnce(RecordFields) -> R
    {
        split_line_and_run(line_of_text, |columns| {
            let fields = RecordFields::new(columns);
            functor(fields)
        })
    }
}
