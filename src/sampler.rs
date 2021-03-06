//! This module defines what a pseudo-file sampler is and how it's implemented
//!
//! Aside from a small amount of "static" system information, such as the kernel
//! version, most pseudo-files are interfaced through a sampling interface which
//! allows monitoring their time evolution.
//!
//! This sampling interface always works in the same way: read the contents of
//! the file and hand it to a parser which will extract and decode a set of
//! measurements, that will in turn be fed to a container. As a consequence, it
//! is possible to standardize the sampling abstraction, which is what this
//! module does.


/// Define the sampler struct associated with a certain pseudo-file parser
///
/// The purpose of a sampler is to load the contents of a certain pseudo-file
/// and feed it into a certain parser's sample() method every time its sample()
/// method is called. For example, the invocation...
///
/// `define_sampler!(MemInfoSampler : "/proc/meminfo" => MemInfoParser
///                                                   => MemInfoData)`
///
/// ...defines a sampler called "MemInfoSampler" which loads data from the file
/// /proc/meminfo and feeds it to a parser of type "MemInfoParser", and then
/// stores the parsed data into a container called "MemInfoData".
///
/// In today's Rust, this job must be done via macros, because Rust does not yet
/// support generics with value parameters. In future Rust, once this genericity
/// feature has landed, the define_sampler macro will go away in favor of a
/// simpler generic struct instantiation.
///
/// You may want to check out the "parser" module to get an idea of what the
/// parser's interface should be like, and the "data" module to get an idea of
/// what the data container's interface should be like.
///
macro_rules! define_sampler {
    ($sampler: ident : $file_location:expr => $parser:ty => $container:ty) => {
        // Hopefully the host won't need to import these...
        use ::reader::ProcFileReader;
        use std::io;

        /// Mechanism for sampling measurements from $file_location
        pub struct $sampler {
            /// Reader object for $file_location
            reader: ProcFileReader,

            /// Streaming parser for $file_location
            parser: $parser,

            /// Samples of data extracted from $file_location
            samples: $container
        }
        //
        impl $sampler {
            /// Create a new sampler for $file_location
            pub fn new() -> io::Result<Self> {
                // Set up a sampling reader
                let mut reader = ProcFileReader::open($file_location)?;

                // Build parsing and storage infrastructure from a first sample
                let (parser, samples) = reader.sample(|file| {
                    let mut parser = <$parser>::new(file);
                    let samples = <$container>::new(parser.parse(file));
                    (parser, samples)
                })?;

                // Return the full sampling setup
                Ok(
                    Self {
                        reader,
                        parser,
                        samples,
                    }
                )
            }

            /// Acquire a new sample of data from $file_location
            pub fn sample(&mut self) -> io::Result<()> {
                let parser = &mut self.parser;
                let samples = &mut self.samples;
                self.reader.sample(|file| {
                    let stream = parser.parse(file);
                    samples.push(stream);
                })
            }
        }
    };
}


/// Generate the tests associated with a certain sampler
///
/// This macro should be invoked inside of the module associated with the unit
/// tests for a certain pseudo-file.
///
#[cfg(test)]
macro_rules! define_sampler_tests {
    ($sampler:ty) => {
        /// Check that sampler initialization works well
        #[test]
        fn init_sampler() {
            let sampler = <$sampler>::new()
                                     .expect("Failed to create a sampler");
            assert_eq!(sampler.samples.len(), 0);
        }

        /// Check that basic sampling works as expected
        #[test]
        fn basic_sampling() {
           let mut sampler = <$sampler>::new()
                                        .expect("Failed to create a sampler");
           sampler.sample().expect("Failed to acquire a first sample");
           assert_eq!(sampler.samples.len(), 1);
           sampler.sample().expect("Failed to acquire a second sample");
           assert_eq!(sampler.samples.len(), 2);
        }
    };
}


/// Generate the performance benchmarks associated with a certain sampler
///
/// This macro should be invoked inside of the module associated with the
/// benchmarks for a certain pseudo-file.
///
/// The macro parameters are the sampler type, the path to the associated
/// pseudo-file, and the number of benchmark iterations to be carried out.
///
#[cfg(test)]
macro_rules! define_sampler_benchs {
    ($sampler:ty, $file_location:expr, $bench_iters:expr) => {
        use ::reader::ProcFileReader;
        use testbench;

        /// Benchmark for the raw pseudo-file readout overhead
        #[test]
        #[ignore]
        fn readout_overhead() {
            let mut reader =
                ProcFileReader::open($file_location)
                               .expect("Failed to open pseudo-file");
            testbench::benchmark($bench_iters, || {
                reader.sample(|_| {}).expect("Failed to read pseudo-file");
            });
        }

        /// Benchmark for the full pseudo-file sampling overhead
        #[test]
        #[ignore]
        fn sampling_overhead() {
            let mut stat = <$sampler>::new()
                                      .expect("Failed to create a sampler");
            testbench::benchmark($bench_iters, || {
                stat.sample().expect("Failed to sample data");
            });
        }
    }
}
