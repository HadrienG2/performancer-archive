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

pub mod meminfo;
pub mod stat;
pub mod uptime;
pub mod version;

use std::iter::Peekable;
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
            let decimals_int =
                decimals.parse::<u32>()
                        .expect("Failed to parse the fractional seconds");
            decimals_int * nanosecs_multiplier
        }
    };

    // At this point, we should be at the end of the string
    debug_assert_eq!(integer_iter.next(), None,
                     "Unexpected input found at end of the duration string");

    // Return the Duration that we just parsed
    Duration::new(seconds, nanoseconds)
}


/// Fast Lines specialization for Unix line separators (\n)
///
/// The meminfo parser seems to be bottlenecked by the underlying line iterator.
/// I'm not sure whether there is as much to gain here as with SplitWhitespace,
/// but I guess it's worth trying anyhow.
///
struct UnixLines<'a> {
    /// String which we are trying to split
    target: &'a str,

    /// Iterator over the characters and their byte indices
    char_iter: CharIndices<'a>,

    /// Byte index which represents the start of the next line (if any)
    first_idx: usize,
}
//
impl<'a> UnixLines<'a> {
    /// Create a line-splitting iterator
    fn new(target: &'a str) -> Self {
        Self {
            target,
            char_iter: target.char_indices(),
            first_idx: 0,
        }
    }
}
//
impl<'a> Iterator for UnixLines<'a> {
    /// We're outputting strings
    type Item = &'a str;

    /// This is how one iterates through the Unix lines
    fn next(&mut self) -> Option<Self::Item> {
        // Last index where we observed a character (initially invalid)
        let mut last_idx = usize::max_value();

        // As long as we see new characters in the string...
        while let Some((idx, ch)) = self.char_iter.next() {
            // ...we can update the aforementioned index (should have zero cost,
            // as the Rust compiler should optimize it as a renaming)
            last_idx = idx;

            // ...and look for line feeds, which end the active line
            if ch == '\n' {
                let result = &self.target[self.first_idx..last_idx];
                self.first_idx = last_idx + ch.len_utf8();
                return Some(result);
            }
        }

        // The reason why we need to do the above gymnastics with indices is
        // that the definition of Lines allows the last line to be terminated
        // by either a trailing newline (without any character after it) or by
        // the end of the string, and we need to disambiguate.
        if last_idx == usize::max_value() {
            return None;
        } else {
            return Some(&self.target[self.first_idx..]);
        }
    }
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
    fn next(&mut self) -> Option<Self::Item> {
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


// An iterator over **both** lines of text and space-separated words
//
// Files in procfs often attribute different semantics to spaces and line feeds.
// Line feeds are used to separate high-level metadata (e.g. CPU vs RAM) whereas
// spaces are used to separate different details of one piece of metadata (e.g.
// different kinds of CPU/RAM consumption).
//
// This means that in order to parse a procfs file, one needs to iterate at two
// hierarchical levels: at the level of lines of text, and at the level of
// space-separated "words" within a line of text.
//
// This can be done by combining Lines (or UnixLines) and SplitWhitespace (or
// SplitSpace), however each line of text must then be parsed twice: first by
// Lines in order to extract a line of text, and again by SplitWhitespace in
// order to separate the words inside of the line of text.
//
// In principle, however, both of those separations could be carried out in a
// single pass through the text by taking different actions on spaces and line
// feeds, and producing both end-of-word and end-of-line signals.
//
// The Rust Iterator API does not offer any guarantee about whether calling
// next() on an iterator which returned None continues to return None. This
// iterator exploits that undefined behaviour by defining a version of
// SplitSpaces which outputs None whenever a newline character is reached,
// effectively implementing some (admittedly crude) line splitting without
// needing to parse each line of the input twice.
//
// A boolean flag is provided to disambiguate whether a None indicates the end
// of a line of text or the end of the input string.
//
pub struct SplitLinesBySpace<'a> {
    /// String which we are trying to split
    target: &'a str,

    /// Iterator over the characters and their byte indices
    char_iter: Peekable<CharIndices<'a>>,

    /// Where we are within the input (at the beginning of a line, somewhere
    /// inside a line, or at the end of the input string)
    status: LineSpaceSplitterStatus,
}
//
impl<'a> SplitLinesBySpace<'a> {
    /// Create a line- and space-splitting iterator
    pub fn new(target: &'a str) -> Self {
        let mut char_iter = target.char_indices().peekable();
        let input_empty = char_iter.peek().is_none();
        Self {
            target,
            char_iter,
            status: if input_empty {
                        LineSpaceSplitterStatus::AtInputEnd
                    } else {
                        LineSpaceSplitterStatus::AtLineStart
                    },
        }
    }

    /// Try to go to the beginning of the next line. Return true if successful,
    /// false if we reached the end of the file and there is no next line.
    pub fn next_line(&mut self) -> bool {
        match self.status {
            // We are at the beginning of a line of text. Tell the client that
            // it can parse it, and be ready to skip it on the next call.
            LineSpaceSplitterStatus::AtLineStart => {
                self.status = LineSpaceSplitterStatus::InsideLine;
                return true;
            },

            // We are in the middle of a line of text. Iterate until we reach
            // either the end of that line, or that of the input.
            LineSpaceSplitterStatus::InsideLine => loop {
                match self.char_iter.next() {
                    // A newline was encountered. Check if there is text after
                    // it or it's just trailing at the end of the input.
                    Some((_, '\n')) => {
                        if self.char_iter.peek().is_some() {
                            return true;
                        } else {
                            self.status = LineSpaceSplitterStatus::AtInputEnd;
                            return false;
                        }
                    }

                    // Another character was encountered. Continue iteration.
                    Some((_, _)) => continue,

                    // We reached the end of the input, and will stop there.
                    None => {
                        self.status = LineSpaceSplitterStatus::AtInputEnd;
                        return false;
                    },
                }
            },

            // There is no next line, we are at the end of the input string
            LineSpaceSplitterStatus::AtInputEnd => return false,
        }
    }
}
//
impl<'a> Iterator for SplitLinesBySpace<'a> {
    /// We're outputting strings
    type Item = &'a str;

    /// This is how one iterates through space-separated words until a newline
    fn next(&mut self) -> Option<Self::Item> {
        // Find the first non-space character before the end of line (if any)
        let first_idx;
        loop {
            match self.char_iter.next() {
                // Ignore spaces
                Some((_, ' ')) => {
                    continue;
                },

                // Output a None when a newline is reached, notify the line
                // iterator, and tell it whether more input will be coming.
                Some((_, '\n')) => {
                    self.status = if self.char_iter.peek().is_some() {
                                      LineSpaceSplitterStatus::AtInputEnd
                                  } else {
                                      LineSpaceSplitterStatus::AtLineStart 
                                  };
                    return None;
                },

                // Record the index of the first non-space character
                Some((idx, _)) => {
                    first_idx = idx;
                    break;
                },

                // Terminate when the end of the text is reached
                None => {
                    self.status = LineSpaceSplitterStatus::AtInputEnd;
                    return None;
                },
            }
        }

        // Look for a space or newline as a word terminator. Do not swallow
        // newlines or None, as we must produce None on the next iteration.
        while let Some(&(idx, ch)) = self.char_iter.peek() {
            if (ch == ' ') || (ch == '\n') {
                return Some(&self.target[first_idx..idx]);
            } else {
                self.char_iter.next();
            }
        }

        // If we see the end of the string, that will be our terminator
        return Some(&self.target[first_idx..]);
    }
}
//
enum LineSpaceSplitterStatus { AtLineStart, InsideLine, AtInputEnd }


/// Unit tests
#[cfg(test)]
mod tests {
    use super::{UnixLines, SplitLinesBySpace, SplitSpace};
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

    /// Check that UnixLines works as intended
    #[test]
    fn unix_lines() {
        // An empty string
        let mut lines_empty = UnixLines::new("");
        assert_eq!(lines_empty.next(), None);

        // Some text without a trailing newline
        let mut lines_text = UnixLines::new("abc123");
        assert_eq!(lines_text.next(), Some("abc123"));
        assert_eq!(lines_text.next(), None);

        // A lone newline
        let mut lines_newline = UnixLines::new("\n");
        assert_eq!(lines_newline.next(), Some(""));
        assert_eq!(lines_newline.next(), None);

        // Some text with a trailing newline
        let mut lines_trailing = UnixLines::new("azertyuiop\n");
        assert_eq!(lines_trailing.next(), Some("azertyuiop"));
        assert_eq!(lines_trailing.next(), None);

        // Two lines of text without a trailing newline
        let mut lines_double = UnixLines::new("aonghosi\nsongroq");
        assert_eq!(lines_double.next(), Some("aonghosi"));
        assert_eq!(lines_double.next(), Some("songroq"));
        assert_eq!(lines_double.next(), None);

        // Two lines of text with a trailing newline
        let mut lines_double_trailing = UnixLines::new("seinjha\nhzq4w1\n");
        assert_eq!(lines_double_trailing.next(), Some("seinjha"));
        assert_eq!(lines_double_trailing.next(), Some("hzq4w1"));
        assert_eq!(lines_double_trailing.next(), None);
    }

    /// Check that SplitSpace works as intended
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

    /// Check that SplitLinesBySpace works as intended
    #[test]
    fn split_lines_by_space() {
        // Split the empty string
        let mut split_empty = SplitLinesBySpace::new("");
        assert_eq!(split_empty.next_line(), false);

        // Split a string full of space
        let mut split_space = SplitLinesBySpace::new(" ");
        assert_eq!(split_space.next_line(), true);
        assert_eq!(split_space.next(), None);
        assert_eq!(split_space.next_line(), false);

        // Split a newline
        let mut split_newline = SplitLinesBySpace::new("\n");
        assert_eq!(split_newline.next_line(), true);
        assert_eq!(split_newline.next(), None);
        assert_eq!(split_newline.next_line(), false);

        // Split a single word
        let mut split_word = SplitLinesBySpace::new("42");
        assert_eq!(split_word.next_line(), true);
        assert_eq!(split_word.next(), Some("42"));
        assert_eq!(split_word.next(), None);
        assert_eq!(split_word.next_line(), false);

        // Split a word preceded by spaces
        let mut split_space_word = SplitLinesBySpace::new("  24");
        assert_eq!(split_space_word.next_line(), true);
        assert_eq!(split_space_word.next(), Some("24"));
        assert_eq!(split_space_word.next(), None);
        assert_eq!(split_space_word.next_line(), false);

        // Split a word preceded by a newline
        let mut split_newline_word = SplitLinesBySpace::new("\nabc123");
        assert_eq!(split_newline_word.next_line(), true);
        assert_eq!(split_newline_word.next(), None);
        assert_eq!(split_newline_word.next_line(), true);
        assert_eq!(split_newline_word.next(), Some("abc123"));
        assert_eq!(split_newline_word.next(), None);
        assert_eq!(split_newline_word.next_line(), false);

        // Split a word followed by spaces
        let mut split_word_space = SplitLinesBySpace::new("viwb ");
        assert_eq!(split_word_space.next_line(), true);
        assert_eq!(split_word_space.next(), Some("viwb"));
        assert_eq!(split_word_space.next(), None);
        assert_eq!(split_word_space.next_line(), false);

        // Split a word followed by a newline
        let mut split_word_newline = SplitLinesBySpace::new("g1s13\n");
        assert_eq!(split_word_newline.next_line(), true);
        assert_eq!(split_word_newline.next(), Some("g1s13"));
        assert_eq!(split_word_newline.next(), None);
        assert_eq!(split_word_newline.next_line(), false);

        // Split three words with spaces and newlines
        let mut split_everything = SplitLinesBySpace::new("  s( é \n o,p");
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next(), Some("s("));
        assert_eq!(split_everything.next(), Some("é"));
        assert_eq!(split_everything.next(), None);
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next(), Some("o,p"));
        assert_eq!(split_everything.next(), None);
        assert_eq!(split_everything.next_line(), false);

        // TODO: Also check that everything goes well when the client calls
        //       next_line less carefully.
    }
}
