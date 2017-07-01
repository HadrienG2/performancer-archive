//! TODO: This file needs documentation

mod uptime;

use std::fs::File;
use std::io::{Read, Result, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;


// TODO: Organize implementation details better

/// Specialized parser for Durations expressed in fractional seconds, using the
/// usual text format XXXX[.[YY]]. This is about parsing standardized data, so
/// the input is assumed to be correct, and errors will be handled via panics.
fn parse_duration_secs(input: &str) -> Duration {
    // Separate the integral part from the fractional part (if any)
    let mut integer_iter = input.split('.');

    // Parse the number of full seconds
    let seconds = integer_iter.next().unwrap()
                              .parse::<u64>().unwrap();

    // Parse the number of extra nanoseconds, if any
    let nanoseconds = match integer_iter.next() {
        // Handle the "XXXX." syntax used by some text printers
        Some("")       => 0,

        // If there is something after the ., assume it is decimals. Sub nano-
        // second decimals will be truncated: we only count whole nanoseconds.
        Some(mut decimals) => {
            if decimals.len() > 9 { decimals = &decimals[0..9]; }
            let nanosecs_multiplier = 10u32.pow(9 - (decimals.len() as u32));
            decimals.parse::<u32>().unwrap() * nanosecs_multiplier
        }

        // No decimals means no nanoseconds
        None           => 0,
    };

    // At this point, we should be at the end of the string
    debug_assert_eq!(integer_iter.next(), None);

    // Return the Duration that we just parsed
    Duration::new(seconds, nanoseconds)
}


/// Pseudo-files from /proc have a number of characteristics which this custom
/// reader is designed to account for:
///
/// * They are very small (a few kB at most), so they are best read in one go.
/// * They are not actual files, so blocking file readout isn't an issue.
/// * They almost exclusively contain text, and the few binary ones aren't very
///   interesting for the purpose of performance studies.
/// * Their size does not vary much, so reusing readout buffers is worthwhile.
/// * They can be "updated" just by seeking back to the beginning.
/// * Their format is part of the kernel API, and should thust only be modified
///   through backwards-compatible extensions.
///
/// The general design of this reader should probably also work with /sys files,
/// but since I have not yet started looking into these, I reserve my judgment
/// on this matter for now.
///
struct ProcFileReader {
    /// Persistent handle to the file being sampled
    file_handle: File,

    /// Buffer in which the characters that are read out will be stored
    readout_buffer: String,
}
//
impl ProcFileReader {
    /// Attempt to open a proc pseudo-file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file_handle = File::open(path)?;
        Ok(
            Self {
                file_handle,
                readout_buffer: String::new(),
            }
        )
    }

    /// Acquire a new sample of data from the file
    ///
    /// This method takes care of loading the text from the file, and then hands
    /// it to a user-provider parser which shall do whatever it needs to do with
    /// it (including mutating external state).
    ///
    /// No avenue is provided for the user parser to report errors, because it
    /// should not need to. The format of proc-files is part of the kernel API,
    /// so it should only change in backwards compatible ways. And all the user
    /// code is supposed to do here is parse that format and store the results
    /// somewhere. So the only possible errors are logic errors in the parser
    /// and major system issues such as OOM, for which panicking is fine.
    ///
    pub fn sample<F: FnMut(&str)>(&mut self, mut parser: F) -> Result<()> {
        // Read the current contents of the file
        self.file_handle.read_to_string(&mut self.readout_buffer)?;

        // Run the user-provided parser on the file contents
        parser(&self.readout_buffer);

        // Reset the reader state to prepare for the next sample
        self.readout_buffer.clear();
        self.file_handle.seek(SeekFrom::Start(0u64))?;
        Ok(())
    }
}


/// For now, these tests are very much about experimenting
#[cfg(test)]
mod tests {
    use std::time::Duration;
    use uptime::UptimeSampler;

    /// Check that our Duration parser works as expected
    #[test]
    fn parse_duration() {
        assert_eq!(::parse_duration_secs("2"),
                   Duration::new(2, 0));
        assert_eq!(::parse_duration_secs("3."),
                   Duration::new(3, 0));
        assert_eq!(::parse_duration_secs("4.2"),
                   Duration::new(4, 200_000_000));
        assert_eq!(::parse_duration_secs("5.34"),
                   Duration::new(5, 340_000_000));
        assert_eq!(::parse_duration_secs("6.567891234"),
                   Duration::new(6, 567_891_234));
        assert_eq!(::parse_duration_secs("7.8901234567"),
                   Duration::new(7, 890_123_456));
    }

    /// Check that our uptime sampler works and is fast enough
    /// TODO: This should move to the uptime module
    #[test]
    fn it_works() {
        let mut uptime = UptimeSampler::new().unwrap();
        for _ in 0..10_000_000 {
            uptime.sample().unwrap();
        }
    }
}
