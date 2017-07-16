//! This library is a sampling interface to Linux' procfs (/proc)
//!
//! Its main design goal is to allow taking periodical measurements of system
//! activity, as described by the Linux kernel's procfs API, at a relatively
//! high rate (at least 1 kHz), for the purpose of performance analysis.

extern crate chrono;
#[macro_use] extern crate lazy_static;
extern crate libc;
extern crate regex;
extern crate testbench;

pub mod parsers;

use std::fs::File;
use std::io::{Read, Result, Seek, SeekFrom};
use std::path::Path;


/// Sampling-oriented reader for procfs pseudo-files
///
/// Pseudo-files from /proc have a number of characteristics which this custom
/// reader is designed to account for:
///
/// * They are very small (a few kB at most), so they are best read in one go.
/// * They are not actual files, so blocking readout isn't an issue.
/// * They almost exclusively contain text, and the few binary ones aren't very
///   interesting for the purpose of performance studies.
/// * Their size does not vary much, so reusing readout buffers is worthwhile.
/// * They can be "updated" just by seeking back to the beginning.
/// * Their format is part of the kernel API, and should thust only be modified
///   through backwards-compatible extensions.
///
/// The general design of this reader should probably also work with /sys files,
/// but since I have not yet started looking into these, I will refrain from
/// making a strong statement on this matter for now.
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


/// Performance benchmarks
///
/// These benchmarks masquerading as tests are a stopgap solution until
/// benchmarking lands in Stable Rust. They should be compiled in release mode,
/// and run with only one OS thread. In addition, the default behaviour of
/// swallowing test output should obviously be suppressed.
///
/// TL;DR: cargo test --release -- --ignored --nocapture --test-threads=1
///
/// TODO: Switch to standard Rust benchmarks once they are stable
///
#[cfg(test)]
mod benchmarks {
    // No global benchmark yet :-(
}
