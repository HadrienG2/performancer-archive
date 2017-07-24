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

use std::ascii::AsciiExt;
use std::slice;
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


// An "iterator" operating over **both** lines of text and space-separated words
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
// This can be done by combining Lines and SplitWhitespace (or optimized
// versions thereof), however each line of text must then be parsed twice: first
// by Lines in order to extract a line of text, and again by SplitWhitespace in
// order to separate the words inside of the line of text.
//
// In principle, however, both of those separations could be carried out in a
// single pass through the text by taking different actions on spaces and line
// feeds, and producing both end-of-word and end-of-line signals.
//
// The Rust Iterator API does not offer any guarantee about whether calling
// next() on an iterator which returned None continues to return None. This
// iterator exploits that undefined behaviour by defining a variant of
// SplitWhitespace which outputs None whenever a newline character is reached,
// effectively implementing some (admittedly crude) line splitting without
// needing to parse each line of the input twice.
//
// Expected usage is to call next_line() on every line, continue iterating over
// lines as long as this function returns true, and for each line use
// SplitLinesBySpace as a regular iterator over space-separated words.
//
pub struct SplitLinesBySpace<'a> {
    /// String which we are trying to split
    target: &'a str,

    /// Iterator over the characters and their byte indices
    char_iter: FastCharIndices<'a>,

    /// Where we are within the input (at the beginning of a line, somewhere
    /// inside a line, or at the end of the input string)
    status: LineSpaceSplitterStatus,
}
//
impl<'a> SplitLinesBySpace<'a> {
    /// Create a line- and space-splitting iterator
    pub fn new(target: &'a str) -> Self {
        let mut char_iter = FastCharIndices::new(target);
        let input_empty = Self::at_end_impl(&mut char_iter);
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

            // We are in the middle of a line of text. Skip it by iterating
            // until we reach either the end of that line, or that of the input.
            LineSpaceSplitterStatus::InsideLine => loop {
                match self.char_iter.next() {
                    // A newline was encountered. Check if there is text after
                    // it or it's just trailing at the end of the input.
                    Some('\n') => {
                        if self.at_end() {
                            self.status = LineSpaceSplitterStatus::AtInputEnd;
                            return false;
                        } else {
                            return true;
                        }
                    }

                    // Some other character was encountered. Continue iteration.
                    Some(_) => continue,

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

    /// INTERNAL: Tell whether we reached the end of the internal iterator
    #[inline]
    fn at_end(&mut self) -> bool {
        Self::at_end_impl(&mut self.char_iter)
    }

    /// INTERNAL: Implementation of at_end, must be separate for new() to use it
    #[inline]
    fn at_end_impl(iter: &mut FastCharIndices<'a>) -> bool {
        if let Some(item) = iter.next() {
            iter.put_back(item);
            false
        } else {
            true
        }
    }
}
//
impl<'a> Iterator for SplitLinesBySpace<'a> {
    /// We're outputting strings
    type Item = &'a str;

    /// This is how one iterates through space-separated words until a newline
    fn next(&mut self) -> Option<Self::Item> {
        // We expect the caller to have properly called next_line() beforehand
        assert_eq!(self.status, LineSpaceSplitterStatus::InsideLine);

        // Find the first non-space character before the end of line (if any):
        // that will be the start of the next word.
        let first_idx;
        loop {
            match self.char_iter.next() {
                // Discard all the spaces along the way.
                Some(' ') => continue,

                // Output a None when a newline is reached, to signal the client
                // of space-separated data that it's time to yield control back
                // to the line iterator (which we configure along the way).
                Some('\n') => {
                    self.status = if self.at_end() {
                                      LineSpaceSplitterStatus::AtInputEnd
                                  } else {
                                      LineSpaceSplitterStatus::AtLineStart
                                  };
                    return None;
                },

                // Record the index of the first non-space character
                Some(_) => {
                    first_idx = self.char_iter.last_index();
                    break;
                },

                // Terminate when the end of the text is reached
                None => {
                    self.status = LineSpaceSplitterStatus::AtInputEnd;
                    return None;
                },
            }
        }

        // We are now inside of a word, and looking for its end. From now on,
        // we need to be more careful: if the word completes at the end of the
        // current line, we will need to output two things in a row, first the
        // word, then a None. We handle that by peeking instead of iterating.
        loop {
            match self.char_iter.next() {
                // We reached the end of a word: output said word.
                Some(' ') => {
                    let last_idx = self.char_iter.last_index();
                    return Some(&self.target[first_idx..last_idx]);
                },

                // Newlines also terminate words, but we must put them back in
                // because we want to subsequently signal them as a None.
                Some('\n') => {
                    let last_idx = self.char_iter.last_index();
                    self.char_iter.put_back('\n');
                    return Some(&self.target[first_idx..last_idx]);
                }

                // We are still in the middle of the word: move on
                Some(_) => continue,

                // We reached the end of the input: output the last word
                None => return Some(&self.target[first_idx..]),
            }
        }
    }
}
///
/// State machine used for iterating over lines
#[derive(Debug, PartialEq)]
enum LineSpaceSplitterStatus { AtLineStart, InsideLine, AtInputEnd }
///
/// A conceptual cousin of PutBack<CharIndices>, heavily optimized for our needs
/// of ASCII-only parsing, frequent character lookup with infrequent index
/// lookup, and occasional putting back of a character.
struct FastCharIndices<'a> {
    /// Iterator over the characters of an ASCII string
    char_iter: slice::Iter<'a, u8>,

    /// Byte index of the _next_ character
    next_char_index: usize,

    /// Facility to put one character back into the iterator
    put_back_buf: Option<char>,
}
//
impl<'a> FastCharIndices<'a> {
    /// Initialize the iterator
    #[inline]
    fn new(input: &'a str) -> Self {
        Self {
            char_iter: input.as_bytes().iter(),
            next_char_index: 0,
            put_back_buf: None,
        }
    }

    /// Tell what was the index of the last character. Requires the knowledge
    /// of said character, which FastCharIndices does not keep around.
    #[inline]
    fn last_index(&self) -> usize {
        self.next_char_index - 1
    }

    /// Put a character back in. You should only put back one character at a
    /// time, and you should not do so before the iterator has outputted
    /// anything, and after it has outputted its final None.
    #[inline]
    fn put_back(&mut self, ch: char) {
        debug_assert_eq!(self.put_back_buf, None);
        self.put_back_buf = Some(ch);
        self.next_char_index -= 1;
    }
}
///
/// Iterate over characters (use index() to know about the character's index)
impl<'a> Iterator for FastCharIndices<'a> {
    type Item = char;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Take from the put_back_buffer if full, otherwise from the iterator
        let result = if self.put_back_buf.is_some() {
            self.put_back_buf.take()
        } else {
            self.char_iter.next().map(|b| { debug_assert!(b.is_ascii());
                                            char::from(*b) })
        };

        // Increment the character counter
        self.next_char_index += 1;

        // Return the freshly read character
        result
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use super::{SplitLinesBySpace, SplitSpace};
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

    /// Check that SplitLinesBySpace works as intended, both when skipping
    /// through lines and when exhaustively iterating through their words.
    #[test]
    fn split_lines_by_space() {
        // Split the empty string
        let mut split_empty = SplitLinesBySpace::new("");
        assert_eq!(split_empty.next_line(), false);

        // Split a string full of space
        let mut split_space = SplitLinesBySpace::new(" ");
        assert_eq!(split_space.next_line(), true);
        assert_eq!(split_space.next_line(), false);
        split_space = SplitLinesBySpace::new(" ");
        assert_eq!(split_space.next_line(), true);
        assert_eq!(split_space.next(), None);
        assert_eq!(split_space.next_line(), false);

        // Split a newline
        let mut split_newline = SplitLinesBySpace::new("\n");
        assert_eq!(split_newline.next_line(), true);
        assert_eq!(split_newline.next_line(), false);
        split_newline = SplitLinesBySpace::new("\n");
        assert_eq!(split_newline.next_line(), true);
        assert_eq!(split_newline.next(), None);
        assert_eq!(split_newline.next_line(), false);

        // Split a single word
        let mut split_word = SplitLinesBySpace::new("42");
        assert_eq!(split_word.next_line(), true);
        assert_eq!(split_word.next_line(), false);
        split_word = SplitLinesBySpace::new("42");
        assert_eq!(split_word.next_line(), true);
        assert_eq!(split_word.next(), Some("42"));
        assert_eq!(split_word.next(), None);
        assert_eq!(split_word.next_line(), false);

        // Split a word preceded by spaces
        let mut split_space_word = SplitLinesBySpace::new("  24");
        assert_eq!(split_space_word.next_line(), true);
        assert_eq!(split_space_word.next_line(), false);
        split_space_word = SplitLinesBySpace::new("  24");
        assert_eq!(split_space_word.next_line(), true);
        assert_eq!(split_space_word.next(), Some("24"));
        assert_eq!(split_space_word.next(), None);
        assert_eq!(split_space_word.next_line(), false);

        // Split a word preceded by a newline
        let mut split_newline_word = SplitLinesBySpace::new("\nabc123");
        assert_eq!(split_newline_word.next_line(), true);
        assert_eq!(split_newline_word.next_line(), true);
        assert_eq!(split_newline_word.next_line(), false);
        split_newline_word = SplitLinesBySpace::new("\nabc123");
        assert_eq!(split_newline_word.next_line(), true);
        assert_eq!(split_newline_word.next(), None);
        assert_eq!(split_newline_word.next_line(), true);
        assert_eq!(split_newline_word.next(), Some("abc123"));
        assert_eq!(split_newline_word.next(), None);
        assert_eq!(split_newline_word.next_line(), false);

        // Split a word followed by spaces
        let mut split_word_space = SplitLinesBySpace::new("viwb ");
        assert_eq!(split_word_space.next_line(), true);
        assert_eq!(split_word_space.next_line(), false);
        split_word_space = SplitLinesBySpace::new("viwb ");
        assert_eq!(split_word_space.next_line(), true);
        assert_eq!(split_word_space.next(), Some("viwb"));
        assert_eq!(split_word_space.next(), None);
        assert_eq!(split_word_space.next_line(), false);

        // Split a word followed by a newline
        let mut split_word_newline = SplitLinesBySpace::new("g1s13\n");
        assert_eq!(split_word_newline.next_line(), true);
        assert_eq!(split_word_newline.next_line(), false);
        split_word_newline = SplitLinesBySpace::new("g1s13\n");
        assert_eq!(split_word_newline.next_line(), true);
        assert_eq!(split_word_newline.next(), Some("g1s13"));
        assert_eq!(split_word_newline.next(), None);
        assert_eq!(split_word_newline.next_line(), false);

        // Split three words with spaces and newlines
        let mut split_everything = SplitLinesBySpace::new("  s( é \n o,p");
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next_line(), false);
        split_everything = SplitLinesBySpace::new("  s( é \n o,p");
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next(), Some("s("));
        assert_eq!(split_everything.next(), Some("é"));
        assert_eq!(split_everything.next(), None);
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next(), Some("o,p"));
        assert_eq!(split_everything.next(), None);
        assert_eq!(split_everything.next_line(), false);
    }
}
