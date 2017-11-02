//! This module defines how one interacts with sampled pseudo-file data
//!
//! After parsing, we usually want to store the parsed data in some kind of
//! container which accumulates file samples for future use in local GUI
//! displays, batched network transfers to a remote client, or whatnot.
//!
//! The following describes what we expect from such containers.


/// What we expect from all sampled data containers. In an ideal type system,
/// everything should be inside of this trait, but since we can't write code
/// which is generic over the number of lifetimes parameters in the parsed file
/// sample type, we'll also need one subtrait per parsed file sample type.
pub(crate) trait SampledData {
    /// Tell how many data samples are present in this container, and in debug
    /// mode, also check that any redundant metadata is consistent
    fn len(&self) -> usize;
}


/// Sampled data container for data with no lifetime parameter (for example,
/// data which is coming out of an eager parser)
pub(crate) trait SampledData0 : SampledData {
    type Input;

    /// Construct container using a sample of parsed data for schema analysis
    fn new(sample: Self::Input) -> Self;

    /// Push a sample of parsed data into the container
    fn push(&mut self, sample: Self::Input);
}


/* TODO: Stabilize these parser traits once associated type constructors land
         in Stable Rust. As of writing (2017-10), they are not even implemented.

/// Sampled data container for data with one lifetime parameter (for example,
/// data which is coming out of an incremental parser)
pub(crate) trait SampledData1 : SampledData {
    type Input<'a>;

    /// Construct container using a sample of parsed data for schema analysis
    fn new(sample: Self::Input) -> Self;

    /// Push a sample of parsed data into the container
    fn push(&mut self, sample: Self::Input);
}


/// Sampled data container for data with two lifetime parameters (for example,
/// data which is coming out of a caching parser)
pub(crate) trait SampledData2 : SampledData {
    type Input<'a, 'b>;

    /// Construct container using a sample of parsed data for schema analysis
    fn new(sample: Self::Input) -> Self;

    /// Push a sample of parsed data into the container
    fn push(&mut self, sample: Self::Input);
}*/
