//! This library is a sampling interface to Linux' pseudo-filesystems
//!
//! It currently covers procfs (aka "/proc"), and may in the future also cover
//! sysfs (aka "/sys") for selected purposes.
//!
//! Its main design goal is to allow taking periodical measurements of system
//! activity, as described by the Linux kernel's procfs API, at a relatively
//! high rate (up to 1 kHz) and with low CPU overhead (down to 0.1%), for the
//! purpose of performance analysis.

#[macro_use] extern crate lazy_static;

extern crate bytesize;
extern crate chrono;
extern crate libc;
extern crate regex;
extern crate testbench;

#[macro_use] mod sampler;

mod parser;
pub mod procfs;
mod reader;
mod splitter;


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
