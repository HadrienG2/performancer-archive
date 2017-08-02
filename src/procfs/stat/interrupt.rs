//! This module contains facilities for parsing and storing the data contained
//! in the IRQ statistics of /proc/stat (intr and softirq).

use ::splitter::SplitLinesBySpace;
use super::StatDataStore;


/// Interrupt statistics from /proc/stat, in structure-of-array layout
// TODO: This should be pub(super), waiting for next rustc version...
#[derive(Debug, PartialEq)]
pub struct InterruptStatData {
    /// Total number of interrupts that were serviced. May be higher than the
    /// sum of the breakdown below if there are unnumbered interrupt sources.
    total: Vec<u64>,

    /// For each numbered source, details on the amount of serviced interrupt.
    details: Vec<InterruptCounts>
}
//
impl InterruptStatData {
    /// Create new interrupt statistics, given the amount of interrupt sources
    /// TODO: This should be pub(super), waiting for next rustc version...
    pub fn new(num_irqs: u16) -> Self {
        Self {
            total: Vec::new(),
            details: vec![InterruptCounts::new(); num_irqs as usize],
        }
    }
}
//
impl StatDataStore for InterruptStatData {
    /// Parse interrupt statistics and add them to the internal data store
    fn push(&mut self, stats: &mut SplitLinesBySpace) {
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
/// On some platforms such as x86, there are a lot of hardware IRQs (~500 on my
/// machines), but most of them are unused and never fire. Parsing and storing
/// the associated zeroes from /proc/stat by normal means wastes CPU time and
/// RAM, so we take a shortcut for this common use case.
///
#[derive(Clone, Debug, PartialEq)]
enum InterruptCounts {
    /// If we've only ever seen zeroes, we only count the number of zeroes
    Zeroes(usize),

    /// Otherwise, we sample the interrupt counts normally
    Samples(Vec<u64>),
}
//
impl InterruptCounts {
    /// Initialize the interrupt count sampler
    fn new() -> Self {
        InterruptCounts::Zeroes(0)
    }

    /// Insert a new interrupt count from /proc/stat
    fn push(&mut self, intr_count: &str) {
        match *self {
            // Have we only seen zeroes so far?
            InterruptCounts::Zeroes(zero_count) => {
                // Are we seeing a zero again?
                if intr_count == "0" {
                    // If yes, just increment the zero counter
                    *self = InterruptCounts::Zeroes(zero_count+1);
                } else {
                    // If not, move to regular interrupt count sampling
                    let mut samples = vec![0; zero_count];
                    samples.push(
                        intr_count.parse().expect("Failed to parse IRQ count")
                    );
                    *self = InterruptCounts::Samples(samples);
                }
            },

            // If the interrupt counter is nonzero, sample it normally
            InterruptCounts::Samples(ref mut vec) => {
                vec.push(intr_count.parse()
                                   .expect("Failed to parse IRQ count"));
            }
        }
    }

    /// Tell how many interrupt counts we have recorded so far
    #[cfg(test)]
    fn len(&self) -> usize {
        match *self {
            InterruptCounts::Zeroes(zero_count) => zero_count,
            InterruptCounts::Samples(ref vec) => vec.len(),
        }
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use ::splitter::split_line;
    use super::{InterruptCounts, InterruptStatData, StatDataStore};

    /// Check that initializing an interrupt count sampler works as expected
    #[test]
    fn init_interrupt_counts() {
        let counts = InterruptCounts::new();
        assert_eq!(counts, InterruptCounts::Zeroes(0));
        assert_eq!(counts.len(), 0);
    }

    /// Check that interrupt count sampling works as expected
    #[test]
    fn parse_interrupt_counts() {
        // Adding one zero should keep us in the base "zeroes" state
        let mut counts = InterruptCounts::new();
        counts.push("0");
        assert_eq!(counts, InterruptCounts::Zeroes(1));
        assert_eq!(counts.len(), 1);

        // Adding a nonzero value should get us out of this state
        counts.push("123");
        assert_eq!(counts, InterruptCounts::Samples(vec![0, 123]));
        assert_eq!(counts.len(), 2);

        // After that, sampling should work normally
        counts.push("456");
        assert_eq!(counts, InterruptCounts::Samples(vec![0, 123, 456]));
        assert_eq!(counts.len(), 3);

        // Sampling right from the start should work as well
        let mut counts2 = InterruptCounts::new();
        counts2.push("789");
        assert_eq!(counts2, InterruptCounts::Samples(vec![789]));
        assert_eq!(counts2.len(), 1);
    }

    /// Check that interrupt statistics initialization works as expected
    #[test]
    fn init_interrupt_stat() {
        // Check that interrupt statistics without any details work
        let no_details_stats = InterruptStatData::new(0);
        assert_eq!(no_details_stats.total.len(), 0);
        assert_eq!(no_details_stats.details.len(), 0);
        assert_eq!(no_details_stats.len(), 0);

        // Check that interrupt statistics with two detailed counters work
        let two_stats = InterruptStatData::new(2);
        assert_eq!(two_stats.details.len(), 2);
        assert_eq!(two_stats.details[0].len(), 0);
        assert_eq!(two_stats.details[1].len(), 0);
        assert_eq!(two_stats.len(), 0);

        // Check that interrupt statistics with lots of detailed counters work
        let many_stats = InterruptStatData::new(256);
        assert_eq!(many_stats.details.len(), 256);
        assert_eq!(many_stats.details[0].len(), 0);
        assert_eq!(many_stats.details[255].len(), 0);
        assert_eq!(many_stats.len(), 0);
    }

    /// Check that parsing interrupt statistics works as expected
    #[test]
    fn parse_interrupt_stat() {
        // Interrupt statistics without any detail
        let mut no_details_stats = InterruptStatData::new(0);
        no_details_stats.push(&mut split_line("12345"));
        assert_eq!(no_details_stats.total, vec![12345]);
        assert_eq!(no_details_stats.details.len(), 0);
        assert_eq!(no_details_stats.len(), 1);

        // Interrupt statistics with two detailed counters
        let mut two_stats = InterruptStatData::new(2);
        two_stats.push(&mut split_line("12345 678 910"));
        assert_eq!(two_stats.total, vec![12345]);
        assert_eq!(two_stats.details, 
                   vec![InterruptCounts::Samples(vec![678]),
                        InterruptCounts::Samples(vec![910])]);
        assert_eq!(two_stats.len(), 1);
    }
}
