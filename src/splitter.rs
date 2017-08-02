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
pub(crate) struct SplitLinesBySpace<'a> {
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

        // We are now inside of a word, and looking for its end. There is one
        // special scenario to take care of: if the word completes at the end
        // of the current line, we will need to output two things in a row,
        // first the word, then a None to signal the line ending. We handle that
        // using the backtracking ability of FastCharIndices.
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

                // We reached the end of the input: output the last word. We do
                // not need to backtrack since the character iterator is fused.
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
/// This iterator is fused: it will continue to output None indefinitely after
/// the end. We will later signal this via the FusedIterator marker trait.
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
//
// TODO: Implement FusedIterator once it is stable


/// Testing code often needs to split a single line of text, even though the
/// Real Thing needs to operate over multiple lines of text. We got you covered.
#[cfg(test)]
pub(crate) fn split_line(input: &str) -> SplitLinesBySpace {
    let mut line_splitter = SplitLinesBySpace::new(input);
    assert!(line_splitter.next_line());
    line_splitter
}


/// Unit tests
#[cfg(test)]
mod tests {
    use super::{FastCharIndices, SplitLinesBySpace};

    /// Check that FastCharIndices handles empty strings correctly
    #[test]
    fn empty_char_indices() {
        let mut empty_iter = FastCharIndices::new("");
        assert!(empty_iter.is_empty());
        assert_eq!(empty_iter.next(), None);
    }

    /// Check that FastCharIndices works well on a single-char string
    #[test]
    fn single_char_indices() {
        // Initial state
        let mut single_char_iter = FastCharIndices::new("@");
        assert!(!single_char_iter.is_empty());

        // Iterating through the character
        assert_eq!(single_char_iter.next(), Some('@'));
        assert!(single_char_iter.is_empty());
        assert_eq!(single_char_iter.prev_index(), 0);

        // Going back and starting over
        single_char_iter.back();
        assert!(!single_char_iter.is_empty());
        assert_eq!(single_char_iter.next(), Some('@'));
        assert!(single_char_iter.is_empty());
        assert_eq!(single_char_iter.prev_index(), 0);

        // Checking that we do get a None at the end
        assert_eq!(single_char_iter.next(), None);
    }

    /// Check that FastCharIndices also works well on a two-char string
    #[test]
    fn two_char_indices() {
        // Initial state
        let mut dual_char_iter = FastCharIndices::new("42");
        assert!(!dual_char_iter.is_empty());

        // Iterating through the first character
        assert_eq!(dual_char_iter.next(), Some('4'));
        assert!(!dual_char_iter.is_empty());
        assert_eq!(dual_char_iter.prev_index(), 0);

        // Iterating through the second character
        assert_eq!(dual_char_iter.next(), Some('2'));
        assert!(dual_char_iter.is_empty());
        assert_eq!(dual_char_iter.prev_index(), 1);

        // Going back and starting over
        dual_char_iter.back();
        assert!(!dual_char_iter.is_empty());
        assert_eq!(dual_char_iter.next(), Some('2'));
        assert!(dual_char_iter.is_empty());
        assert_eq!(dual_char_iter.prev_index(), 1);

        // Checking that we do get a None at the end
        assert_eq!(dual_char_iter.next(), None);
    }

    /// Test that SplitLinesBySpace works as expected
    #[test]
    fn split_lines_by_space() {
        // The empty string is alone in being considered as zero lines long
        test_splitter("",       &[]);

        // All recognized character classes, taken in isolation
        test_splitter("\n",     &[&[]]);
        test_splitter(" ",      &[&[]]);
        test_splitter("a",      &[&[&"a"]]);

        // All ordered combinations of two character classes
        test_splitter("\n\n",   &[&[],          &[]]);
        test_splitter("\n ",    &[&[],          &[]]);
        test_splitter("\nb",    &[&[],          &[&"b"]]);
        test_splitter(" \n",    &[&[]]);
        test_splitter("  ",     &[&[]]);
        test_splitter(" c",     &[&[&"c"]]);
        test_splitter("d\n",    &[&[&"d"]]);
        test_splitter("e ",     &[&[&"e"]]);
        test_splitter("fg",     &[&[&"fg"]]);

        // At this stage, we have tested...
        //  - Empty text, non-empty text with empty and non-empty lines
        //  - Words at the beginning of a line, after a space, after a line feed
        //  - Words terminated by a space, a line feed, and the end of input
        //  - Words of one character and of more than one character
        //
        // This dataset thus provides coverage of...
        //  - All the states of the initial loop of next()
        //  - All the states of its final loop
        //  - All states of the inner loop of next_line(), via test_splitter
        //
        // What we do not test so well, however, is whether the iterator's state
        // remains consistent at word boundaries. Hence this last test.
        test_splitter("This. Is\nSPARTA", &[&[&"This.", &"Is"], &[&"SPARTA"]]);
    }

    // Test that split_line behaves as expected:
    #[test]
    fn split_line() {
        let mut splitter = super::split_line("The answer is 42");
        assert_eq!(splitter.next(), Some("The"));
        assert_eq!(splitter.next(), Some("answer"));
        assert_eq!(splitter.next(), Some("is"));
        assert_eq!(splitter.next(), Some("42"));
        assert_eq!(splitter.next(), None);
    }
    // TODO: Test split_line

    /// INTERNAL: Given a string and its decomposition into lines and space-
    ///           separated columns, check if SplitLinesBySpace works on it.
    fn test_splitter(string: &str, decomposition: &[&[&str]]) {
        // Start by skipping through the lines
        let mut splitter = SplitLinesBySpace::new(string);
        for _ in decomposition.iter() {
            assert!(splitter.next_line());
        }
        assert!(!splitter.next_line());

        // Then count the columns of each line
        splitter = SplitLinesBySpace::new(string);
        for line in decomposition.iter() {
            assert!(splitter.next_line());
            assert_eq!(splitter.col_count(), line.len());
        }
        assert!(!splitter.next_line());

        // Check that reading one column and skipping through the rest works
        splitter = SplitLinesBySpace::new(string);
        for line in decomposition.iter() {
            assert!(splitter.next_line());
            if line.len() >= 1 {
                assert_eq!(splitter.next(), Some(line[0]));
            }
        }
        assert!(!splitter.next_line());

        // And finish with full column iteration
        let mut splitter = SplitLinesBySpace::new(string);
        for line in decomposition.iter() {
            assert!(splitter.next_line());
            for column in line.iter() {
                assert_eq!(splitter.next(), Some(*column));
            }
            assert_eq!(splitter.next(), None);
        }
        assert!(!splitter.next_line());
    }
}
