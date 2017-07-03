//! This module contains sampling parsers for the contents of procfs.
//!
//! These parsers are designed to allow sampling the contents of /proc files at
//! a rapid rate, for the purpose of acquiring, analyzing and displaying useful
//! statistics on the time evolution of system performance.
//!
//! Each submodule corresponds to one file in /proc, and is named as close to
//! that file as allowed by the Rust module system. The various modules are
//! intended to work in the same way, and we will later explore avenues for
//! enforcing this interface contract and reducing code duplication through
//! code generation mechanisms.
//!
//! The top-level module currently contains utilities which are potentially
//! usable by multiple modules. If this shared utility library grows, it will be
//! extracted to a dedicated "detail" submodule.

pub mod uptime;
pub mod version;

use std::time::Duration;


/// Specialized parser for Durations expressed in fractional seconds, using the
/// usual text format XXXX[.[YY]]. This is about parsing standardized data, so
/// the input is assumed to be correct, and errors will be handled via panics.
fn parse_duration_secs(input: &str) -> Duration {
    // Separate the integral part from the fractional part (if any)
    let mut integer_iter = input.split('.');

    // Parse the number of full seconds
    let seconds = integer_iter.next().unwrap()
                              .parse::<u64>().unwrap();

    // Parse the number of extra nanoseconds, if any
    let nanoseconds = match integer_iter.next() {
        // Handle the "XXXX." syntax used by some text printers
        Some("")       => 0,

        // If there is something after the ., assume it is decimals. Sub nano-
        // second decimals will be truncated: we only count whole nanoseconds.
        Some(mut decimals) => {
            if decimals.len() > 9 { decimals = &decimals[0..9]; }
            let nanosecs_multiplier = 10u32.pow(9 - (decimals.len() as u32));
            decimals.parse::<u32>().unwrap() * nanosecs_multiplier
        }

        // No decimals means no nanoseconds
        None           => 0,
    };

    // At this point, we should be at the end of the string
    debug_assert_eq!(integer_iter.next(), None);

    // Return the Duration that we just parsed
    Duration::new(seconds, nanoseconds)
}


/// These are the unit tests for this module
#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// Check that our Duration parser works as expected
    #[test]
    fn parse_duration() {
        // Plain seconds
        assert_eq!(super::parse_duration_secs("42"),
                   Duration::new(42, 0));

        // Trailing decimal point
        assert_eq!(super::parse_duration_secs("3."),
                   Duration::new(3, 0));

        // Some amounts of fractional seconds, down to nanosecond precision
        assert_eq!(super::parse_duration_secs("4.2"),
                   Duration::new(4, 200_000_000));
        assert_eq!(super::parse_duration_secs("5.34"),
                   Duration::new(5, 340_000_000));
        assert_eq!(super::parse_duration_secs("6.567891234"),
                   Duration::new(6, 567_891_234));

        // Sub-nanosecond precision is truncated
        assert_eq!(super::parse_duration_secs("7.8901234567"),
                   Duration::new(7, 890_123_456));
    }
}
