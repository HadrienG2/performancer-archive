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
        let char_iter = FastCharIndices::new(target);
        let input_empty = char_iter.is_empty();
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
                        if self.char_iter.is_empty() {
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

    /// Consume the current line, indicating how many space-separated words were
    /// encountered, but without consuming the entire iterator.
    pub fn word_count(&mut self) -> usize {
        let mut word_count = 0usize;
        while self.next().is_some() {
            word_count += 1;
        }
        word_count
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
                    self.status = if self.char_iter.is_empty() {
                                      LineSpaceSplitterStatus::AtInputEnd
                                  } else {
                                      LineSpaceSplitterStatus::AtLineStart
                                  };
                    return None;
                },

                // Record the index of the first non-space character
                Some(_) => {
                    first_idx = self.char_iter.prev_index();
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
                    let last_idx = self.char_iter.prev_index();
                    return Some(&self.target[first_idx..last_idx]);
                },

                // Newlines also terminate words, but we must put them back in
                // because we want to subsequently signal them as a None.
                Some('\n') => {
                    let last_idx = self.char_iter.prev_index();
                    self.char_iter.back();
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
/// State machine used by SplitLinesBySpace for iterating over lines
#[derive(Debug, PartialEq)]
enum LineSpaceSplitterStatus { AtLineStart, InsideLine, AtInputEnd }
///
///
/// A conceptual cousin of PutBack<CharIndices>, heavily optimized for our needs
/// of ASCII-only parsing, frequent character lookup with infrequent index
/// lookup, and occasional backtracking on a character.
///
struct FastCharIndices<'a> {
    /// Byte-wise view of the original ASCII string
    raw_bytes: &'a [u8],

    /// Byte index of the _next_ character
    next_char_index: usize,
}
//
impl<'a> FastCharIndices<'a> {
    /// Initialize the iterator
    #[inline]
    fn new(input: &'a str) -> Self {
        Self {
            raw_bytes: input.as_bytes(),
            next_char_index: 0,
        }
    }

    /// Non-destructively tell whether we reached the end of the iterator.
    /// TODO: Once ExactSizeIterator::is_empty is stable, implement that trait.
    #[inline]
    fn is_empty(&self) -> bool {
        self.next_char_index >= self.raw_bytes.len()
    }

    /// Tell what was the index of the last character from next()
    #[inline]
    fn prev_index(&self) -> usize {
        self.next_char_index - 1
    }

    /// Go back to the previous character, reversing the action of next()
    #[inline]
    fn back(&mut self) {
        self.next_char_index -= 1;
    }
}
///
/// Iterate over characters (use index() to know about the character's index)
impl<'a> Iterator for FastCharIndices<'a> {
    type Item = char;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Get the current character, if any
        let result = self.raw_bytes.get(self.next_char_index)
                                   .map(|b| { debug_assert!(b.is_ascii());
                                              char::from(*b) });

        // Increment the character counter
        self.next_char_index += 1;

        // Return the freshly read character
        result
    }
}
///
///
/// Testing code often needs to split a single line of text, even though the
/// Real Thing needs to operate over multiple lines of text. We got you covered.
///
#[allow(dead_code)]
fn split_line(input: &str) -> SplitLinesBySpace {
    let mut line_splitter = SplitLinesBySpace::new(input);
    assert!(line_splitter.next_line());
    line_splitter
}


/// Unit tests
#[cfg(test)]
mod tests {
    use super::SplitLinesBySpace;
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
        let mut split_everything = SplitLinesBySpace::new("  s( e \n o,p");
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next_line(), false);
        split_everything = SplitLinesBySpace::new("  s( e \n o,p");
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next(), Some("s("));
        assert_eq!(split_everything.next(), Some("e"));
        assert_eq!(split_everything.next(), None);
        assert_eq!(split_everything.next_line(), true);
        assert_eq!(split_everything.next(), Some("o,p"));
        assert_eq!(split_everything.next(), None);
        assert_eq!(split_everything.next_line(), false);
    }
}
