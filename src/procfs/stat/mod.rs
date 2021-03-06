//! This module contains a sampling parser for /proc/stat

mod cpu;
mod interrupts;
mod paging;

use ::data::{SampledData, SampledData0};
use ::parser::PseudoFileParser;
use ::splitter::{SplitColumns, SplitLinesBySpace};
use chrono::{DateTime, TimeZone, Utc};
use std::str::FromStr;


// Implement a sampler for /proc/meminfo
define_sampler!{ Sampler : "/proc/stat" => Parser => Data }


/// Incremental parser for /proc/stat
pub struct Parser {}
//
impl PseudoFileParser for Parser {
    /// Build a parser, using an initial file sample. Here, this is used to
    /// perform quick schema validation, just to maximize the odds that failure,
    /// if any, will occur at initialization time rather than run time.
    fn new(initial_contents: &str) -> Self {
        let mut stream = RecordStream::new(initial_contents);
        while let Some(record) = stream.next() {
            if let RecordKind::Unsupported(header) = record.kind() {
                debug_assert!(false, "Unsupported record header: {}", header);
            }
        }
        Self {}
    }
}
//
// TODO: Implement IncrementalParser once that trait is usable in stable Rust
impl Parser {
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
/// This represents a line of /proc/stat, which may contain various kinds of
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
            // The header of global stats starts with "cpu"
            cpu_header if (cpu_header.len() >= 3) &&
                          (&cpu_header[0..3] == "cpu") => {
                if cpu_header.len() == 3 {
                    // If it's just "cpu", we're dealing with global CPU stats
                    RecordKind::CPUTotal
                } else {
                    // If it's followed by a numerical identifier, we're
                    // dealing with per-thread CPU stats
                    if let Ok(thread_id) = cpu_header[3..].parse() {
                        RecordKind::CPUThread(thread_id)
                    } else {
                        RecordKind::Unsupported(cpu_header.to_owned())
                    }
                }
            },

            // The header of paging statistics is "page" or "swap"
            "page" => RecordKind::PagingTotal,
            "swap" => RecordKind::PagingSwap,

            // The header of hardware IRQ activity is "intr"
            "intr" => RecordKind::InterruptsHW,

            // The header of the context switch counter is "ctxt"
            "ctxt" => RecordKind::ContextSwitches,

            // The header of the boot time is "btime"
            "btime" => RecordKind::BootTime,

            // The header of total process forking activity is "processes"
            "processes" => RecordKind::ProcessForks,

            // Current process activity has a header starting with "procs_"
            procs_header if (procs_header.len() > 6) &&
                            (&procs_header[0..6] == "procs_") => {
                match &procs_header[6..] {
                    "running" => RecordKind::ProcessesRunnable,
                    "blocked" => RecordKind::ProcessesBlocked,
                    _ => RecordKind::Unsupported(procs_header.to_owned())
                }
            }

            // The header of software IRQ activity is "softirq"
            "softirq" => RecordKind::InterruptsSW,

            // This header is not supported
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

            // Check for per-thread CPU stats
            RecordKind::CPUThread(thread_id) => {
                (self.header.len() > 3) &&
                (&self.header[0..3] == "cpu") &&
                (self.header[3..].parse() == Ok(thread_id))
            },

            // Check for paging statistics
            RecordKind::PagingTotal => (self.header == "page"),
            RecordKind::PagingSwap => (self.header == "swap"),

            // Check for hardware IRQ acticity
            RecordKind::InterruptsHW => (self.header == "intr"),

            // Check for context switch counter
            RecordKind::ContextSwitches => (self.header == "ctxt"),

            // Check for the boot time
            RecordKind::BootTime => (self.header == "btime"),

            // Check for total process forking activity
            RecordKind::ProcessForks => (self.header == "processes"),

            // Check for current process activity
            RecordKind::ProcessesRunnable => (self.header == "procs_running"),
            RecordKind::ProcessesBlocked => (self.header == "procs_blocked"),

            // Check for software IRQ activity
            RecordKind::InterruptsSW => (self.header == "softirq"),

            // Check for unsupported headers
            RecordKind::Unsupported(ref header) => (self.header == header)
        }
    }

    /// Parse the current record as global or per-core CPU stats
    fn parse_cpu(self) -> cpu::RecordFields<'a, 'b> {
        // In debug mode, check that we don't misinterpret things
        debug_assert!(match self.kind() {
            RecordKind::CPUTotal | RecordKind::CPUThread(_) => true,
            _ => false
        });

        // Delegate the parsing to the dedicated "cpu" submodule
        cpu::RecordFields::new(self.data_columns)
    }

    /// Parse the current record as paging or swapping statistics
    fn parse_paging(self) -> paging::RecordFields {
        // In debug mode, check that we don't misinterpret things
        debug_assert!(match self.kind() {
            RecordKind::PagingTotal | RecordKind::PagingSwap => true,
            _ => false
        });

        // Delegate the parsing to the dedicated "paging" submodule
        paging::RecordFields::new(self.data_columns)
    }

    /// Parse the current record as hardware or software interrupt statistics
    fn parse_interrupts(self) -> interrupts::RecordFields<'a, 'b> {
        // In debug mode, check that we don't misinterpret things
        debug_assert!(match self.kind() {
            RecordKind::InterruptsHW | RecordKind::InterruptsSW => true,
            _ => false
        });

        // Delegate the parsing to the dedicated "interrupts" submodule
        interrupts::RecordFields::new(self.data_columns)
    }

    /// Parse the current record as a context switch counter
    fn parse_context_switches(mut self) -> u64 {
        // In debug mode, check that we don't misinterpret things
        debug_assert_eq!(self.kind(), RecordKind::ContextSwitches);

        // Context switches happen rather frequently (up to 10k/second), so
        // anything less than a 64-bit counter would be unwise for this quantity
        let result = self.data_columns
                         .next().expect("Expected context switch counter")
                         .parse().expect("Failed to parse context switches");

        // In debug mode, check that nothing weird appeared in the input
        debug_assert_eq!(self.data_columns.next(), None,
                         "Unexpected additional context switching stat");

        // Return the context switch counter
        result
    }

    /// Parse the current record as a boot time
    fn parse_boot_time(mut self) -> DateTime<Utc> {
        // In debug mode, check that we don't misinterpret things
        debug_assert_eq!(self.kind(), RecordKind::BootTime);

        // Boot times are provided in seconds since the UNIX UTC epoch
        let result = Utc.timestamp(
            self.data_columns.next().expect("Expected boot time")
                             .parse().expect("Boot time should be an integer"),
            0
        );

        // In debug mode, check that nothing weird appeared in the input
        debug_assert_eq!(self.data_columns.next(), None,
                         "Unexpected additional boot time stat");

        // Return the boot time
        result
    }

    /// Parse the current record as a process fork counter
    fn parse_process_forks(mut self) -> u32 {
        // In debug mode, check that we don't misinterpret things
        debug_assert_eq!(self.kind(), RecordKind::ProcessForks);

        // Spawning four billion processes seems somewhat unusual for the uptime
        // of a typical UNIX machine, so I think we can stick with u32 here
        let result = self.data_columns
                         .next().expect("Expected process fork counter")
                         .parse().expect("Failed to parse fork counter");

        // In debug mode, check that nothing weird appeared in the input
        debug_assert_eq!(self.data_columns.next(), None,
                         "Unexpected additional process fork stat");

        // Return the process fork counter
        result
    }

    /// Parse the current record as a counter of live processes
    fn parse_processes(mut self) -> u16 {
        // In debug mode, check that we don't misinterpret things
        debug_assert!(match self.kind() {
            RecordKind::ProcessesRunnable
                | RecordKind::ProcessesBlocked => true,
            _ => false
        });

        // Do you know of someone who typically has more than 65535 processes
        // running or waiting for IO at a given time on a single machine? If so,
        // I'd like to hear about that. Until then, 16 bits seem to be enough.
        let result = self.data_columns
                         .next().expect("Expected live process counter")
                         .parse().expect("Failed to parse process counter");

        // In debug mode, check that nothing weird appeared in the input
        debug_assert_eq!(self.data_columns.next(), None,
                         "Unexpected additional process counter stat");

        // Return the process counter
        result
    }

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
#[derive(Clone, Debug, PartialEq)]
pub enum RecordKind {
    /// Total CPU usage
    CPUTotal,

    /// Single hardware CPU thread usage, with the thread's numerical ID.
    ///
    /// As of 2017, where CPUs with a little more than 256 threads have just
    /// started to appear in HPC centers, a 16-bit ID appears both necessary
    /// and sufficient for storing Linux' CPU thread IDs.
    ///
    CPUThread(u16),

    /// Total paging activity to and from disk
    PagingTotal,

    /// Paging activity that is specifically related to swap usage
    PagingSwap,

    /// Interrupt actvity of hardware IRQs
    InterruptsHW,

    /// Number of context switches since boot
    ContextSwitches,

    /// System boot time
    BootTime,

    /// Number of spawned processes (forks) since boot
    ProcessForks,

    /// Number of processes which are currently in a runnable state
    ProcessesRunnable,

    /// Number of processes which are currently blocked waiting for I/O
    ProcessesBlocked,

    /// Interrupt activity of software IRQs ("softirqs")
    InterruptsSW,

    /// Some record type unsupported by this parser :-(
    ///
    /// Comes with the associated header, so that we can check that at least it
    /// did not change from one parsing pass to the next.
    ///
    Unsupported(String),
}


/// INTERNAL: Helpful wrapper for pushing data into optional containers that we
///           actually know from additional metadata to be around.
///
///           This macro used to be a generic method of Data, but at this
///           point in time we have unfortunately out-smarted the Rust type
///           system. In a nutshell, the problem lies in the fact that
///           "record_fields" may or may not have lifetime parameters.
///
///           Obviously, the container must have a compatible "push" method, in
///           the spirit of the relevant SampledDataN trait.
///
macro_rules! force_push {
    ($store:expr, $record_fields:expr) => {
        $store.as_mut()
              .expect("Attempted to push into a nonexistent container")
              .push($record_fields);
    };
}


/// Data samples from /proc/stat, in structure-of-array layout
///
/// Courtesy of Linux's total lack of promises regarding the variability of
/// /proc/stat across hardware architectures, or even on a given system
/// depending on kernel configuration, most entries of this struct are
/// considered optional at this point...
///
#[derive(Clone, Debug, PartialEq)]
struct Data {
    /// Total CPU usage stats, aggregated across all hardware threads
    all_cpus: Option<cpu::Data>,

    /// Per-CPU usage statistics, featuring one entry per hardware CPU thread
    ///
    /// An empty Vec here has the same meaning as a None in other entries: the
    /// per-thread breakdown of CPU usage was not provided by the kernel.
    ///
    each_thread: Vec<cpu::Data>,

    /// Number of pages that the system paged in and out from disk, overall...
    paging: Option<paging::Data>,

    /// ...and narrowing it down to swapping activity in particular
    swapping: Option<paging::Data>,

    /// Statistics on the number of hardware interrupts that were serviced
    interrupts: Option<interrupts::Data>,

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
    softirqs: Option<interrupts::Data>,

    /// INTERNAL: This vector indicates how each line of /proc/stat maps to the
    /// members of this struct. It basically is a legal and move-friendly
    /// variant of the obvious Vec<&mut StatDataParser> approach.
    ///
    /// The idea of mapping lines of /proc/stat to struct members builds on the
    /// assumption, which we make in other places in this library, that the
    /// kernel configuration (and thus the layout of /proc/stat) will not change
    /// over the course of a series of sampling measurements.
    ///
    line_target: Vec<RecordKind>,
}
//
impl SampledData for Data {
    /// Tell how many samples are present in the data store + check consistency
    fn len(&self) -> usize {
        let mut opt_len = None;
        Self::update_len(&mut opt_len, &self.all_cpus);
        debug_assert!(
            self.each_thread
                .iter()
                .all(|cpu| {
                    opt_len.expect("each_thread should come with all_cpus") ==
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
// TODO: Implement SampledData1 once that is usable in stable Rust
impl Data {
    /// Create a new statistical data store, using a first sample to know the
    /// structure of /proc/stat on this system
    fn new(mut stream: RecordStream) -> Self {
        // Our statistical data store will eventually go there
        let mut data = Self {
            all_cpus: None,
            each_thread: Vec::new(),
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

        // For each initial record of /proc/stat...
        while let Some(record) = stream.next() {
            // ...and check the header
            let record_kind = record.kind();
            match record_kind {
                // Statistics on all CPUs
                RecordKind::CPUTotal => {
                    data.all_cpus = Some(
                        cpu::Data::new(record.parse_cpu())
                    );
                }

                // Statistics on a specific CPU thread (should be enumerated in
                // order, from thread 0 to thread Nt-1)
                RecordKind::CPUThread(thread_id) => {
                    assert_eq!(thread_id, data.each_thread.len() as u16,
                               "Unexpected CPU thread ordering");
                    data.each_thread.push(
                        cpu::Data::new(record.parse_cpu())
                    );
                },

                // Paging statistics
                RecordKind::PagingTotal => {
                    data.paging = Some(
                        paging::Data::new(record.parse_paging())
                    );
                },

                // Swapping statistics
                RecordKind::PagingSwap => {
                    data.swapping = Some(
                        paging::Data::new(record.parse_paging())
                    );
                },

                // Hardware interrupt statistics
                RecordKind::InterruptsHW => {
                    data.interrupts = Some(
                        interrupts::Data::new(record.parse_interrupts())
                    );
                },

                // Context switch statistics
                RecordKind::ContextSwitches => {
                    data.context_switches = Some(Vec::new());
                },

                // Boot time
                RecordKind::BootTime => {
                    data.boot_time = Some(record.parse_boot_time());
                },

                // Number of process forks since boot
                RecordKind::ProcessForks => {
                    data.process_forks = Some(Vec::new());
                },

                // Number of processes in the runnable state
                RecordKind::ProcessesRunnable => {
                    data.runnable_processes = Some(Vec::new());
                },

                // Number of processes waiting for I/O
                RecordKind::ProcessesBlocked => {
                    data.blocked_processes = Some(Vec::new());
                },

                // Softirq statistics
                RecordKind::InterruptsSW => {
                    data.softirqs = Some(
                        interrupts::Data::new(record.parse_interrupts())
                    );
                },

                // Something we do not support yet? We should!
                RecordKind::Unsupported(_) => {}
            }

            // Remember what kind of record that was
            data.line_target.push(record_kind);
        }

        // Return our data collection setup
        data
    }

    /// Parse the contents of /proc/stat and add a data sample to all
    /// corresponding entries in the internal data store
    fn push(&mut self, mut stream: RecordStream) {
        // This will iterate over the hardware CPU thread data
        let mut thread_iter = self.each_thread.iter_mut();

        // This time, we know how lines of /proc/stat map to our members
        for target in self.line_target.iter() {
            // Check that the record structure of the file has not changed. We
            // do not support events which can change the /proc/stat schema
            // (such as kernel updates or CPU hotplug) at this point in time,
            // so all we need to do is to check for schema consistency.
            let record = stream.next().expect("Unsupported schema change");
            assert!(record.has_kind(target), "Unsupported schema change");

            // Now we can sample the new contents of that record
            match *target {
                RecordKind::CPUTotal => {
                    force_push!(self.all_cpus, record.parse_cpu());
                },
                RecordKind::CPUThread(_) => {
                    thread_iter.next()
                               .expect("Found a bug in CPU thread iteration")
                               .push(record.parse_cpu());
                },
                RecordKind::PagingTotal => {
                    force_push!(self.paging, record.parse_paging());
                },
                RecordKind::PagingSwap => {
                    force_push!(self.swapping, record.parse_paging());
                },
                RecordKind::InterruptsHW => {
                    force_push!(self.interrupts, record.parse_interrupts());
                },
                RecordKind::ContextSwitches => {
                    force_push!(self.context_switches,
                                record.parse_context_switches());
                },
                RecordKind::BootTime => {
                    // Nothing to do, we only measure boot time once
                },
                RecordKind::ProcessForks => {
                    force_push!(self.process_forks,
                                record.parse_process_forks());
                },
                RecordKind::ProcessesRunnable => {
                    force_push!(self.runnable_processes,
                                record.parse_processes());
                },
                RecordKind::ProcessesBlocked => {
                    force_push!(self.blocked_processes,
                                record.parse_processes());
                },
                RecordKind::InterruptsSW => {
                    force_push!(self.softirqs, record.parse_interrupts());
                },
                RecordKind::Unsupported(_) => {}
            }
        }

        // At the end of parsing, we should have consumed all statistics from
        // the file, otherwise the /proc/stat schema got updated behind our back
        debug_assert!(stream.next().is_none(), "Unsupported schema change");
        debug_assert!(thread_iter.next().is_none(),
                      "Found a bug in CPU thread iteration");
    }

    /// INTERNAL: Update our prior knowledge of the amount of stored samples
    ///           (current_len) according to an optional data source.
    fn update_len<T>(current_len: &mut Option<usize>, opt_store: &Option<T>)
        where T: SampledData
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


/// Every sub-store of sampled data inside of Data should implement SampledData,
/// including the trusty old Vec (which is a case of SampledDataEager).
impl<T> SampledData for Vec<T>
    where T: FromStr
{
    /// Tell how many data samples are present in this container
    fn len(&self) -> usize {
        <Vec<T>>::len(self)
    }
}
//
impl<T> SampledData0 for Vec<T>
    where T: FromStr
{
    type Input = T;

    /// Construct container using a sample of parsed data for schema analysis
    fn new(_sample: Self::Input) -> Self { <Vec<T>>::new() }

    /// Push a sample of parsed data into the container
    fn push(&mut self, sample: Self::Input) { <Vec<T>>::push(self, sample); }
}


/// Unit tests
#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use ::splitter::split_line_and_run;
    use super::{cpu, interrupts, paging};
    use super::{Data, Parser, PseudoFileParser, Record, RecordKind,
                RecordStream, SampledData};

    /// Check that CPU stats are parsed properly
    #[test]
    fn cpu_record() {
        // Check that we parse global CPU stats well
        check_tag_parsing("cpu", RecordKind::CPUTotal);
        with_record("cpu 98 6 966 48", |record| {
            let cpu_fields = record.parse_cpu();
            assert_eq!(cpu_fields.count(), 4);
        });

        // Check that we parse per-thread CPU stats well
        with_record("cpu42 98 6 966 48 62", |record| {
            check_kind(&record, RecordKind::CPUThread(42));
            let cpu_fields = record.parse_cpu();
            assert_eq!(cpu_fields.count(), 5);
        });
    }

    /// Check that paging stats are parsed properly
    #[test]
    fn paging_record() {
        // Global paging statistics should be parsed well
        check_tag_parsing("page", RecordKind::PagingTotal);
        with_record("page 9846 1367", |record| {
            assert_eq!(record.parse_paging(),
                       paging::RecordFields { incoming: 9846, outgoing: 1367 });
        });

        // Swapping statistics should be parsed well
        check_tag_parsing("swap", RecordKind::PagingSwap);
        with_record("swap 3645 4793", |record| {
            assert_eq!(record.parse_paging(),
                       paging::RecordFields { incoming: 3645, outgoing: 4793 });
        });
    }

    /// Check that interrupt stats are parsed properly
    #[test]
    fn interrupt_record() {
        // Hardware interrupt statistics should be parsed well
        check_tag_parsing("intr", RecordKind::InterruptsHW);
        with_record("intr 127 0 66", |record| {
            let fields = record.parse_interrupts();
            assert_eq!(fields.total, 127);
            assert_eq!(fields.details.count(), 2);
        });

        // Software interrupt statistics should be parsed well
        check_tag_parsing("softirq", RecordKind::InterruptsSW);
        with_record("softirq 666 72 69 0", |record| {
            let fields = record.parse_interrupts();
            assert_eq!(fields.total, 666);
            assert_eq!(fields.details.count(), 3);
        });
    }

    /// Check that context switching stats are parsed properly
    #[test]
    fn context_switches() {
        check_tag_parsing("ctxt", RecordKind::ContextSwitches);
        with_record("ctxt 46115", |record| {
            assert_eq!(record.parse_context_switches(), 46115);
        });
    }

    /// Check that boot time stats are parsed properly
    #[test]
    fn boot_time() {
        check_tag_parsing("btime", RecordKind::BootTime);
        with_record("btime 713705", |record| {
            assert_eq!(record.parse_boot_time(), Utc.timestamp(713705, 0));
        });
    }

    /// Check that process forks are parsed properly
    #[test]
    fn process_forks() {
        check_tag_parsing("processes", RecordKind::ProcessForks);
        with_record("processes 9564", |record| {
            assert_eq!(record.parse_process_forks(), 9564);
        });
    }

    /// Check that process activity is parsed properly
    #[test]
    fn process_activity() {
        // Check that we parse the amount of running processes well
        check_tag_parsing("procs_running", RecordKind::ProcessesRunnable);
        with_record("procs_running 666", |record| {
            assert_eq!(record.parse_processes(), 666);
        });

        // Check that we parse the amount of blocked processes well
        check_tag_parsing("procs_blocked", RecordKind::ProcessesBlocked);
        with_record("procs_blocked 1563", |record| {
            assert_eq!(record.parse_processes(), 1563);
        });
    }

    /// Check that record streams work well
    #[test]
    fn record_stream() {
        // Build a pseudo-file from a set of records
        let pseudo_file = ["cpu  9 8 7 6",
                           "cpu0 7 5 3 1",
                           "cpu1 2 3 4 5",
                           "page 666 999",
                           "swap 333 888",
                           "intr 128 0 3 4 5",
                           "ctxt 6461165",
                           "btime 61616659",
                           "processes 161316",
                           "procs_running 24",
                           "procs_blocked 13",
                           "totally_unsupported 222",
                           "softirq 614651 13 16 61 632"].join("\n");

        // This is the associated record stream
        let record_stream = RecordStream::new(&pseudo_file);

        // Check that our test record stream looks as expected
        check_record_stream(record_stream, &pseudo_file);
    }

    // Check that parsers work well
    #[test]
    fn parser() {
        // Build a pseudo-file from a set of records, use that to init a parser
        let initial_file = ["cpu  9 8 7 6",
                            "cpu0 7 5 3 1",
                            "cpu1 2 3 4 5",
                            "page 666 999",
                            "swap 333 888",
                            "intr 128 0 3 4 5",
                            "ctxt 6461165",
                            "btime 61616659",
                            "processes 161316",
                            "procs_running 24",
                            "procs_blocked 13",
                            "softirq 614651 13 16 61 632"].join("\n");
        let mut parser = Parser::new(&initial_file);

        // Now, build another file which is a variant of the first one, and
        // check that the parser can ingest it just fine
        let file_contents = ["cpu  24 48 72 96",
                             "cpu0 17 22 38 91",
                             "cpu1 7 26 34 5",
                             "page 888 1010",
                             "swap 666 987",
                             "intr 129 0 3 4 5",
                             "ctxt 8461188",
                             "btime 61616659",
                             "processes 191436",
                             "procs_running 14",
                             "procs_blocked 6",
                             "softirq 614851 313 216 61 1632"].join("\n");
        let record_stream = parser.parse(&file_contents);

        // Check that our test record stream looks as expected
        check_record_stream(record_stream, &file_contents);
    }

    /// Check that statistical data containers work as expected
    #[test]
    fn sampled_data() {
        // First, let's define some shortcuts...
        type CpuData = cpu::Data;
        type PagingData = paging::Data;
        type InterruptsData = interrupts::Data;

        // Build a new data container associated with certain file contents. If
        // the push flag is set, also push the same file contents into it so as
        // to create a basic mock data sample.
        let new_sampled_data =
            |file_contents: &str, push: bool| -> Data
        {
            let mut data = Data::new(RecordStream::new(file_contents));
            if push {
                data.push(RecordStream::new(file_contents));
            }
            data
        };

        // Same idea, but for a CPU stats container / file record
        let new_cpu_data =
            |textual_record: &str, push: bool| -> CpuData
        {
            let mut data = with_record(textual_record, |record| {
                CpuData::new(record.parse_cpu())
            });
            if push {
                with_record(textual_record, |record| {
                    data.push(record.parse_cpu());
                });
            }
            data
        };

        // Same idea, but for a paging stats container / file record
        let new_paging_data =
            |textual_record: &str, push: bool| -> PagingData
        {
            let mut data = with_record(textual_record, |record| {
                PagingData::new(record.parse_paging())
            });
            if push {
                with_record(textual_record, |record| {
                    data.push(record.parse_paging());
                });
            }
            data
        };

        // Same idea, but for an interrupts stats container / file record
        let new_interrupts_data =
            |textual_record: &str, push: bool| -> InterruptsData
        {
            let mut data = with_record(textual_record, |record| {
                InterruptsData::new(record.parse_interrupts())
            });
            if push {
                with_record(textual_record, |record| {
                    data.push(record.parse_interrupts());
                });
            }
            data
        };

        // ...and now, onto the actual tests

        // First, check the contents of a sampled data container for empty
        // /proc/stat samples. While technically allowed by the procfs man page,
        // this format is unlikely to be encountered in practice, but it's a
        // good base case which we can build other sampled data tests upon.
        let mut stats = String::new();
        let empty_void_stats = new_sampled_data(&stats, false);
        let mut expected_empty = Data { all_cpus: None,
                                        each_thread: Vec::new(),
                                        paging: None,
                                        swapping: None,
                                        interrupts: None,
                                        context_switches: None,
                                        boot_time: None,
                                        process_forks: None,
                                        runnable_processes: None,
                                        blocked_processes: None,
                                        softirqs: None,
                                        line_target: Vec::new() };
        assert_eq!(empty_void_stats, expected_empty);
        let full_void_stats = new_sampled_data(&stats, true);
        let mut expected_full = expected_empty.clone();
        assert_eq!(full_void_stats, expected_full);

        // We will then test supported records one by one, in the following way
        let mut check_new_record =
            |extra_text: &str,
             update_expected: &Fn(&mut Data, bool)|
         {
            // Add new record(s) to our mock file sample
            stats.push_str(extra_text);

            // Build an empty container for stats matching the current format
            let empty_stats = new_sampled_data(&stats, false);

            // Update our expectations of an empty stats container and check
            // that the container which we've built matches them
            update_expected(&mut expected_empty, false);
            assert_eq!(empty_stats, expected_empty);
            assert_eq!(empty_stats.len(), 0);

            // Build a containers with one stat sample in it
            let full_stats = new_sampled_data(&stats, true);

            // Update our expectations of a full stats container and check that
            // the container which we've built matches them
            update_expected(&mut expected_full, true);
            assert_eq!(full_stats, expected_full);
            assert_eq!(full_stats.len(), 1);
        };

        // ...adding global CPU stats
        const CPU_STR: &str = "cpu 1 2 3 4\n";
        check_new_record(
            CPU_STR,
            &|expected, push| {
                expected.all_cpus = Some(new_cpu_data(CPU_STR, push));
                expected.line_target.push(RecordKind::CPUTotal);
            },
        );

        // ...adding dual-core CPU stats
        check_new_record(
            "cpu0 0 1 1 3
             cpu1 1 1 2 1\n",
            &|expected, push| {
                expected.each_thread = vec![new_cpu_data("cpu0 0 1 1 3", push),
                                            new_cpu_data("cpu1 1 1 2 1", push)];
                expected.line_target.push(RecordKind::CPUThread(0));
                expected.line_target.push(RecordKind::CPUThread(1));
            }
        );

        // ...adding paging stats
        const PAGE_STR: &str = "page 42 43\n";
        check_new_record(
            PAGE_STR,
            &|expected, push| {
                expected.paging = Some(new_paging_data(PAGE_STR, push));
                expected.line_target.push(RecordKind::PagingTotal);
            }
        );

        // ...adding swapping stats
        const SWAP_STR: &str = "swap 24 34\n";
        check_new_record(
            SWAP_STR,
            &|expected, push| {
                expected.swapping = Some(new_paging_data(SWAP_STR, push));
                expected.line_target.push(RecordKind::PagingSwap);
            }
        );

        // ...adding interrupt stats
        const INTR_STR: &str = "intr 12345 678 910\n";
        check_new_record(
            INTR_STR,
            &|expected, push| {
                expected.interrupts = Some(new_interrupts_data(INTR_STR, push));
                expected.line_target.push(RecordKind::InterruptsHW);
            }
        );

        // ...adding context switches
        check_new_record(
            "ctxt 654321\n",
            &|expected, push| {
                expected.context_switches = Some(
                    if push { vec![654321] } else { Vec::new() }
                );
                expected.line_target.push(RecordKind::ContextSwitches);
            }
        );

        // ...adding boot time
        check_new_record(
            "btime 5738295\n",
            &|expected, _push| {
                expected.boot_time = Some(Utc.timestamp(5738295, 0));
                expected.line_target.push(RecordKind::BootTime);
            }
        );

        // ...adding process fork counter
        check_new_record(
            "processes 94536551\n",
            &|expected, push| {
                expected.process_forks = Some(
                    if push { vec![94536551] } else { Vec::new() }
                );
                expected.line_target.push(RecordKind::ProcessForks);
            }
        );

        // ...adding runnable process counter
        check_new_record(
            "procs_running 1624\n",
            &|expected, push| {
                expected.runnable_processes = Some(
                    if push { vec![1624] } else { Vec::new() }
                );
                expected.line_target.push(RecordKind::ProcessesRunnable);
            }
        );

        // ...adding blocked process counter
        check_new_record(
            "procs_blocked 8948\n",
            &|expected, push| {
                expected.blocked_processes = Some(
                    if push { vec![8948] } else { Vec::new() }
                );
                expected.line_target.push(RecordKind::ProcessesBlocked);
            }
        );

        // ...adding softirq stats
        const SOFTIRQ_STR: &str = "softirq 94651 1561 21211 12 71867\n";
        check_new_record(
            SOFTIRQ_STR,
            &|expected, push| {
                expected.softirqs =
                    Some(new_interrupts_data(SOFTIRQ_STR, push));
                expected.line_target.push(RecordKind::InterruptsSW);
            }
        );
    }

    /// Build the record structure associated with a certain line of text
    fn with_record<F, R>(line_of_text: &str, functor: F) -> R
        where F: FnOnce(Record) -> R
    {
        split_line_and_run(line_of_text, |columns| {
            let record = Record::new(columns);
            functor(record)
        })
    }

    /// Exhaustively check that a record has a certain kind
    fn check_kind(record: &Record, expected_kind: RecordKind) {
        assert_eq!(record.kind(), expected_kind);
        assert!(record.has_kind(&expected_kind));
    }

    /// Make sure that record analysis will recognize the proper tag, nothing
    /// more and nothing less. For example, not "cp", not "cpuz", but "cpu".
    fn check_tag_parsing(tag: &str, expected_kind: RecordKind) {
        // Dummy data to make the test look more realistic, shouldn't be parsed
        const TEST_DATA: &str = " 984 654";

        // Start with something that should look like a valid record
        let mut test_record_str = tag.to_owned();
        test_record_str.push_str(TEST_DATA);
        with_record(&test_record_str, |record| {
            check_kind(&record, expected_kind);
        });

        // Check an invalid record where the tag is one character too short
        let mut test_tag = (&tag[..tag.len()-1]).to_owned();
        test_record_str = test_tag.clone();
        test_record_str.push_str(TEST_DATA);
        with_record(&test_record_str, |record| {
            check_kind(&record, RecordKind::Unsupported(test_tag));
        });

        // Check an invalid record where the tag is one character too long
        test_tag = tag.to_owned();
        test_tag.push('z');
        test_record_str = test_tag.clone();
        test_record_str.push_str(TEST_DATA);
        with_record(&test_record_str, |record| {
            check_kind(&record, RecordKind::Unsupported(test_tag));
        });
    }

    /// Test that the output of a record stream is right for a given input file
    fn check_record_stream(mut stream: RecordStream, file_contents: &str) {
        for record_str in file_contents.lines() {
            with_record(record_str, |expected_record| {
                // Check that the record is there in the actual stream
                let mut actual_record = stream.next().unwrap();

                // Check that the header matches
                assert_eq!(expected_record.header, actual_record.header);

                // Check that the columns match
                for expected_column in expected_record.data_columns {
                    assert_eq!(actual_record.data_columns.next(),
                               Some(expected_column));
                }
                assert_eq!(actual_record.data_columns.next(), None);
            });
        }
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
