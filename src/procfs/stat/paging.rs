//! This module contains facilities for parsing and storing the data contained
//! in the paging statistics of /proc/stat (page and swap).

use splitter::SplitColumns;
use super::StatDataStore;


/// Paging statistics record from /proc/stat
///
/// For the paging scenario of interest, this iterator should yield...
///
/// * The amount of memory pages that were brought in from disk
/// * The amount of memory pages that were sent out to disk
/// * A None terminator
///
pub(super) struct RecordFields<'a, 'b> where 'a: 'b {
    /// Data columns of the record, interpreted as paging statistics
    data_columns: SplitColumns<'a, 'b>,
}
//
impl<'a, 'b> Iterator for RecordFields<'a, 'b> {
    /// We're outputting 64-bit counters
    type Item = u64;

    /// This is how we generate them from file columns
    fn next(&mut self) -> Option<Self::Item> {
        self.data_columns.next().map(|str_counter| {
            str_counter.parse().expect("Failed to parse paging counter")
        })
    }
}
//
impl<'a, 'b> RecordFields<'a, 'b> {
    /// Build a new parser for paging record fields
    pub fn new(data_columns: SplitColumns<'a, 'b>) -> Self {
        Self {
            data_columns,
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
    pub fn new() -> Self {
        Self {
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }
    }
}
//
impl StatDataStore for SampledData {
    /// Parse paging statistics and add them to the internal data store
    fn push(&mut self, mut stats: SplitColumns) {
        // Load the incoming and outgoing page count
        self.incoming.push(stats.next().expect("Missing incoming page count")
                                .parse().expect("Could not parse page count"));
        self.outgoing.push(stats.next().expect("Missing outgoing page count")
                                .parse().expect("Could not parse page count"));

        // At this point, we should have loaded all available stats
        debug_assert!(stats.next().is_none(),
                      "Unexpected counter in paging statistics");
    }

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
