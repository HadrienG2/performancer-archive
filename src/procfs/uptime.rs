//! This module contains a sampling parser for /proc/uptime

use ::procfs;
use std::str::SplitWhitespace;
use std::time::Duration;


// Implement a sampler for /proc/uptime
define_sampler!{ Sampler : "/proc/uptime" => Parser => SampledData }


/// Streaming parser for /proc/uptime
///
/// TODO: Replace following paragraph with a real description once ready
///
/// This is an experiment towards a parser redesign that would decouple the
/// code used for parsing pseudo-file contents from the code used for storing
/// it. The goal is to determine if this would be practical and efficient.
///
/// The idea behind having two separate Parser and Stream components is that it
/// allows the parser to cache long-lived metadata about the file being parsed.
///
pub struct Parser {}
//
impl Parser {
    /// Build a parser, using initial file contents for schema analysis
    fn new(initial_contents: &str) -> Self {
        let col_count = initial_contents.split_whitespace().count();
        assert!(col_count >= 2, "Uptime and idle time should be present");
        debug_assert_eq!(col_count, 2, "Unsupported entry in /proc/uptime");
        Self {}
    }

    /// Begin to parse a pseudo-file sample, streaming its data out
    fn parse<'a>(&mut self, file_contents: &'a str) -> FieldStream<'a> {
        FieldStream {
            file_columns: file_contents.split_whitespace(),
        }
    }
}
///
///
/// Stream of parsed data from /proc/uptime
///
/// TODO: Compare and contrast the "streaming reader" approach from vitalyd,
///       where values are lazily rather than eagerly produced.
///
/// This iterator should successively yield...
///
/// * The machine uptime (wall clock time elapsed since boot)
/// * The idle time (total CPU time spent in the idle state)
/// * A None terminator
///
pub struct FieldStream<'a> {
    /// Extracted columns from /proc/uptime
    file_columns: SplitWhitespace<'a>,
}
//
impl<'a> Iterator for FieldStream<'a> {
    /// We output durations
    type Item = Duration;

    /// Parse the next duration from /proc/uptime
    fn next(&mut self) -> Option<Self::Item> {
        self.file_columns.next().map(procfs::parse_duration_secs)
    }
}


/// Data samples from /proc/uptime, in structure-of-array layout
struct SampledData {
    /// Elapsed wall clock time since the system was started
    wall_clock_uptime: Vec<Duration>,

    /// Cumulative amount of time spent by all CPUs in the idle state
    cpu_idle_time: Vec<Duration>,
}
//
impl SampledData {
    /// Create a new uptime data store
    fn new(stream: FieldStream) -> Self {
        let field_count = stream.count();
        assert!(field_count >= 2, "Missing expected entry in /proc/uptime");
        debug_assert_eq!(field_count, 2, "Unsupported entry in /proc/uptime");
        Self {
            wall_clock_uptime: Vec::new(),
            cpu_idle_time: Vec::new(),
        }
    }

    /// Push a new stream of parsed data from /proc/uptime into the store
    fn push(&mut self, mut stream: FieldStream) {
        // Start parsing our input data sample
        self.wall_clock_uptime.push(
            stream.next().expect("Machine uptime is missing")
        );
        self.cpu_idle_time.push(
            stream.next().expect("Machine idle time is missing")
        );

        // If this debug assert fails, the contents of the file have been
        // extended by a kernel revision, and the code should be updated
        debug_assert_eq!(stream.next(), None,
                         "Unsupported entry in /proc/uptime");
    }

    /// Tell how many samples are present in the data store
    #[cfg(test)]
    fn len(&self) -> usize {
        let length = self.wall_clock_uptime.len();
        debug_assert_eq!(length, self.cpu_idle_time.len());
        length
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use super::{SampledData, Parser, Sampler};

    /// Check that creating un uptime parser works
    #[test]
    fn init_parser() {
        let _ = Parser::new("56.78 12.34");
    }

    /// Check that parsing uptime data works
    #[test]
    fn parse_data() {
        let mut parser = Parser::new("10.11 12.13");
        let mut stream = parser.parse("13.52  50.34");
        assert_eq!(stream.next(), Some(Duration::new(13, 520_000_000)));
        assert_eq!(stream.next(), Some(Duration::new(50, 340_000_000)));
        assert_eq!(stream.next(), None);
    }

    /// Check that creating an uptime data store works
    #[test]
    fn init_container() {
        let initial = "16.191963 19686.615";
        let mut parser = Parser::new(initial);
        let data = SampledData::new(parser.parse(initial));
        assert_eq!(data.wall_clock_uptime.len(), 0);
        assert_eq!(data.cpu_idle_time.len(), 0);
        assert_eq!(data.len(), 0);
    }

    /// Check that parsing uptime data works
    #[test]
    fn push_data() {
        let initial = "145.16 16546.1469";
        let mut parser = Parser::new(initial);
        let mut data = SampledData::new(parser.parse(initial));
        data.push(parser.parse("614.461  10645.163"));
        assert_eq!(data.wall_clock_uptime,
                   vec![Duration::new(614, 461_000_000)]);
        assert_eq!(data.cpu_idle_time,
                   vec![Duration::new(10645, 163_000_000)]);
        assert_eq!(data.len(), 1);
    }

    /// Check that the sampler works well
    define_sampler_tests!{ Sampler }

    /// Check that the sampled uptime increases over time
    #[test]
    fn increasing_uptime() {
        // Create an uptime sampler
        let mut uptime = Sampler::new().expect("Failed to create a sampler");

        // Acquire a first sample
        uptime.sample().expect("Failed to sample uptime once");

        // Wait a bit
        thread::sleep(Duration::from_millis(50));

        // Acquire another sample
        uptime.sample().expect("Failed to sample uptime twice");

        // The uptime and idle time should have increased
        assert!(uptime.samples.wall_clock_uptime[1] >
                    uptime.samples.wall_clock_uptime[0]);
        assert!(uptime.samples.cpu_idle_time[1] >
                    uptime.samples.cpu_idle_time[0]);
    }
}


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    define_sampler_benchs!{ super::Sampler,
                            "/proc/uptime",
                            3_000_000 }
}
