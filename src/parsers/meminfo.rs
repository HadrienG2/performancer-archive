//! This module contains a sampling parser for /proc/meminfo

use parsers::SplitSpace;

// TODO: Mechanism for sampling measurements from /proc/meminfo
// TODO: Data samples from /proc/meminfo, in structure-of-array layout

/// As /proc/meminfo is basically a (large) set of named data volumes and
/// performance counters, it maps very well to a homogeneous collection (with
/// just an enum inside to disambiguate between both).
///
/// There is, however, a catch: for fast sampling, we want to be able to iterate
/// over the records in the order in which they appear in /proc/meminfo. But for
/// fast lookup, we want to be able to quickly find a certain entry. We resolve
/// this dilemma by using a Vec for fast ordered access to the measurements
/// during sampling, and a HashSet index for fast unordered lookup.
///
struct MemInfoData {
    // Sampled meminfo records, in the order in which they appear in the file
    records: Vec<MemInfoRecord>,

    // Unordered index into the meminfo records
    // TODO: Is this really right from an ownership PoV?
    //       And if not, where should the keys go?
    index: HashMap<&str, usize>,
}
// TODO: Impl this... and report unsupported fields in debug mode!


/// Sampled records from /proc/meminfo, which can measure different things:
enum MemInfoRecord {
    // A volume of data in kibibytes (1 KiB = 1024 bytes)
    // TODO: Investigate crates for handling data volumes
    KiB(Vec<u64>),

    // A raw counter of something (e.g. free huge pages)
    Count(Vec<u64>),

    // Something unsupported by this parser :-(
    Unsupported,
}
//
impl MemInfoRecord {
    // Create a new record, choosing the type based on some raw data
    fn new(mut raw_data: SplitSpace) -> Self {
        // The raw data should start with a numerical field. Make sure that we
        // can parse it. Otherwise, we don't support the associated content.
        let number_parse_result = raw_data.next().unwrap().parse::<u64>();

        // The number may or may not come with a suffix which clarifies its
        // semantics: is it just a raw counter, or some volume of data?
        match (number_parse_result, raw_data.next()) {
            // It's a volume of data (and the kernel cannot be trusted on units)
            (Ok(_), Some("kB")) => {
                debug_assert_eq!(raw_data.next(), None);
                MemInfoRecord::KiB(Vec::new())
            },

            // It's a raw counter without any special semantics attached to it
            (Ok(_), None) => MemInfoRecord::Count(Vec::new()),

            // It's something we don't know how to parse
            _ => MemInfoRecord::Unsupported,
        }
    }
}

// TODO: Unit tests for this module

/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    use ::ProcFileReader;
    use testbench;

    /// Benchmark for the raw meminfo readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader = ProcFileReader::open("/proc/meminfo").unwrap();
        testbench::benchmark(400_000, || {
            reader.sample(|_| {}).unwrap();
        });
    }

    // TODO: Benchmark for the full meminfo sampling overhead
}
