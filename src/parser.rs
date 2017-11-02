//! This module defines what a pseudo-file parser is and how it's implemented
//!
//! The textual data from a pseudo-file is not exploitable right after it has
//! been fetched from the kernel. It must be parsed back into usable numbers
//! first. And this is what a parser does: it takes the text of a pseudo-file
//! as input, and provides parsed data as output. By nature, this operation is
//! very specific to a given pseudo-file format, aside from some basic text
//! processing building blocks, so each file gets its own parser.
//!
//! In order to support high-performance usage patterns, such as skipping unused
//! file records quickly instead of parsing them, or storing the final data in
//! structure-of-array layout, most parsers do not emit parsed output all at
//! once, but use a streaming design in which file records are processed one by
//! one, on the user's request.


/// All pseudo-file parsers are expected to implement the following trait, which
/// covers basic initialization. The parsing mechanism itself has several
/// possible variations, which will be covered by more specialized traits below.
pub(crate) trait PseudoFileParser {
    /// Setup a parser by analyzing a first sample of the file
    fn new(initial_contents: &str) -> Self;
}


/// The simplest parsing mechanism is eager parsing, in which the parser eagerly
/// goes through the entire file, parses everything, put all contents into a
/// struct, and returns that struct. For most files, it has a relatively high
/// overhead, but it can be convenient and fast enough for small files so we
/// include it for the sake of completeness.
pub(crate) trait EagerParser : PseudoFileParser {
    type Output;
    fn parse(&mut self, file_contents: &str) -> Self::Output;
}


/* TODO: Stabilize these parser traits once associated type constructors land
         in Stable Rust. As of writing (2017-10), they are not even implemented.

/// The most common mechanism is incremental parsing, where the parser does not
/// really do much at invocation time, instead returning a struct that will
/// iteratively do the parsing accoding to user requests. This struct holds a
/// reference to the initial file contents, and thus cannot exit its scope.
pub(crate) trait IncrementalParser : PseudoFileParser {
    type Output<'a>;
    fn parse(&mut self, file_contents: &'a str) -> Self::Output<'a>;
}


/// Finally, one optimization which we do not use yet, but are acknowledging for
/// possible future use, is to cache data across parser runs so as to use our
/// knowledge of previous file contents to parse future file contents quicker.
/// This requires keeping a reference to the parser's internal state as well.
pub(crate) trait CachingParser : PseudoFileParser {
    type Output<'a, 'b>;
    fn parse(&'a mut self, file_contents: &'b str) -> Self::Output<'a, 'b>;
}*/
