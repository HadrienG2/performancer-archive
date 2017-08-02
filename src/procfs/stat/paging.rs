//! This module contains facilities for parsing and storing the data contained
//! in the paging statistics of /proc/stat (page and swap).

use splitter::SplitLinesBySpace;
use super::StatDataStore;


/// Storage paging ativity statistics
/// TODO: This should be pub(super), waiting for next rustc version...
#[derive(Debug, PartialEq)]
pub struct PagingStatData {
    /// Number of RAM pages that were paged in from disk
    incoming: Vec<u64>,

    /// Number of RAM pages that were paged out to disk
    outgoing: Vec<u64>,
}
//
impl PagingStatData {
    /// Create new paging statistics
    /// TODO: This should be pub(super), waiting for next rustc version...
    pub fn new() -> Self {
        Self {
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }
    }
}
//
impl StatDataStore for PagingStatData {
    /// Parse paging statistics and add them to the internal data store
    fn push(&mut self, stats: &mut SplitLinesBySpace) {
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
    #[allow(dead_code)]
    fn len(&self) -> usize {
        let length = self.incoming.len();
        debug_assert_eq!(length, self.outgoing.len());
        length
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use ::splitter::split_line;
    use super::{PagingStatData, StatDataStore};

    /// Check that paging statistics initialization works as expected
    #[test]
    fn init_paging_stat() {
        let stats = PagingStatData::new();
        assert_eq!(stats.incoming.len(), 0);
        assert_eq!(stats.outgoing.len(), 0);
        assert_eq!(stats.len(), 0);
    }

    /// Check that parsing paging statistics works as expected
    #[test]
    fn parse_paging_stat() {
        let mut stats = PagingStatData::new();
        stats.push(&mut split_line("123 456"));
        assert_eq!(stats.incoming, vec![123]);
        assert_eq!(stats.outgoing, vec![456]);
        assert_eq!(stats.len(), 1);
    }
}
