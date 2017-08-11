//! A sampling-oriented reader for Linux pseudo-files
//!
//! Linux pseudo-files, such as those from /proc, have a number of special
//! characteristics that are best accounted for through a special abstraction
//! when performing sampling measurements:
//!
//! - They are small (a few kB at most), so it is best to read them in one go.
//! - They do not live on hardware devices, but are generated on the host CPU.
//!   So there is no performance benefit in reading them asynchronously.
//! - They almost exclusively contain ASCII-encoded text. And I have yet to find
//!   a binary-encoded file that is interesting for performance studies.
//! - Their size does not vary much. So a buffer which was large enough for one
//!   read is likely to be suitable for the next read.
//! - One can update their "contents" just by seeking to the beginning.
//! - Their format is part of the kernel ABI, and is thus expected to only be
//!   modified through backwards-compatible extensions.
//!
//! The SamplingReader that is provided in this module is designed to properly
//! account for these characteristics while reading these pseudo-files.

use std::fs::File;
use std::io::{Read, Result, Seek, SeekFrom};
use std::path::Path;


/// Sampling-oriented reader for procfs pseudo-files
///
/// It should also work for files from sysfs, but I'll refrain from making a
/// definite statement about this until I have really taken the time to study
/// sysfs and check that the above assumptions still hold.
///
pub(crate) struct ProcFileReader {
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
    pub fn sample<F, R>(&mut self, mut parser: F) -> Result<R>
        where F: FnMut(&str) -> R
    {
        // Read the current contents of the file
        self.file_handle.read_to_string(&mut self.readout_buffer)?;

        // Run the user-provided parser on the file contents
        let result = parser(&self.readout_buffer);

        // Reset the reader state to prepare for the next sample
        self.readout_buffer.clear();
        self.file_handle.seek(SeekFrom::Start(0u64))?;

        // Return the parser's results
        Ok(result)
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use super::ProcFileReader;

    /// Check that opening /proc/uptime works as expected
    #[test]
    fn open_file() {
        let _ = ProcFileReader::open("/proc/uptime")
                               .expect("Should be able to open /proc/uptime");
    }

    /// Check that two uptime measurements separated by some sleep differ
    #[test]
    fn uptime_sampling() {
        // Open the uptime file
        let mut reader =
            ProcFileReader::open("/proc/uptime")
                           .expect("Should be able to open /proc/uptime");

        // Read its contents once
        let mut meas1 = String::new();
        reader.sample(|text| meas1.push_str(text))
              .expect("Should be able to read uptime once");

        // Wait a bit
        thread::sleep(Duration::from_millis(50));

        // Read its contents again
        let mut meas2 = String::new();
        reader.sample(|text| meas2.push_str(text))
              .expect("Should be able to read uptime twice");

        // The contents should have changed
        assert!(meas1 != meas2, "Uptime should change over time");
    }
}
