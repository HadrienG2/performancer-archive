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
    use super::{SampledData, StatDataStore};

    /// Check that paging statistics initialization works as expected
    #[test]
    fn init_paging_stat() {
        let stats = SampledData::new();
        assert_eq!(stats.incoming.len(), 0);
        assert_eq!(stats.outgoing.len(), 0);
        assert_eq!(stats.len(), 0);
    }

    /// Check that parsing paging statistics works as expected
    #[test]
    fn parse_paging_stat() {
        let mut stats = SampledData::new();
        stats.push_str("123 456");
        assert_eq!(stats.incoming, vec![123]);
        assert_eq!(stats.outgoing, vec![456]);
        assert_eq!(stats.len(), 1);
    }
}
