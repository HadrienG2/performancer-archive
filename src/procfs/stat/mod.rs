//! This module contains a sampling parser for /proc/stat

mod cpu;
mod interrupt;
mod paging;

use ::sampler::PseudoFileParser;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use chrono::{DateTime, TimeZone, Utc};
use std::fmt::Debug;
use std::str::FromStr;


// Implement a sampler for /proc/stat using StatData for parsing & storage
define_sampler!{ Sampler : "/proc/stat" => SampledData }


/// Streaming parser for /proc/stat
///
/// TODO: Decide whether a more extensive description is needed
///
pub struct Parser {}
//
impl Parser {
    /// Build a parser, using initial file contents for schema analysis
    pub fn new(_initial_contents: &str) -> Self {
        Self {}
    }

    /// Parse a pseudo-file sample into a stream of records
    pub fn parse<'a>(&mut self, file_contents: &'a str) -> RecordStream<'a> {
        RecordStream::new(file_contents)
    }
}
///
///
/// Stream of records from /proc/stat
///
/// This streaming iterator should yield a stream of records, each representing
/// a line of /proc/stat (i.e. a named homogeneous dataset, like CPU activity
/// counters or interrupt counters).
///
pub struct RecordStream<'a> {
    /// Iterator into the lines and columns of /proc/stat
    file_lines: SplitLinesBySpace<'a>,
}
//
impl<'a> RecordStream<'a> {
    /// Extract the next record from /proc/stat
    pub fn next<'b>(&'b mut self) -> Option<Record<'a, 'b>>
        where 'a: 'b
    {
        self.file_lines.next().map(Record::new)
    }

    /// Create a record stream from raw contents
    fn new(file_contents: &'a str) -> Self {
        Self {
            file_lines: SplitLinesBySpace::new(file_contents),
        }
    }
}
///
///
/// Parseable record from /proc/stat
///
/// This represents Ima line of /proc/stat, which may contain various kinds of
/// data. Use the kind() method of this type to identify what kind of supported
/// data is stored in this record, if any.
///
/// After the first sample, you may switch to calling the appropriate
/// parse_xyz() method directly *if* you do not intend to support kernel version
/// changes or CPU hotplug. Otherwise, you will want to use has_kind() in order
/// to check for schema changes, which can occur on these events.
///
pub struct Record<'a, 'b> where 'a: 'b {
    /// Header of the record, used to identify what kind of record it is
    header: &'a str,

    /// Data columns of the record, to be handed to a record-specific parser
    data_columns: SplitColumns<'a, 'b>,
}
//
impl<'a, 'b> Record<'a, 'b> {
    /// Tell how the active record should be parsed (if at all)
    fn kind(&self) -> RecordKind {
        match self.header {
            /// The header of global or per-core CPU stats starts with "cpu"
            cpu_header if &cpu_header[0..3] == "cpu" => {
                if cpu_header.len() == 3 {
                    // If it's just "cpu", we're dealing with global CPU stats
                    RecordKind::CPUTotal
                } else {
                    // If it's followed by a numerical identifier, we're
                    // dealing with per-core CPU stats
                    if let Ok(cpu_id) = cpu_header[3..].parse::<u16>() {
                        RecordKind::CPUCore(cpu_id)
                    } else {
                        RecordKind::Unsupported(cpu_header.to_owned())
                    }
                }
            },

            // TODO: Add remaining record types

            /// This header is not supported
            other_header => RecordKind::Unsupported(other_header.to_owned())
        }
    }

    /// Check if the active record is of a certain kind
    ///
    /// This operation is faster than calling kind() and comparing the result.
    /// It is intended to be used in order to check whether the /proc/stat
    /// schema has changed, e.g. due to a kernel update or CPU hotplug event.
    ///
    fn has_kind(&self, kind: &RecordKind) -> bool {
        match *kind {
            // Check for global CPU stats
            RecordKind::CPUTotal => (self.header == "cpu"),

            // Check for per-core CPU stats
            RecordKind::CPUCore(cpu_id) => {
                (&self.header[0..3] == "cpu") &&
                (self.header[4..].parse() == Ok(cpu_id))
            },

            // TODO: Add remaining record types

            // Check for unsupported headers
            RecordKind::Unsupported(ref header) => (self.header == header)
        }
    }

    // TODO: Parsers for each kind() of record

    /// Construct a new record from associated file columns
    fn new(mut file_columns: SplitColumns<'a, 'b>) -> Self {
        Self {
            header: file_columns.next().expect("Missing record header"),
            data_columns: file_columns,
        }
    }
}
///
/// Records from /proc/stat can feature different kinds of statistical data
pub enum RecordKind {
    /// Total CPU usage
    CPUTotal,

    /// Single (virtual) CPU core usage, with the core's numerical identifier
    CPUCore(u16),

    // TODO: Add remaining record types

    /// Some record type unsupported by this parser :-(
    ///
    /// Comes with the associated header, so we can check that at least it
    /// did not change from one parsing pass to the next.
    ///
    Unsupported(String),
}


/// Data samples from /proc/stat, in structure-of-array layout
///
/// Courtesy of Linux's total lack of promises regarding the variability of
/// /proc/stat across hardware architectures, or even on a given system
/// depending on kernel configuration, most entries of this struct are
/// considered optional at this point...
///
#[derive(Debug, PartialEq)]
struct SampledData {
    /// Total CPU usage stats, aggregated across all hardware threads
    all_cpus: Option<cpu::SampledData>,

    /// Per-CPU usage statistics, featuring one entry per hardware thread
    ///
    /// An empty Vec here has the same meaning as a None in other entries: the
    /// per-thread breakdown of CPU usage was not provided by the kernel.
    ///
    each_cpu: Vec<cpu::SampledData>,

    /// Number of pages that the system paged in and out from disk, overall...
    paging: Option<paging::SampledData>,

    /// ...and narrowing it down to swapping activity in particular
    swapping: Option<paging::SampledData>,

    /// Statistics on the number of hardware interrupts that were serviced
    interrupts: Option<interrupt::SampledData>,

    // NOTE: Linux 2.4 used to have disk_io statistics in /proc/stat as well,
    //       but since that is incredibly ancient, we propose not to support it.

    /// Number of context switches that the system underwent since boot
    context_switches: Option<Vec<u64>>,

    /// Boot time (only collected once)
    boot_time: Option<DateTime<Utc>>,

    /// Number of process forks that occurred since boot
    process_forks: Option<Vec<u32>>,

    /// Number of processes in a runnable state (since Linux 2.5.45)
    runnable_processes: Option<Vec<u16>>,

    /// Number of processes blocked waiting for I/O (since Linux 2.5.45)
    blocked_processes: Option<Vec<u16>>,

    /// Statistics on the number of softirqs that were serviced. These use the
    /// same layout as hardware interrupt stats, where softirqs are enumerated
    /// in the same order as in /proc/softirq.
    softirqs: Option<interrupt::SampledData>,

    /// INTERNAL: This vector indicates how each line of /proc/stat maps to the
    /// members of this struct. It basically is a legal and move-friendly
    /// variant of the obvious Vec<&mut StatDataParser> approach.
    ///
    /// The idea of mapping lines of /proc/stat to struct members builds on the
    /// assumption, which we make in other places in this library, that the
    /// kernel configuration (and thus the layout of /proc/stat) will not change
    /// over the course of a series of sampling measurements.
    ///
    line_target: Vec<StatDataMember>,
}
//
impl PseudoFileParser for SampledData {
    /// Create a new statistical data store, using a first sample to know the
    /// structure of /proc/stat on this system
    fn new(initial_contents: &str) -> Self {
        // Our statistical data store will eventually go there
        let mut data = Self {
            all_cpus: None,
            each_cpu: Vec::new(),
            paging: None,
            swapping: None,
            interrupts: None,
            context_switches: None,
            boot_time: None,
            process_forks: None,
            runnable_processes: None,
            blocked_processes: None,
            softirqs: None,
            line_target: Vec::new(),
        };

        // The amount of CPU timers will go there once it's known
        let mut num_cpu_timers = 0u8;

        // For each line of the initial contents of /proc/stat...
        let mut lines = SplitLinesBySpace::new(initial_contents);
        while let Some(mut columns) = lines.next() {
            // ...and check the header
            match columns.next().expect("Unexpected empty line") {
                // Statistics on all CPUs (should come first)
                "cpu" => {
                    num_cpu_timers = columns.count() as u8;
                    data.all_cpus = Some(
                        cpu::SampledData::new(num_cpu_timers)
                    );
                    data.line_target.push(StatDataMember::AllCPUs);
                }

                // Statistics on a specific CPU thread (should be consistent
                // with the global stats and come after them)
                header if &header[0..3] == "cpu" => {
                    assert_eq!(columns.count() as u8, num_cpu_timers,
                               "Inconsistent amount of CPU timers");
                    data.each_cpu.push(
                        cpu::SampledData::new(num_cpu_timers)
                    );
                    data.line_target.push(StatDataMember::EachCPU);
                },

                // Paging statistics
                "page" => {
                    data.paging = Some(paging::SampledData::new());
                    data.line_target.push(StatDataMember::Paging);
                },

                // Swapping statistics
                "swap" => {
                    data.swapping = Some(paging::SampledData::new());
                    data.line_target.push(StatDataMember::Swapping);
                },

                // Hardware interrupt statistics
                "intr" => {
                    let num_interrupts = (columns.count() - 1) as u16;
                    data.interrupts = Some(
                        interrupt::SampledData::new(num_interrupts)
                    );
                    data.line_target.push(StatDataMember::Interrupts);
                },

                // Context switch statistics
                "ctxt" => {
                    data.context_switches = Some(Vec::new());
                    data.line_target.push(StatDataMember::ContextSwitches);
                },

                // Boot time
                "btime" => {
                    let btime_str = columns.next()
                                           .expect("Missing boot time data");
                    debug_assert_eq!(columns.next(), None,
                                     "Unexpected extra boot time data");
                    data.boot_time = Some(
                        Utc.timestamp(
                            btime_str.parse()
                                     .expect("Boot time should be an integer"),
                            0
                        )
                    );
                    data.line_target.push(StatDataMember::BootTime);
                },

                // Number of process forks since boot
                "processes" => {
                    data.process_forks = Some(Vec::new());
                    data.line_target.push(StatDataMember::ProcessForks);
                },

                // Number of processes in the runnable state
                "procs_running" => {
                    data.runnable_processes = Some(Vec::new());
                    data.line_target.push(StatDataMember::RunnableProcesses);
                },

                // Number of processes waiting for I/O
                "procs_blocked" => {
                    data.blocked_processes = Some(Vec::new());
                    data.line_target.push(StatDataMember::BlockedProcesses);
                },

                // Softirq statistics
                "softirq" => {
                    let num_interrupts = (columns.count() - 1) as u16;
                    data.softirqs = Some(
                        interrupt::SampledData::new(num_interrupts)
                    );
                    data.line_target.push(StatDataMember::SoftIRQs);
                },

                // Something we do not support yet? We should!
                unknown_header => {
                    debug_assert!(false,
                                  "Unsupported entry '{}' detected!",
                                  unknown_header);
                    data.line_target.push(StatDataMember::Unsupported);
                }
            }
        }

        // Return our data collection setup
        data
    }

    /// Parse the contents of /proc/stat and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, file_contents: &str) {
        // This is the hardware CPU thread which we are currently considering
        let mut cpu_iter = self.each_cpu.iter_mut();

        // This time, we know how lines of /proc/stat map to our members
        let mut lines = SplitLinesBySpace::new(file_contents);
        for target in self.line_target.iter() {
            // The beginning of parsing is the same as before: split by spaces
            // and extract the header of each line.
            let mut columns = lines.next()
                                   .expect("A stat record has disappeared");
            let header = columns.next().expect("Unexpected empty line");

            // Forward the /proc/stat data to the appropriate parser, detecting
            // any structural change in the file (caused by, for example, kernel
            // updates or CPU hotplug) which we do not support at the moment.
            const STRUCTURE_ERR: &'static str = "Unsupported structure change";
            match *target {
                StatDataMember::AllCPUs => {
                    assert_eq!(header, "cpu", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.all_cpus, columns);
                },
                StatDataMember::EachCPU => {
                    assert_eq!(&header[0..3], "cpu", "{}", STRUCTURE_ERR);
                    cpu_iter.next()
                            .expect("Per-cpu stats do not match each_cpu.len()")
                            .push(columns);
                },
                StatDataMember::Paging => {
                    assert_eq!(header, "page", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.paging, columns);
                },
                StatDataMember::Swapping => {
                    assert_eq!(header, "swap", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.swapping, columns);
                },
                StatDataMember::Interrupts => {
                    assert_eq!(header, "intr", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.interrupts, columns);
                },
                StatDataMember::ContextSwitches => {
                    assert_eq!(header, "ctxt", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.context_switches, columns);
                },
                StatDataMember::BootTime => {
                    assert_eq!(header, "btime", "{}", STRUCTURE_ERR);
                    // Nothing to do, we only measure boot time once
                },
                StatDataMember::ProcessForks => {
                    assert_eq!(header, "processes", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.process_forks, columns);
                },
                StatDataMember::RunnableProcesses => {
                    assert_eq!(header, "procs_running", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.runnable_processes, columns);
                },
                StatDataMember::BlockedProcesses => {
                    assert_eq!(header, "procs_blocked", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.blocked_processes, columns);
                },
                StatDataMember::SoftIRQs => {
                    assert_eq!(header, "softirq", "{}", STRUCTURE_ERR);
                    Self::force_push(&mut self.softirqs, columns);
                }
                StatDataMember::Unsupported => {},
            }
        }

        // At the end of parsing, we should have consumed all statistics from
        // the file, otherwise the /proc/stat schema got updated behind our back
        debug_assert_eq!(lines.next(), None,
                         "A stat record appeared out of nowhere");

        // At the end of parsing, all CPU threads should have been considered
        debug_assert!(cpu_iter.next().is_none(),
                      "Per-cpu stats do not match each_cpu.len()");
    }

    /// Tell how many samples are present in the data store, and in debug mode
    /// check for internal data store consistency
    #[cfg(test)]
    fn len(&self) -> usize {
        let mut opt_len = None;
        Self::update_len(&mut opt_len, &self.all_cpus);
        debug_assert!(
            self.each_cpu
                .iter()
                .all(|cpu| {
                    opt_len.expect("each_cpu should come with all_cpus") ==
                        cpu.len()
                })
        );
        Self::update_len(&mut opt_len, &self.paging);
        Self::update_len(&mut opt_len, &self.swapping);
        Self::update_len(&mut opt_len, &self.interrupts);
        Self::update_len(&mut opt_len, &self.context_switches);
        Self::update_len(&mut opt_len, &self.process_forks);
        Self::update_len(&mut opt_len, &self.runnable_processes);
        Self::update_len(&mut opt_len, &self.blocked_processes);
        Self::update_len(&mut opt_len, &self.softirqs);
        opt_len.unwrap_or(0)
    }
}
//
impl SampledData {
    /// INTERNAL: Helpful wrapper for pushing into optional containers that we
    ///           actually know from additional metadata to be around
    fn force_push<T>(store: &mut Option<T>, columns: SplitColumns)
        where T: StatDataStore
    {
        store.as_mut()
             .expect("Attempted to push into a nonexistent container")
             .push(columns);
    }

    /// INTERNAL: Update our prior knowledge of the amount of stored samples
    ///           (current_len) according to an optional data source.
    #[cfg(test)]
    fn update_len<T>(current_len: &mut Option<usize>, opt_store: &Option<T>)
        where T: StatDataStore
    {
        // This closure will get us the amount of samples stored inside of the
        // optional data source as an Option<usize>, if we turn out to need it
        let get_len = || opt_store.as_ref().map(|store| store.len());
        
        // Do we already know the amount of samples that should be in there?
        match *current_len {
            // If so, we only need to check if it is as expected, in debug mode
            Some(old_len) => {
                if let Some(new_len) = get_len() {
                    debug_assert_eq!(new_len, old_len,
                                     "Inconsistent amounts of stored samples");
                }
            },

            // If not, we can safely overwrite our knowledge of the amount of
            // samples with data from our new data source
            None => *current_len = get_len(),
        }
    }
}
///
/// This enum should be kept in sync with the definition of StatData
///
#[derive(Debug, PartialEq)]
enum StatDataMember {
    /// Data storage elements of StatData
    AllCPUs,
    EachCPU,
    Paging,
    Swapping,
    Interrupts,
    ContextSwitches,
    BootTime,
    ProcessForks,
    RunnableProcesses,
    BlockedProcesses,
    SoftIRQs,

    /// Special entry for unsupported fields of /proc/stat
    Unsupported
}


/// Every container of /proc/stat data should implement the following trait,
/// which exposes its ability to be filled from segmented /proc/stat contents.
trait StatDataStore {
    /// Parse and record a sample of data from /proc/stat
    fn push(&mut self, splitter: SplitColumns);

    /// In testing code, working from a raw string is sometimes more convenient
    #[cfg(test)]
    fn push_str(&mut self, input: &str) {
        use splitter::split_line_and_run;
        split_line_and_run(input, |columns| self.push(columns))
    }

    /// Number of data samples that were recorded so far
    #[cfg(test)]
    fn len(&self) -> usize;
}


/// We implement this trait for primitive types that can be parsed from &str
impl<T, U> StatDataStore for Vec<T>
    where T: FromStr<Err=U>,
          U: Debug
{
    fn push(&mut self, mut columns: SplitColumns) {
        self.push(columns.next().expect("Expected statistical data")
                         .parse().expect("Failed to parse statistical data"));
        debug_assert!(columns.next().is_none(),
                      "No other statistical data should be present");
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        <Vec<T>>::len(self)
    }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use super::{PseudoFileParser, SampledData, StatDataMember, StatDataStore};
    use super::{cpu, interrupt, paging};

    /// Check that scalar statistics parsing works as expected
    #[test]
    fn parse_scalar_stat() {
        let mut scalar_stats = Vec::<u64>::new();
        assert_eq!(StatDataStore::len(&scalar_stats), 0);
        StatDataStore::push_str(&mut scalar_stats, "123");
        assert_eq!(scalar_stats, vec![123]);
        assert_eq!(StatDataStore::len(&scalar_stats), 1);
    }

    /// Check that statistical data initialization works as expected
    #[test]
    fn init_stat_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut stats = String::new();
        let empty_stats = SampledData::new(&stats);
        assert!(empty_stats.all_cpus.is_none());
        assert_eq!(empty_stats.each_cpu.len(), 0);
        assert!(empty_stats.paging.is_none());
        assert!(empty_stats.swapping.is_none());
        assert!(empty_stats.interrupts.is_none());
        assert!(empty_stats.context_switches.is_none());
        assert!(empty_stats.boot_time.is_none());
        assert!(empty_stats.process_forks.is_none());
        assert!(empty_stats.runnable_processes.is_none());
        assert!(empty_stats.blocked_processes.is_none());
        assert!(empty_stats.softirqs.is_none());
        let mut expected = empty_stats;

        // ...adding global CPU stats
        stats.push_str("cpu 1 2 3 4");
        let global_cpu_stats = SampledData::new(&stats);
        expected.all_cpus = Some(cpu::SampledData::new(4));
        expected.line_target.push(StatDataMember::AllCPUs);
        assert_eq!(global_cpu_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding dual-core CPU stats
        stats.push_str("\ncpu0 0 1 1 3
                          cpu1 1 1 2 1");
        let local_cpu_stats = SampledData::new(&stats);
        expected.each_cpu = vec![cpu::SampledData::new(4); 2];
        expected.line_target.push(StatDataMember::EachCPU);
        expected.line_target.push(StatDataMember::EachCPU);
        assert_eq!(local_cpu_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding paging stats
        stats.push_str("\npage 42 43");
        let paging_stats = SampledData::new(&stats);
        expected.paging = Some(paging::SampledData::new());
        expected.line_target.push(StatDataMember::Paging);
        assert_eq!(paging_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding swapping stats
        stats.push_str("\nswap 24 34");
        let swapping_stats = SampledData::new(&stats);
        expected.swapping = Some(paging::SampledData::new());
        expected.line_target.push(StatDataMember::Swapping);
        assert_eq!(swapping_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding interrupt stats
        stats.push_str("\nintr 12345 678 910");
        let interrupt_stats = SampledData::new(&stats);
        expected.interrupts = Some(interrupt::SampledData::new(2));
        expected.line_target.push(StatDataMember::Interrupts);
        assert_eq!(interrupt_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding context switches
        stats.push_str("\nctxt 654321");
        let context_stats = SampledData::new(&stats);
        expected.context_switches = Some(Vec::new());
        expected.line_target.push(StatDataMember::ContextSwitches);
        assert_eq!(context_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding boot time
        stats.push_str("\nbtime 5738295");
        let boot_time_stats = SampledData::new(&stats);
        expected.boot_time = Some(Utc.timestamp(5738295, 0));
        expected.line_target.push(StatDataMember::BootTime);
        assert_eq!(boot_time_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding process fork counter
        stats.push_str("\nprocesses 94536551");
        let process_fork_stats = SampledData::new(&stats);
        expected.process_forks = Some(Vec::new());
        expected.line_target.push(StatDataMember::ProcessForks);
        assert_eq!(process_fork_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding runnable process counter
        stats.push_str("\nprocs_running 1624");
        let runnable_process_stats = SampledData::new(&stats);
        expected.runnable_processes = Some(Vec::new());
        expected.line_target.push(StatDataMember::RunnableProcesses);
        assert_eq!(runnable_process_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding blocked process counter
        stats.push_str("\nprocs_blocked 8948");
        let blocked_process_stats = SampledData::new(&stats);
        expected.blocked_processes = Some(Vec::new());
        expected.line_target.push(StatDataMember::BlockedProcesses);
        assert_eq!(blocked_process_stats, expected);
        assert_eq!(expected.len(), 0);

        // ...adding softirq stats
        stats.push_str("\nsoftirq 94651 1561 21211 12 71867");
        let softirq_stats = SampledData::new(&stats);
        expected.softirqs = Some(interrupt::SampledData::new(4));
        expected.line_target.push(StatDataMember::SoftIRQs);
        assert_eq!(softirq_stats, expected);
        assert_eq!(expected.len(), 0);
    }

    /// Check that statistical data parsing works as expected
    #[test]
    fn parse_stat_data() {
        // Starting with an empty file (should never happen, but good base case)
        let mut stats = String::new();
        let mut empty_stats = SampledData::new(&stats);
        empty_stats.push(&stats);
        let mut expected = SampledData::new(&stats);
        assert_eq!(empty_stats, expected);

        // Adding global CPU stats
        stats.push_str("cpu 1 2 3 4");
        let mut global_cpu_stats = SampledData::new(&stats);
        global_cpu_stats.push(&stats);
        expected = SampledData::new(&stats);
        expected.all_cpus.as_mut()
                         .expect("CPU stats incorrectly marked as missing")
                         .push_str("1 2 3 4");
        assert_eq!(global_cpu_stats, expected);
        assert_eq!(expected.len(), 1);

        // Adding dual-core CPU stats
        stats.push_str("\ncpu0 0 1 1 3
                          cpu1 1 1 2 1");
        let mut local_cpu_stats = SampledData::new(&stats);
        local_cpu_stats.push(&stats);
        expected = SampledData::new(&stats);
        expected.all_cpus.as_mut()
                         .expect("CPU stats incorrectly marked as missing")
                         .push_str("1 2 3 4");
        expected.each_cpu[0].push_str("0 1 1 3");
        expected.each_cpu[1].push_str("1 1 2 1");
        assert_eq!(local_cpu_stats, expected);
        assert_eq!(expected.len(), 1);

        // Starting over from paging stats
        stats = String::from("page 42 43");
        let mut paging_stats = SampledData::new(&stats);
        paging_stats.push(&stats);
        expected = SampledData::new(&stats);
        expected.paging.as_mut()
                       .expect("Paging stats incorrectly marked as missing")
                       .push_str("42 43");
        assert_eq!(paging_stats, expected);
        assert_eq!(expected.len(), 1);

        // Starting over from softirq stats
        stats = String::from("softirq 94651 1561 21211 12 71867");
        let mut softirq_stats = SampledData::new(&stats);
        softirq_stats.push(&stats);
        expected = SampledData::new(&stats);
        expected.softirqs.as_mut()
                         .expect("Softirq stats incorrectly marked as missing")
                         .push_str("94651 1561 21211 12 71867");
        assert_eq!(softirq_stats, expected);
        assert_eq!(expected.len(), 1);
    }

    /// Check that the sampler works well
    define_sampler_tests!{ super::Sampler }
}


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    define_sampler_benchs!{ super::Sampler,
                            "/proc/stat",
                            100_000 }
}
