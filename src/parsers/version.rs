//! This module contains a parser for /proc/version
//!
//! Since the kernel version is not expected to change during a normal
//! performance measurement, this module is not designed for sampling, unlike
//! others, but only for a one-time readout that subsequently gets re-used.

use regex::Regex;
use std::fs::File;
use std::io::{Read, Result};


lazy_static! {
    /// We should only need to parse the host's kernel version once
    pub static ref LINUX_VERSION: LinuxVersion = LinuxVersion::load().unwrap();
}


/// Mechanism to collect kernel versioning information
#[derive(Debug, Eq, PartialEq)]
pub struct LinuxVersion {
    /// Upstream kernel version, following Linux 3.x style
    ///
    /// Be warned that in the pre-3.0 era, these nubers actually had different
    /// semantics: the third "bugfix" number was actually used for feature
    /// releases, and a fourth version number was used for bugfixes.
    ///
    /// Because Linux 2.6 has long been unmaintained and is only used by
    /// obsolete "entreprise" Linux distributions, we believe that not
    /// fully supporting its versioning scheme is an acceptable compromise.
    ///
    pub major: u8,
    pub minor: u8,
    pub bugfix: u8,

    /// Distribution-specific versioning information and kernel flavours.
    /// Parsing this further would require an extensive study of ditributions'
    /// kernel versioning schemes, which I am not ready to carry out right now.
    /// So as a stopgap solution, this is not yet part of the public interface.
    distro_flavour: Option<String>,

    /// Build information (host, compiler, date...) is not parsed either, since
    /// we have no use for it at the momment.
    build_info: String,
}
//
impl LinuxVersion {
    // Load kernel versioning information from /proc/version
    pub fn load() -> Result<Self> {
        // Read the raw kernel versioning information
        let mut file = File::open("/proc/version")?;
        let mut raw_version = String::new();
        file.read_to_string(&mut raw_version)?;
        let trimmed_version = raw_version.trim_right();

        // Parse it and return the result
        Ok(Self::parse(trimmed_version))
    }

    // Check if we are using at least a certain kernel version (included)
    pub fn greater_eq(&self, major: u8, minor: u8, bugfix: u8) -> bool {
        // Test major version
        if self.major < major { return false; }
        if self.major > major { return true; }

        // Major version is equal, test minor version
        if self.minor < minor { return false; }
        if self.minor > minor { return true; }

        // Minor version is equal, test bugfix version
        self.bugfix >= bugfix
    }

    // Check if we are below a certain kernel version (excluded)
    pub fn smaller(&self, major: u8, minor: u8, bugfix: u8) -> bool {
        return !self.greater_eq(major, minor, bugfix);
    }

    // INTERNAL: Parse the (trimmed) contents of /proc/version
    fn parse(trimmed_version: &str) -> Self {
        // This library only supports Linux's flavour of procfs
        assert_eq!(&trimmed_version[0..5], "Linux");

        // Ultimately, the contents of /proc/version should match this regex
        let version_regex = Regex::new(r"^Linux version (?P<major>[1-9]\d*)\.(?P<minor>\d+)(?:\.(?P<bugfix>\d+))?(?:-(?P<distro_flavour>\S+))? (?P<build_info>.+)$").unwrap();
        let captures = version_regex.captures(trimmed_version).unwrap();

        // Return the parsed kernel version
        Self {
            major: captures["major"].parse().unwrap(),
            minor: captures["minor"].parse().unwrap(),
            bugfix: captures.name("bugfix")
                            .map_or(0, |m| m.as_str().parse().unwrap()),
            distro_flavour: captures.name("distro_flavour")
                                    .map(|m| m.as_str().parse().unwrap()),
            build_info: captures["build_info"].to_owned(),
        }
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use super::{LinuxVersion, LINUX_VERSION};

    /// Test the linux kernel version string parser
    #[test]
    fn parse_version() {
        // No bugfix version and no flavour
        assert_eq!(
            LinuxVersion::parse("Linux version 4.2 (gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            LinuxVersion {
                major: 4,
                minor: 2,
                bugfix: 0,
                distro_flavour: None,
                build_info: String::from("(gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            }
        );

        // A bugfix version, but no flavour
        assert_eq!(
            LinuxVersion::parse("Linux version 4.2.7 (gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            LinuxVersion {
                major: 4,
                minor: 2,
                bugfix: 7,
                distro_flavour: None,
                build_info: String::from("(gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            }
        );

        // A flavour, but no bugfix version
        assert_eq!(
            LinuxVersion::parse("Linux version 4.2-yeah (gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            LinuxVersion {
                major: 4,
                minor: 2,
                bugfix: 0,
                distro_flavour: Some(String::from("yeah")),
                build_info: String::from("(gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            }
        );

        // Both a flavour and a bugfix version
        assert_eq!(
            LinuxVersion::parse("Linux version 4.2.9-wooo (gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            LinuxVersion {
                major: 4,
                minor: 2,
                bugfix: 9,
                distro_flavour: Some(String::from("wooo")),
                build_info: String::from("(gralouf@yolo) #1 Sat May 14 01:51:54 UTC 2048"),
            }
        );
    }

    /// Check that reading the kernel version string of the host works
    #[test]
    fn load_host_version() {
        assert_eq!(*LINUX_VERSION, LinuxVersion::load().unwrap());
    }

    /// Check that kernel version compatibility checks work
    #[test]
    fn check_version_compatibility() {
        // Let's build an arbitrary kernel version struct
        let version = LinuxVersion {
            major: 4,
            minor: 2,
            bugfix: 5,
            distro_flavour: None,
            build_info: String::new(),
        };

        // Check "greater than or equal" version constraint
        assert!(!version.greater_eq(4, 2, 6));
        assert!(version.greater_eq(4, 2, 5));
        assert!(version.greater_eq(4, 2, 4));
        assert!(!version.greater_eq(4, 3, 5));
        assert!(version.greater_eq(4, 1, 6));
        assert!(!version.greater_eq(5, 2, 5));
        assert!(version.greater_eq(3, 3, 6));

        // Check "smaller than" version constraint
        assert!(version.smaller(4, 2, 6));
        assert!(!version.smaller(4, 2, 5));
        assert!(!version.smaller(4, 2, 4));
        assert!(version.smaller(4, 3, 5));
        assert!(!version.smaller(4, 1, 6));
        assert!(version.smaller(5, 2, 5));
        assert!(!version.smaller(3, 3, 6));
    }
}
