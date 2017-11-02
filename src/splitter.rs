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
//!   single pass through the text, at the cost of a bit more code complexity.
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
/// - To iterate over lines, call next(). If there is a line of text left in the
///   input, this will produce an iterator over the space-separated columns of
///   that line, otherwise this function will return None.
///
/// This interface was designed to mimick regular Rust iterators, except for the
/// fact that the "parent" line iterator and its "children" column iterators
/// actually share a common character iterator under the hood.
///
/// Working in this fashion avoids internally parsing each line of input twice,
/// once for extracting the line and another time for separating its columns.
/// This makes a nice difference in performance in our memory-bound parsing
/// scenarios. However, it also introduces additional restrictions with respect
/// to standard Rust iterators. For example, a column iterator cannot be live at
/// the time where SplitLinesBySpace::next() is called, as it would be
/// invalidated. Hence SplitLinesBySpace cannot implement std::iter::Iterator.
///
#[derive(Debug, PartialEq)]
pub(crate) struct SplitLinesBySpace<'a> {
    /// Reference to the string which we are trying to split
    target: &'a str,

    /// Iterator over the characters, with on-demand access to byte indices
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

    /// Iterate over lines (see caveats in struct description)
    /// TODO: Consider implementing some variation of StreamingIterator
    pub fn next<'b>(&'b mut self) -> Option<SplitColumns<'a, 'b>>
        where 'a: 'b
    {
        match self.status {
            // We are at the beginning of a line of text. Tell the client that
            // it can parse it, and be ready to skip it on the next call.
            LineSpaceSplitterStatus::AtLineStart => {
                self.status = LineSpaceSplitterStatus::InsideLine;
                return Some(SplitColumns{ parent: self });
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
                            return None;
                        } else {
                            return Some(SplitColumns{ parent: self });
                        }
                    }

                    // Some other character was encountered. Continue iteration.
                    Some(_) => continue,

                    // We reached the end of the input, and will stop there.
                    None => {
                        self.status = LineSpaceSplitterStatus::AtInputEnd;
                        return None;
                    },
                }
            },

            // There is no next line, we are at the end of the input string
            LineSpaceSplitterStatus::AtInputEnd => return None,
        }
    }

    // INTERNAL: Iterate over the space-separated columns of the current line.
    //           This is essentially the implementation of SplitColumns::next().
    fn next_col(&mut self) -> Option<&'a str> {
        // Assuming proper usage of the underlying line iterator...
        assert_eq!(self.status, LineSpaceSplitterStatus::InsideLine);

        // Consume input chars until we reach something that's not a space
        let first_non_space = loop {
            match self.char_iter.next() {
                Some(' ') => continue,
                other => break other,
            }
        };

        // Determine if we reached a new column of data, or the end of the line
        let first_idx = match first_non_space {
            // We reached the end of the line. Prepare the line iterator for the
            // next line, and notify the column iterator client that we're done
            // with this line by returning a None.
            Some('\n') => {
                self.status = if self.char_iter.is_empty() {
                                  LineSpaceSplitterStatus::AtInputEnd
                              } else {
                                  LineSpaceSplitterStatus::AtLineStart
                              };
                return None;
            },

            // We reached the beginning of a data column. Yield its index and
            // let the remainder of this function extract and return the data.
            Some(_) => self.char_iter.prev_index(),

            // We reached the end of the input text. Terminate all iteration.
            None => {
                self.status = LineSpaceSplitterStatus::AtInputEnd;
                return None;
            },
        };

        // If control reaches this point, we're inside of a data column. Iterate
        // through input chars until we reach a column terminator (whitespace
        // character or end of the input text), to locate the end of the column.
        let last_idx = loop {
            match self.char_iter.next() {
                // We can discard spaces and end-of-input terminators: we don't
                // care about spaces, and the character iterator is fused.
                Some(' ') | None => break self.char_iter.prev_index(),

                // We must backtrack on end-of-line terminators, as they can
                // only be handled by the next column iterator invocation.
                Some('\n') => {
                    let last_idx = self.char_iter.prev_index();
                    self.char_iter.back();
                    break last_idx;
                },

                // We are still fetching data characters from the column.
                Some(_) => continue,
            }
        };

        // The data column has been successfully extracted, we can return it.
        Some(&self.target[first_idx..last_idx])
    }
}
///
/// State machine used by SplitLinesBySpace when iterating over lines
#[derive(Debug, PartialEq)]
enum LineSpaceSplitterStatus { AtLineStart, InsideLine, AtInputEnd }
///
///
/// For each line of the input text, SplitLinesBySpace produces an iterator over
/// the space-separated columns of that line. This inner iterator advances the
/// internal character iterator of the "outer" SplitLinesBySpace, so as long as
/// it is alive, SplitLinesBySpace cannot be iterated over further.
///
/// The reason why we moved towards this rather complex streaming design, rather
/// than directly allowing SplitLinesBySpace to iterate over columns as it did
/// before, is that it allows the column iterator to be consumed ("moved away"),
/// which unlocks the full power of the standard Rust iteration interface.
///
#[derive(Debug, PartialEq)]
pub(crate) struct SplitColumns<'a, 'b> where 'a: 'b {
    /// Underlying SplitLinesBySpace iterator
    parent: &'b mut SplitLinesBySpace<'a>,
}
//
impl<'a, 'b> Iterator for SplitColumns<'a, 'b> {
    /// We're outputting strings
    type Item = &'a str;

    /// This is how one iterates through space-separated columns until a newline
    fn next(&mut self) -> Option<Self::Item> {
        self.parent.next_col()
    }
}
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
#[derive(Debug, PartialEq)]
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


/// Testing code often needs to split a single line of text, even though The
/// Real Thing operates on more complex input. This test harness handles this.
#[cfg(test)]
pub(crate) fn split_line_and_run<'a, F, R>(input: &'a str, test_runner: F) -> R
    where F: for<'b> FnOnce(SplitColumns<'a, 'b>) -> R
{
    let mut lines = SplitLinesBySpace::new(input);
    let result = test_runner(lines.next().expect("Input should not be empty"));
    assert_eq!(lines.next(), None, "Input should be only one line long");
    result
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

    // Test that split_line_and_run behaves as expected:
    #[test]
    fn split_line_and_run() {
        let answer = super::split_line_and_run("The answer is 42", |mut columns| {
            assert_eq!(columns.next(), Some("The"));
            assert_eq!(columns.next(), Some("answer"));
            assert_eq!(columns.next(), Some("is"));
            assert_eq!(columns.next(), Some("42"));
            assert_eq!(columns.next(), None);
            42
        });
        assert_eq!(answer, 42);
    }

    /// INTERNAL: Given a string and its decomposition into lines and space-
    ///           separated columns, check if SplitLinesBySpace works on it.
    fn test_splitter(string: &str, decomposition: &[&[&str]]) {
        // Start by skipping through the lines
        let mut lines = SplitLinesBySpace::new(string);
        for _ in decomposition.iter() {
            assert!(lines.next().is_some());
        }
        assert_eq!(lines.next(), None);

        // Check that reading one column and skipping through the rest works
        lines = SplitLinesBySpace::new(string);
        for line in decomposition.iter() {
            let mut columns = lines.next().expect("Unexpected end of file");
            if line.len() >= 1 {
                assert_eq!(columns.next(), Some(line[0]));
            }
        }
        assert_eq!(lines.next(), None);

        // And finish with full column iteration
        lines = SplitLinesBySpace::new(string);
        for line in decomposition.iter() {
            let mut columns = lines.next().expect("Unexpected end of file");
            for column in line.iter() {
                assert_eq!(columns.next(), Some(*column));
            }
            assert_eq!(columns.next(), None);
        }
        assert_eq!(lines.next(), None);
    }
}
