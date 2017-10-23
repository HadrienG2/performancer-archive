//! This module contains a sampling parser for /proc/uptime

use ::parser::PseudoFileParser;
use std::str::SplitWhitespace;
use std::time::Duration;


// Implement a sampler for /proc/uptime
define_sampler!{ Sampler : "/proc/uptime" => Parser => SampledData }


/// Incremental parser for /proc/uptime
pub struct Parser {}
//
impl PseudoFileParser for Parser {
    /// Build a parser, using initial file contents for schema analysis
    fn new(initial_contents: &str) -> Self {
        // TODO: Check that it parses as well
        let col_count = initial_contents.split_whitespace().count();
        assert!(col_count >= 2, "Uptime and idle time should be present");
        debug_assert_eq!(col_count, 2, "Unsupported entry in /proc/uptime");
        Self {}
    }
}
//
// TODO: Implement IncrementalParser once that trait is usable in stable Rust
impl Parser {
    /// Begin to parse a pseudo-file sample, streaming its data out
    fn parse<'a>(&mut self, file_contents: &'a str) -> FieldStream<'a> {
        FieldStream::new(file_contents)
    }
}
///
///
/// Stream of parsed data from /proc/uptime.
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
        self.file_columns.next().map(Self::parse_duration_secs)
    }
}
//
impl<'a> FieldStream<'a> {
    /// Specialized parser for Durations expressed in fractional seconds, using
    /// the usual text format XXXX[.[YY]]. This is about standardized data, so
    /// the input is assumed to be correct, and errors will result in panics.
    ///
    /// If this code turns out to be more generally useful, move it to a higher-
    /// level module of the crate.
    ///
    fn parse_duration_secs(input: &str) -> Duration {
        // Separate the integral part from the fractional part (if any)
        let mut integer_iter = input.split('.');

        // Parse the number of whole seconds
        let seconds : u64
            = integer_iter.next().expect("Input should not be empty")
                          .parse().expect("Input should be a second counter");

        // Parse the number of extra nanoseconds, if any
        let nanoseconds = match integer_iter.next() {
            // No decimals or a trailing decimal point means no nanoseconds.
            Some("") | None => 0,

            // If there is something after the ., assume it is decimals. Sub
            // nanosecond decimals are unsupported and will be truncated.
            Some(mut decimals) => {
                debug_assert!(decimals.chars().all(|c| c.is_digit(10)),
                              "Non-digit character detected inside decimals");
                if decimals.len() > 9 { decimals = &decimals[0..9]; }
                let nanosecs_factor = 10u32.pow(9 - (decimals.len() as u32));
                let decimals_int =
                    decimals.parse::<u32>()
                            .expect("Failed to parse the fractional seconds");
                decimals_int * nanosecs_factor
            }
        };

        // At this point, we should be at the end of the string
        debug_assert_eq!(integer_iter.next(), None,
                         "Unexpected input at end of the duration string");

        // Return the Duration that we just parsed
        Duration::new(seconds, nanoseconds)
    }

    /// Set up a FieldStream for a certain sample of /proc/uptime
    fn new(file_contents: &'a str) -> Self {
        Self {
            file_columns: file_contents.split_whitespace(),
        }
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
        // TODO: That's redundant with parser initialization, remove it
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
    use super::{FieldStream, Parser, PseudoFileParser, SampledData, Sampler};

    /// Check that our Duration parser works as expected
    #[test]
    fn parse_duration() {
        // Plain seconds
        assert_eq!(FieldStream::parse_duration_secs("42"),
                   Duration::new(42, 0));

        // Trailing decimal point
        assert_eq!(FieldStream::parse_duration_secs("3."),
                   Duration::new(3, 0));

        // Some amounts of fractional seconds, down to nanosecond precision
        assert_eq!(FieldStream::parse_duration_secs("4.2"),
                   Duration::new(4, 200_000_000));
        assert_eq!(FieldStream::parse_duration_secs("5.34"),
                   Duration::new(5, 340_000_000));
        assert_eq!(FieldStream::parse_duration_secs("6.567891234"),
                   Duration::new(6, 567_891_234));

        // Sub-nanosecond precision is truncated
        assert_eq!(FieldStream::parse_duration_secs("7.8901234567"),
                   Duration::new(7, 890_123_456));
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
