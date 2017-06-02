# Performance analysis interface to procfs

When measuring system and application performance on Linux, a lot can be learned
just by reading or polling the contents of /proc. Unfortunately, doing so
programmatically is made needlessly difficult (and inefficient) by the UNIX
obsession that everything should be a human-readable text file.

This library aims to correct this by providing a Rust interface to the
procfs-provided system performance data which is easy to use and as efficient
as reasonably feasible, with an eye towards repeated sampling measurements.


## How much of procfs?

Procfs is full of legacy cruft and of low-level hardware details which are not
interesting for performance studies (e.g. a map of memory-mapped hardware).
Interfacing all of this would take a lot of effort and serve little purpose.

The first step of this project is thus to determine which parts of procfs should
be interfaced in a performance analysis API. You will find in the following a
list of the contents of procfs on a typical Linux 4.11 system, sorted in 3
buckets: to be interfaced, to be ignored, and to be investigated further.

Two useful sources of information while writing this list were the kernel
documentation (e.g. Documentation/filesystems/proc.txt in the kernel source
tree) and `man 5 proc`.

### To be interfaced

The information within files or folders marked as "one-time" should not change
over the course of a reasonable performance measurement, and should thus have
an API which is optimized for single-shot readout (as opposed to sampling).

[ ] **/proc/<pid>/clear_refs:** Write-only. May be used to estimate the memory
    working set of a process by clearing various page accessed/dirty bits, with
    the aim to check them again later on using e.g. "smaps".
[ ] **/proc/<pid>/cmdline:** Process command line. May be used to map PIDs to
    human-readable binary names.
[ ] **/proc/<pid>/comm:** Very short (15 bytes + NULL) version of a process'
    binary name. Would be redundant with cmdline if it weren't for the fact that
    this mechanism can also be used to give threads a name.
[ ] **/proc/<pid>/cpuset:** Readout of the process' CPU affinity configuration.
    "/" means no affinity. For more information, see "man 7 cpuset".
[ ] **/proc/<pid>/task/<tid>/comm:** Customizable thread identifier, follows
    the same conventions as the process-wide "comm" and defaults to its value.
[ ] **/proc/buddyinfo:** State of the buddy memory allocator, can hint towards
    RAM fragmentation issues
[ ] **/proc/cmdline:** (one-time) Kernel command line, may be combined with
    /proc/version to implement system-specific hacks.
[ ] **/proc/cpuinfo:** (one-time) System CPU configuration, has many uses
    including distinguishing hyperthreads from physical CPU cores.
[ ] **/proc/diskstats:** Usage of block peripherals, including disk drives.
[ ] **/proc/interrupts:** Hardware CPU interrupt counters.
[ ] **/proc/locks:** POSIX file locks, may help nail down IO scalability issues.
[ ] **/proc/meminfo:** Detailed RAM usage statistics.
[ ] **/proc/pagetypeinfo:** More detailed variant of /proc/buddyinfo.
[ ] **/proc/schedstat:** Kernel scheduler usage statistics, see also
    Documentation/scheduler/sched-stats.txt in kernel source tree.
[ ] **/proc/sched_debug:** More detailed & less documented variant of schedstat.
[ ] **/proc/slabinfo:** Root-only. Kernel memory allocator stats.
[ ] **/proc/softirqs:** Software interrupt counters.
[ ] **/proc/stat:** General system performance statistics, should probably be
    interfaced first due to its diverse contents & high usefulness.
[ ] **/proc/swaps:** Usage of swap partitions, if any.
[ ] **/proc/timer_list:** Usage of timer interrupts.
[ ] **/proc/uptime:** Total time elapsed since system startup, and time spent
    idle (no process running).
[ ] **/proc/version:** Kernel version string. Can be used to gracefully detect
    kernel version incompatibilities.
[ ] **/proc/vmstat:** Detailed virtual memory usage statistics.
[ ] **/proc/zoneinfo:** More detailed memory usage statistics, with an eye
    towards memory zones. Could prove very helpful in NUMA studies.

### To be ignored

These files or folders have not been deemed sufficiently useful to performance
studies in order to justify the cost of implementing a parser & API for them.

* **/proc/<pid>/attr:** Process security attributes, mostly used by SELinux.
* **/proc/<pid>/autogroup:** Optional mechanism used by the CFS Linux kernel
  scheduler to group related processes (e.g. make -j) together.
* **/proc/<pid>/auxv:** Binary metadata used by the dynamic linker. Little of it
  is interesting for performance analysis, and all of it can be found elsewhere.
* **/proc/<pid>/cgroup:** Process-specific cgroup metadata. See also "cgroups".
* **/proc/<pid>/coredump_filter:** Process core dump control.
* **/proc/acpi/:** Most ACPI-related stuff has moved to sysfs, and on my PC
  this folder only tells which peripheral may wake up the system from sleep.
* **/proc/cgroups:** While the process isolation brought by cgroups has the
  potential to make it harder to reason about performance, this basic list of
  cgroups does not look useful for performance studies.
* **/proc/consoles:** Describes active terminals, does not seem useful here.
* **/proc/crypto:** List of crypto algorithms implemented in the kernel.
* **/proc/dma:** List of ISA DMA channels. Generally obsolete.
* **/proc/execdomains:** Various UNIX compatibility layers. Rarely used.
* **/proc/filesystems:** List of filesystems supported by the active kernel.
* **/proc/i8k:** A small bunch of Dell-specific BIOS metadata.
* **/proc/iomem:** Map of memory-mapped IO.
* **/proc/ioports:** Mapping of CPU IO ports to kernel drivers.
* **/proc/kallsyms:** Root-only. Kernel symbol table, used by dylibs and perf.
* **/proc/kcore:** Root-only. A huge raw dump of kernel *virtual* memory.
* **/proc/keys:** One interface to the kernel crypto secret management system.
* **/proc/key-users:** Another interface to kernel-managed crypto secrets.
* **/proc/kmsg:** Root-only. Raw kernel log dump, used by dmesg, syslog and
  friends. Should not be concurrently accessed by multiple processes.
* **/proc/kpagecgroup:** Mapping of system memory to active cgroups.
* **/proc/kpagecount:** Root-only. Metadata on physical RAM page aliasing.
* **/proc/kpageflags:** Root-only. Various flag-based physical RAM properties.
* **/proc/latency_stats:** Lonely remainder of LatencyTOP, a past attempt to
  help diagnose Linux' latency woes. Long gone.
* **/proc/loadavg:** An old and deeply flawed system utilization metric, based
  on the amount of running processes and conflating CPU utilization with IO
  wait. Should not be used in serious performance studies.
* **/proc/modules:** List and basic properties of loaded kernel modules.
* **/proc/mtrr:** Physical RAM bus caching settings, as set by the CPU's memory
  type range registers. Intel-specific.
* **/proc/sysrq-trigger:** Procfs-based equivalent of the Magic SysRq requests.
  Blunt and violent tools, only suitable for rough kernel debugging.
* **/proc/tty:** Low-level metadata on virtual consoles and serial ports.
* **/proc/vmallocinfo:** Root-only. Detailed map of virtual memory allocations.

### To be investigated further

TODO
