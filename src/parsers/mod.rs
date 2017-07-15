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

pub mod stat;
pub mod uptime;
pub mod version;

use std::str::CharIndices;
use std::time::Duration;


/// Specialized parser for Durations expressed in fractional seconds, using the
/// usual text format XXXX[.[YY]]. This is about parsing standardized data, so
/// the input is assumed to be correct, and errors will be handled via panics.
fn parse_duration_secs(input: &str) -> Duration {
    // Separate the integral part from the fractional part (if any)
    let mut integer_iter = input.split('.');

    // Parse the number of whole seconds
    let seconds
        = integer_iter.next().expect("Input string should not be empty")
                      .parse::<u64>().expect("Input should parse as seconds");

    // Parse the number of extra nanoseconds, if any
    let nanoseconds = match integer_iter.next() {
        // No decimals means no nanoseconds. Allow for a trailing decimal point.
        Some("") | None    => 0,

        // If there is something after the ., assume it is decimals. Sub nano-
        // second decimals will be truncated: Rust only understands nanosecs.
        Some(mut decimals) => {
            debug_assert!(decimals.chars().all(|c| c.is_digit(10)),
                          "Only digits are expected after the decimal point");
            if decimals.len() > 9 { decimals = &decimals[0..9]; }
            let nanosecs_multiplier = 10u32.pow(9 - (decimals.len() as u32));
            decimals.parse::<u32>().unwrap() * nanosecs_multiplier
        }
    };

    // At this point, we should be at the end of the string
    debug_assert_eq!(integer_iter.next(), None,
                     "Unexpected input found at end of the duration string");

    // Return the Duration that we just parsed
    Duration::new(seconds, nanoseconds)
}


/// Fast SplitWhitespace specialization for space-separated strings
///
/// The SplitWhitespace iterator from the Rust standard library is great for
/// general-purpose Unicode string parsing, but on the space-separated ASCII
/// lines from /proc, it can spend a lot of time looking for exotic whitespace
/// characters that will never show up. On complex files like /proc/stat, this
/// can become a performance killer.
///
/// This implementation is optimized for such contents by building on the
/// assumption that the only kind of separator that can appear is spaces.
///
/// Note that this means, in particular, that it does *not* understand newlines.
/// In a sense, it operates under the assumption that there is already some
/// newline separator like string.lines() operating on top of it.
///
struct SplitSpace<'a> {
    /// String which we are trying to split
    target: &'a str,

    /// Iterator over the characters and their byte indices
    char_iter: CharIndices<'a>,
}
//
impl<'a> SplitSpace<'a> {
    /// Create a space-splitting iterator
    fn new(target: &'a str) -> Self {
        Self {
            target,
            char_iter: target.char_indices(),
        }
    }
}
//
impl<'a> Iterator for SplitSpace<'a> {
    /// We're outputting strings
    type Item = &'a str;

    /// This is how one iterates the space-separated words
    fn next(&mut self) -> Option<&'a str> {
        // Find the first non-space character (if any)
        let first_idx;
        loop {
            match self.char_iter.next() {
                // Ignore spaces
                Some((_, ' ')) => continue,

                // Record the index of the first non-space character
                Some((idx, _)) => {
                    first_idx = idx;
                    break;
                },

                // If there are no words left, return None
                None => return None,
            }
        }

        // Look for a space as a word terminator
        while let Some((idx, ch)) = self.char_iter.next() {
            if ch == ' ' {
                return Some(&self.target[first_idx..idx]);
            }
        }

        // If no space shows up, the end of the string is our terminator
        return Some(&self.target[first_idx..]);
    }
}


/// These are the unit tests for this module
#[cfg(test)]
mod tests {
    use super::SplitSpace;
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

    // Check that SplitSpace works as intended
    #[test]
    fn split_space() {
        // Split the empty string
        let mut split_empty = SplitSpace::new("");
        assert_eq!(split_empty.next(), None);

        // Split a string full of space
        let mut split_space = SplitSpace::new("   ");
        assert_eq!(split_space.next(), None);

        // Split a string containing only a word
        let mut split_word = SplitSpace::new("42");
        assert_eq!(split_word.next(), Some("42"));
        assert_eq!(split_word.next(), None);

        // ...with leading space
        let mut split_leading_space = SplitSpace::new("  42");
        assert_eq!(split_leading_space.next(), Some("42"));
        assert_eq!(split_leading_space.next(), None);

        // ...with trailing space
        let mut split_trailing_space = SplitSpace::new("  42 ");
        assert_eq!(split_trailing_space.next(), Some("42"));
        assert_eq!(split_trailing_space.next(), None);

        // Split a string containing two words
        let mut split_two_words = SplitSpace::new("42 43");
        assert_eq!(split_two_words.next(), Some("42"));
        assert_eq!(split_two_words.next(), Some("43"));
        assert_eq!(split_two_words.next(), None);

        // ...with leading and trailing space
        let mut split_trailing_pair = SplitSpace::new("  42 43 ");
        assert_eq!(split_trailing_pair.next(), Some("42"));
        assert_eq!(split_trailing_pair.next(), Some("43"));
        assert_eq!(split_trailing_pair.next(), None);
    }
}
