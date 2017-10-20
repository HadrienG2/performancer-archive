//! This module contains facilities for parsing and storing the data contained
//! in the IRQ statistics of /proc/stat (intr and softirq).

use ::splitter::SplitColumns;
use super::StatDataStore;


/// Interrupt statistics record from /proc/stat
pub(super) struct RecordFields<'a, 'b> where 'a: 'b {
    /// Total amount of interrupt requests that were serviced
    pub total: u64,

    /// Breakdown of the interrupt requests per numbered interrupt source.
    pub details: DetailsIter<'a, 'b>,
}
//
impl<'a, 'b> RecordFields<'a, 'b> {
    /// Build a new parser for interrupt record fields
    pub fn new(mut data_columns: SplitColumns<'a, 'b>) -> Self {
        Self {
            total: data_columns.next().expect("Expected total IRQ counter")
                               .parse().expect("Failed to parse IRQ total"),
            details: DetailsIter { data_columns },
        }
    }
}
///
/// Breakdown of IRQ counts per numbered interrupt source.
/// Beware that not all interrupt sources are numbered by the Linux kernel.
pub(super) struct DetailsIter<'a, 'b> where 'a: 'b {
    /// Data columns of the record, interpreted as numbered IRQs
    data_columns: SplitColumns<'a, 'b>,
}
//
impl<'a, 'b> Iterator for DetailsIter<'a, 'b> {
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
                str_counter.parse().expect("Failed to parse IRQ counter")
            }
        })
    }
}


/// Interrupt statistics from /proc/stat, in structure-of-array layout
#[derive(Clone, Debug, PartialEq)]
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
    pub fn new(fields: RecordFields) -> Self {
        Self {
            total: Vec::new(),
            details: vec![SampledCounter::new(); fields.details.count()],
        }
    }

    /// Parse interrupt statistics and add them to the internal data store
    pub fn push(&mut self, fields: RecordFields) {
        // Load the total interrupt count
        self.total.push(fields.total);

        // Load the detailed interrupt counts from each source
        let mut details_iter = fields.details;
        for detail in self.details.iter_mut() {
            detail.push(details_iter.next()
                                    .expect("An IRQ counter went missing"));
        }

        // At this point, we should have loaded all available stats
        debug_assert!(details_iter.next().is_none(),
                      "An IRQ counter appeared out of nowhere");
    }
}
//
impl StatDataStore for SampledData {
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
    fn push(&mut self, intr_count: u64) {
        match *self {
            // Have we only seen zeroes so far?
            SampledCounter::Zeroes(zero_count) => {
                // Are we seeing a zero again?
                if intr_count == 0 {
                    // If yes, just increment the zero counter
                    *self = SampledCounter::Zeroes(zero_count+1);
                } else {
                    // If not, move to regular interrupt count sampling
                    let mut samples = vec![0; zero_count];
                    samples.push(intr_count);
                    *self = SampledCounter::Samples(samples);
                }
            },

            // If the interrupt counter is nonzero, sample it normally
            SampledCounter::Samples(ref mut vec) => {
                vec.push(intr_count);
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
    use ::splitter::split_line_and_run;
    use super::{DetailsIter, RecordFields, SampledCounter, SampledData,
                StatDataStore};

    /// Check that the detailed interrupt count parser works, and that its
    /// optimization for zero interrupt counts does not mess things up
    #[test]
    fn details_iter() {
        split_line_and_run("0 1 56 0 98 0 11 36856", |data_columns| {
            let mut details_iter = DetailsIter { data_columns };
            assert_eq!(details_iter.next(), Some(0));
            assert_eq!(details_iter.next(), Some(1));
            assert_eq!(details_iter.next(), Some(56));
            assert_eq!(details_iter.next(), Some(0));
            assert_eq!(details_iter.next(), Some(98));
            assert_eq!(details_iter.next(), Some(0));
            assert_eq!(details_iter.next(), Some(11));
            assert_eq!(details_iter.next(), Some(36856));
            assert_eq!(details_iter.next(), None);
        })
    }

    /// Check that overall, interrupt statistics are parsed well
    #[test]
    fn record_fields() {
        with_record_fields("666 42 0", |mut fields| {
            assert_eq!(fields.total, 666);
            assert_eq!(fields.details.next(), Some(42));
            assert_eq!(fields.details.next(), Some(0));
            assert_eq!(fields.details.next(), None);
        });
    }

    /// Check that interrupt count samples work well, zero-optimization included
    #[test]
    fn sampled_counter() {
        // Initial sampler state
        let mut samples = SampledCounter::new();
        assert_eq!(samples, SampledCounter::Zeroes(0));
        assert_eq!(samples.len(), 0);

        // Pushing zeroes keeps us in the zero-optimized state
        samples.push(0);
        assert_eq!(samples, SampledCounter::Zeroes(1));
        assert_eq!(samples.len(), 1);
        samples.push(0);
        assert_eq!(samples, SampledCounter::Zeroes(2));
        assert_eq!(samples.len(), 2);

        // Pushing nonzero values gets us out of it correctly
        samples.push(69);
        assert_eq!(samples, SampledCounter::Samples(vec![0, 0, 69]));
        assert_eq!(samples.len(), 3);

        // We don't incorrectly get back to it if we push zero again
        samples.push(0);
        assert_eq!(samples, SampledCounter::Samples(vec![0, 0, 69, 0]));
        assert_eq!(samples.len(), 4);

        // Subsequent pushes work just as well
        samples.push(27);
        assert_eq!(samples, SampledCounter::Samples(vec![0, 0, 69, 0, 27]));
        assert_eq!(samples.len(), 5);
    }

    /// Check that full interrupt samples work well
    #[test]
    fn sampled_data() {
        // Check that initialization works
        let mut data = with_record_fields("666 0 24", SampledData::new);
        assert_eq!(data.total, Vec::new());
        assert_eq!(data.details.len(), 2);
        assert_eq!(data.len(), 0);

        // Check that subsequent pushes work as expected
        with_record_fields("669 0 26", |fields| data.push(fields));
        assert_eq!(data.total, vec![669]);
        assert_eq!(data.details, vec![SampledCounter::Zeroes(1),
                                      SampledCounter::Samples(vec![26])]);
        assert_eq!(data.len(), 1);
        with_record_fields("782 66 42", |fields| data.push(fields));
        assert_eq!(data.total, vec![669, 782]);
        assert_eq!(data.details, vec![SampledCounter::Samples(vec![0,  66]),
                                      SampledCounter::Samples(vec![26, 42])]);
        assert_eq!(data.len(), 2);
    }

    /// Build the interrupt record fields associated with a line of text, and
    /// run code taking that as a parameter
    fn with_record_fields<F, R>(line_of_text: &str, functor: F) -> R
        where F: FnOnce(RecordFields) -> R
    {
        split_line_and_run(line_of_text, |columns| {
            let fields = RecordFields::new(columns);
            functor(fields)
        })
    }
}
