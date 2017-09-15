//! This module contains facilities for parsing and storing the data contained
//! in the IRQ statistics of /proc/stat (intr and softirq).

use ::splitter::SplitColumns;
use super::StatDataStore;


/// Interrupt statistics record from /proc/stat
///
/// For either hardware or software interrupts, this iterator should yield...
///
/// * The total amount of interrupts of this kind that were serviced
/// * A breakdown of the interrupts that were serviced for each "numbered"
///   interrupt source known to the kernel
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
            // On some architectures such as x86_64, there are many possible
            // interrupt sources and most of them will never fire. Special-
            // casing zero interrupt counts will thus speed up parsing.
            if str_counter == "0" {
                0
            } else {
                str_counter.parse().expect("Failed to parse interrupt counter")
            }
        })
    }
}
//
impl<'a, 'b> RecordFields<'a, 'b> {
    /// Build a new parser for interrupt record fields
    pub fn new(data_columns: SplitColumns<'a, 'b>) -> Self {
        Self {
            data_columns,
        }
    }
}


/// Interrupt statistics from /proc/stat, in structure-of-array layout
#[derive(Debug, PartialEq)]
pub(super) struct SampledData {
    /// Total number of interrupts that were serviced. May be higher than the
    /// sum of the breakdown below if there are unnumbered interrupt sources.
    total: Vec<u64>,

    /// For each numbered source, details on the amount of serviced interrupt.
    details: Vec<SampledCounter>
}
//
impl SampledData {
    /// Create new interrupt statistics, given the amount of interrupt sources
    pub fn new(num_irqs: u16) -> Self {
        Self {
            total: Vec::new(),
            details: vec![SampledCounter::new(); num_irqs as usize],
        }
    }
}
//
impl StatDataStore for SampledData {
    /// Parse interrupt statistics and add them to the internal data store
    fn push(&mut self, mut stats: SplitColumns) {
        // Load the total interrupt count
        self.total.push(stats.next().expect("Total IRQ count missing")
                             .parse().expect("Failed to parse IRQ count"));

        // Load the detailed interrupt counts from each source
        for detail in self.details.iter_mut() {
            detail.push(stats.next().expect("An IRQ counter went missing"));
        }

        // At this point, we should have loaded all available stats
        debug_assert!(stats.next().is_none(),
                      "An IRQ counter appeared out of nowhere");
    }

    // Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        let length = self.total.len();
        debug_assert!(self.details.iter().all(|vec| vec.len() == length));
        length
    }
}
///
///
/// On some platforms such as x86, there are a lot of hardware IRQs (~500 on my
/// machines), but most of them are unused and never fire. Parsing and storing
/// the associated zeroes from /proc/stat by normal means wastes CPU time and
/// RAM, so we take a shortcut for this common use case.
///
#[derive(Clone, Debug, PartialEq)]
enum SampledCounter {
    /// If we've only ever seen zeroes, we only count the number of zeroes
    Zeroes(usize),

    /// Otherwise, we sample the interrupt counts normally
    Samples(Vec<u64>),
}
//
impl SampledCounter {
    /// Initialize the interrupt count sampler
    fn new() -> Self {
        SampledCounter::Zeroes(0)
    }

    /// Insert a new interrupt count from /proc/stat
    fn push(&mut self, intr_count: &str) {
        match *self {
            // Have we only seen zeroes so far?
            SampledCounter::Zeroes(zero_count) => {
                // Are we seeing a zero again?
                if intr_count == "0" {
                    // If yes, just increment the zero counter
                    *self = SampledCounter::Zeroes(zero_count+1);
                } else {
                    // If not, move to regular interrupt count sampling
                    let mut samples = vec![0; zero_count];
                    samples.push(
                        intr_count.parse().expect("Failed to parse IRQ count")
                    );
                    *self = SampledCounter::Samples(samples);
                }
            },

            // If the interrupt counter is nonzero, sample it normally
            SampledCounter::Samples(ref mut vec) => {
                vec.push(intr_count.parse()
                                   .expect("Failed to parse IRQ count"));
            }
        }
    }

    /// Tell how many interrupt counts we have recorded so far
    #[cfg(test)]
    fn len(&self) -> usize {
        match *self {
            SampledCounter::Zeroes(zero_count) => zero_count,
            SampledCounter::Samples(ref vec) => vec.len(),
        }
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use super::{SampledCounter, SampledData, StatDataStore};

    /// Check that initializing an interrupt count sampler works as expected
    #[test]
    fn init_interrupt_counts() {
        let counts = SampledCounter::new();
        assert_eq!(counts, SampledCounter::Zeroes(0));
        assert_eq!(counts.len(), 0);
    }

    /// Check that interrupt count sampling works as expected
    #[test]
    fn parse_interrupt_counts() {
        // Adding one zero should keep us in the base "zeroes" state
        let mut counts = SampledCounter::new();
        counts.push("0");
        assert_eq!(counts, SampledCounter::Zeroes(1));
        assert_eq!(counts.len(), 1);

        // Adding a nonzero value should get us out of this state
        counts.push("123");
        assert_eq!(counts, SampledCounter::Samples(vec![0, 123]));
        assert_eq!(counts.len(), 2);

        // After that, sampling should work normally
        counts.push("456");
        assert_eq!(counts, SampledCounter::Samples(vec![0, 123, 456]));
        assert_eq!(counts.len(), 3);

        // Sampling right from the start should work as well
        let mut counts2 = SampledCounter::new();
        counts2.push("789");
        assert_eq!(counts2, SampledCounter::Samples(vec![789]));
        assert_eq!(counts2.len(), 1);
    }

    /// Check that interrupt statistics initialization works as expected
    #[test]
    fn init_interrupt_stat() {
        // Check that interrupt statistics without any details work
        let no_details_stats = SampledData::new(0);
        assert_eq!(no_details_stats.total.len(), 0);
        assert_eq!(no_details_stats.details.len(), 0);
        assert_eq!(no_details_stats.len(), 0);

        // Check that interrupt statistics with two detailed counters work
        let two_stats = SampledData::new(2);
        assert_eq!(two_stats.details.len(), 2);
        assert_eq!(two_stats.details[0].len(), 0);
        assert_eq!(two_stats.details[1].len(), 0);
        assert_eq!(two_stats.len(), 0);

        // Check that interrupt statistics with lots of detailed counters work
        let many_stats = SampledData::new(256);
        assert_eq!(many_stats.details.len(), 256);
        assert_eq!(many_stats.details[0].len(), 0);
        assert_eq!(many_stats.details[255].len(), 0);
        assert_eq!(many_stats.len(), 0);
    }

    /// Check that parsing interrupt statistics works as expected
    #[test]
    fn parse_interrupt_stat() {
        // Interrupt statistics without any detail
        let mut no_details_stats = SampledData::new(0);
        no_details_stats.push_str("12345");
        assert_eq!(no_details_stats.total, vec![12345]);
        assert_eq!(no_details_stats.details.len(), 0);
        assert_eq!(no_details_stats.len(), 1);

        // Interrupt statistics with two detailed counters
        let mut two_stats = SampledData::new(2);
        two_stats.push_str("12345 678 910");
        assert_eq!(two_stats.total, vec![12345]);
        assert_eq!(two_stats.details, 
                   vec![SampledCounter::Samples(vec![678]),
                        SampledCounter::Samples(vec![910])]);
        assert_eq!(two_stats.len(), 1);
    }
}
