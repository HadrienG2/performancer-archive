//! A mechanism for splitting the content of pseudo-files by newlines and spaces
//!
//! The metadata provided by procfs pseudo-files often has a two-dimensional
//! inner structure. Lines of text represent different devices or categories of
//! information, whereas space-separated columns are used to separate the
//! details of a single line (e.g. idle CPU time vs user-mode CPU time).
//!
//! Rust provides ways of dealing with this hierarchy (namely SplitWhitespace
//! and Lines), but these primitives are not fast enough for our demanding
//! application in practice. This deficiency can be attributed to the fact that
//! a naive version of a line- and space- splitter based on standard Rust
//! iterators does many things which we do not need:
//!
//! - It iterates through each line twice, once to determine its boundaries and
//!   another time to separate its columns. This work can be carried out in a
//!   single pass through the text, at the cost of more code complexity.
//! - It treats "characters" in a Unicode-aware fashion, accounting for things
//!   like multiple whitespace characters, whereas we know that the Linux kernel
//!   will only send us ASCII text and only separate it by newlines and spaces.
//!
//! We thus provide a mechanism for separating the lines and space-separated
//! columns of ASCII pseudo-files, achieving much better performance than
//! regular Rust iterators in this scenario.

use std::ascii::AsciiExt;


/// Mechanism for splitting the elements of newlines- and space-separated text
///
/// To use this pseudo-file splitter, proceed as follows:
///
/// - Initialize it on an input string with new()
/// - To iterate over lines, call next_line() as long as it returns true
/// - To iterate over columns, use this as a normal (**non-fused**) iterator
///
/// Note that since the iterator is reused after each line, you cannot consume
/// it using methods like count(). Since counting columns is often useful when
/// initializing a parser, we provide a helper col_count() method which consumes
/// the file columns until the next line and returns their count.
///
struct SplitLinesBySpace<'a> {
    /// Reference to the sring which we are trying to split
    target: &'a str,

    /// Iterator over the characters and their byte indices
    char_iter: FastCharIndices<'a>,

    /// Small state machine tracking our input location (beginning or middle
    /// of a line, end of the input string...)
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

    /// Consume the current line, indicating how many space-separated columns
    /// were encountered, but without consuming the entire iterator.
    pub fn col_count(&mut self) -> usize {
        let mut word_count = 0usize;
        while self.next().is_some() {
            word_count += 1;
        }
        word_count
    }
}
//
// Column iteration is handled using the standard Rust iterator interface.
impl<'a> Iterator for SplitLinesBySpace<'a> {
    /// We're outputting strings
    type Item = &'a str;

    /// This is how one iterates through space-separated columns until a newline
    fn next(&mut self) -> Option<Self::Item> {
        // The caller should have properly called next_line() beforehand
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
                // because we also want to subsequently signal them with a None.
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
/// State machine used by SplitLinesBySpace when iterating over lines
#[derive(Debug, PartialEq)]
enum LineSpaceSplitterStatus { AtLineStart, InsideLine, AtInputEnd }
///
///
/// A conceptual cousin of PutBack<CharIndices>, which we used before, but more
/// tightly optimized for the needs of SplitLinesBySpace:
///
/// - Input is ASCII-only (so, for example, 1 byte = 1 character)
/// - We need characters all the time, but indices only infrequently
/// - We may rarely backtrack on one specific character ('\n')
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

    /// Go back to the previous character, reverting the action of next()
    #[inline]
    fn back(&mut self) {
        self.next_char_index -= 1;
    }
}
///
impl<'a> Iterator for FastCharIndices<'a> {
    /// We implement the iterator interface for character iteration
    type Item = char;

    /// This is how we iterate through ASCII characters
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


/// Testing code often needs to split a single line of text, even though the
/// Real Thing needs to operate over multiple lines of text. We got you covered.
#[cfg(test)]
fn split_line(input: &str) -> SplitLinesBySpace {
    let mut line_splitter = SplitLinesBySpace::new(input);
    assert!(line_splitter.next_line());
    line_splitter
}


/// Unit tests
#[cfg(test)]
mod tests {
    use super::SplitLinesBySpace;

    // TODO: Test FastCharIndices in isolation
    // TODO: Test col_count and split_line
    // TODO: Modularize the splitter testing code

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
