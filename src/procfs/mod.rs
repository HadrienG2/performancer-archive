//! This module contains parsers for the contents of procfs.
//!
//! Most parsers are designed to allow sampling the contents of /proc files at
//! a rapid rate, for the purpose of acquiring, analyzing and displaying useful
//! statistics on the time evolution of system performance.
//!
//! Each submodule corresponds to one file in /proc, and is named as close to
//! that file as allowed by the Rust module system.

pub mod meminfo;
pub mod stat;
pub mod uptime;
pub mod version;
