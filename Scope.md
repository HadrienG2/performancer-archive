# How much of procfs should this project cover?

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

## To be interfaced

The information within files or folders marked as "one-time" should not change
over the course of a reasonable performance measurement, and should thus have
an API which is optimized for single-shot readout (as opposed to sampling).

[ ] **/proc/<pid>/clear_refs:** Write-only. May be used to estimate the memory
    working set of a process by clearing various page accessed/dirty bits, with
    the aim to check them again later on using e.g. "smaps".
[ ] **/proc/<pid>/cmdline:** Process command line. May be used to map PIDs to
    human-readable command names.
[ ] **/proc/<pid>/comm:** Very short (15 bytes + NULL) version of a process'
    binary name. Would be redundant with cmdline if it weren't for the fact that
    this mechanism can also be used to give threads a name.
[ ] **/proc/<pid>/cpuset:** Readout of the process' CPU affinity configuration.
    "/" means no affinity. For more information, see "man 7 cpuset".
[ ] **/proc/<pid>/io:** Basic statistics on the IO activity of a process.
[ ] **/proc/<pid>/limits:** Resource limits for a given process.
[ ] **/proc/<pid>/net:** Network configuration and statistics for the active
    process' networking namespace. Not fully interfaced.
[ ] **/proc/<pid>/net/dev:** Per-interface statistics on network activity.
[ ] **/proc/<pid>/net/dev_snmp6:** More detailed, but IPv6-specific variant.
[ ] **/proc/<pid>/net/ip6_mr_cache:** Active IPv6 multicast routes.
[ ] **/proc/<pid>/net/ip6_mr_vif:** Active IPv6 multicast virtual interfaces.
[ ] **/proc/<pid>/net/ip_mr_cache:** Active IPv4 multicast routes.
[ ] **/proc/<pid>/net/ip_mr_vif:** Active IPv4 multicast virtual interfaces.
[ ] **/proc/<pid>/net/netstat:** Lots of TCP & IP usage statistics.
[ ] **/proc/<pid>/sched:** Scheduling statistics for a given process.
[ ] **/proc/<pid>/schedstat:** Process-specific version of some of the
    load_balance statistics from /proc/schedstat. See
    http://eaglet.rain.com/rick/linux/schedstat/v15/format-15.html
[ ] **/proc/<pid>/smaps:** Detailed statistics about a the memory usage of a
    process' virtual memory regions. See also maps, numa_maps.
[ ] **/proc/<pid>/stat:** A huge bunch of very diverse information, ranging from
    CPU occupancy to the active instruction pointer. Should probably be
    interfaced first.
[ ] **/proc/<pid>/statm:** Some data on a process' overall memory consumption.
[ ] **/proc/<pid>/status:** Historically meant as a human-readable variant of
    stat and statm, but might have grown new fields since.
[ ] **/proc/<pid>/task/<tid>:** Information about a given thread ("task").
    Also duplicates some process-global info. Good luck figuring out what is
    thread-specific and what is process-wide...
[ ] **/proc/<pid>/task/<tid>/comm:** Customizable thread identifier, follows
    the same conventions as the process-wide "comm" and defaults to its value.
[ ] **/proc/<pid>/timers:** Process-specific UNIX timer usager information.
    Related to the system-wide /proc/timers.
[ ] **/proc/buddyinfo:** State of the buddy memory allocator, can hint towards
    RAM fragmentation issues
[ ] **/proc/cmdline:** (one-time) Kernel command line, may be combined with
    /proc/version to implement system-specific hacks.
[ ] **/proc/cpuinfo:** (one-time) System CPU configuration, has many uses
    including distinguishing hyperthreads from physical CPU cores.
[X] **/proc/diskstats:** Usage of block peripherals, including disk drives.
[ ] **/proc/interrupts:** Hardware CPU interrupt counters.
[ ] **/proc/locks:** POSIX file locks, may help nail down IO scalability issues.
[X] **/proc/meminfo:** Detailed RAM usage statistics.
[ ] **/proc/net:** Basically a symlink to /proc/self/net.
[ ] **/proc/pagetypeinfo:** More detailed variant of /proc/buddyinfo.
[ ] **/proc/schedstat:** Kernel scheduler usage statistics, see also
    Documentation/scheduler/sched-stats.txt in kernel source tree.
[ ] **/proc/sched_debug:** More detailed & less documented variant of schedstat.
[ ] **/proc/slabinfo:** Root-only. Kernel memory allocator stats.
[ ] **/proc/softirqs:** Software interrupt counters.
[X] **/proc/stat:** General system performance statistics, should probably be
    interfaced first due to its diverse contents & high usefulness.
[ ] **/proc/swaps:** Usage of swap partitions, if any.
[ ] **/proc/timer_list:** Usage of timer interrupts.
[X] **/proc/uptime:** Total time elapsed since system startup, and time spent
    idle (no process running).
[X] **/proc/version:** Kernel version string. Can be used to gracefully detect
    kernel version incompatibilities.
[ ] **/proc/vmstat:** Detailed virtual memory usage statistics.
[ ] **/proc/zoneinfo:** More detailed memory usage statistics, with an eye
    towards memory zones. Could prove very helpful in NUMA studies.

## To be ignored

These files or folders have not been deemed sufficiently useful to performance
studies in order to justify the cost of implementing a parser & API for them.

* **/proc/<pid>/attr:** Process security attributes, mostly used by SELinux.
* **/proc/<pid>/autogroup:** Optional mechanism used by the CFS Linux kernel
  scheduler to group related processes (e.g. make -j) together.
* **/proc/<pid>/auxv:** Binary metadata used by the dynamic linker. Little of it
  is interesting for performance analysis, and all of it can be found elsewhere.
* **/proc/<pid>/cgroup:** Process-specific cgroup metadata. See also "cgroups".
* **/proc/<pid>/coredump_filter:** Process core dump control.
* **/proc/<pid>/cwd:** Current working directory of the process.
* **/proc/<pid>/environ:** Current environment variables of the process.
* **/proc/<pid>/exe:** Symlink to the current executable of the process.
* **/proc/<pid>/fd:** Link to the files manipulated by the process.
* **/proc/<pid>/fdinfo:** Basic information on the process' file descriptors.
* **/proc/<pid>/gid_map:** Group id mapping, part of user namespace feature.
* **/proc/<pid>/latency:** Per-process LatencyTOP interface. Same caveat as for
  the system-wide /proc/latency_stats: seems obsolete, disabled by default.
* **/proc/<pid>/loginuid:** Used by PAM to tell which account a certain user
  gained access to the system with.
* **/proc/<pid>/make-it-fail:** Part of the kernel's fault injection system.
* **/proc/<pid>/map_files:** Memory-mapped files and their vmem location.
* **/proc/<pid>/maps:** Map of a process' virtual memory allocations.
* **/proc/<pid>/mem:** Raw access to a process' virtual address space.
* **/proc/<pid>/mountinfo:** Mount points accessible to this process.
* **/proc/<pid>/mounts:** An older (Linux 2.4) version of the same thing.
* **/proc/<pid>/mountstats:** More metadata about mount points.
* **/proc/<pid>/net/anycast6:** IPv6 anycast addresses, if enabled.
* **/proc/<pid>/net/arp:** Kernel ARP tables, used for address resolutions.
* **/proc/<pid>/net/connector:** Connector mechanism, used to receive
  notifications of process events, such as fork, exec, UID/GID changes...
* **/proc/<pid>/net/dev_mcast:** Layer 2 multicast groups being listened to.
* **/proc/<pid>/net/fib_trie:** Kernel routing table.
* **/proc/<pid>/net/fib_triestat:** Statistics on the kernel routing table.
* **/proc/<pid>/net/icmp:** Information on active ICMP connections.
* **/proc/<pid>/net/icmp6:** IPv6 version of the "icmp" file.
* **/proc/<pid>/net/if_net6:** Configured IPv6 addresses on the system.
* **/proc/<pid>/net/igmp:** Information on active IGMP connections.
* **/proc/<pid>/net/igmp6:** IPv6 version of the "igmp" file.
* **/proc/<pid>/net/ip6_flowlabel:** Active IPv6 flow labels.
* **/proc/<pid>/net/ip_tables_matches:** List of currently loaded matching
  modules of the netfilter kernel firewall.
* **/proc/<pid>/net/ip_tables_names:** Supported netfilter tables.
* **/proc/<pid>/net/ip_tables_targets:** Targets of netfilter rules (?).
* **/proc/<pid>/net/ipv6_route:** Active IPv6 routes.
* **/proc/<pid>/net/mcfilter:** Some IPv4 multicast filtering mechanism (?).
* **/proc/<pid>/net/mcfilter6:** Probably the IPv6 version of mcfilter (?).
* **/proc/<pid>/net/netfilter:** More netfilter stuff, currently just logs.
* **/proc/<pid>/net/netlink:** List of active Netlink sockets, used by processes
  that wish to communicate with network drivers directly.
* **/proc/<pid>/net/packet:** List of programs that can sniff network packets.
* **/proc/<pid>/net/pnp:** Something related to DNS. Poorly documented across
  the web, and seems broken on my machine (only reports nameserver 0.0.0.0).
* **/proc/<pid>/ns:** Namespaces which a process belongs to.
* **/proc/<pid>/numa_maps:** Some NUMA-related metadata on a process' virtual
  address space. Seems quite hard to interprete.
* **/proc/<pid>/oom_adj:** Write-only. Adjusts odds that a process will be
  killed by the kernel Out-of-Memory killer in low-RAM scenarios. Deprecated.
* **/proc/<pid>/oom_score:** Odds of being killed by the OOM killer.
* **/proc/<pid>/oom_score_adj:** Current (as of Linux 2.6) variant of oom_adj.
* **/proc/<pid>/pagemap:** Kernel page table for this process.
* **/proc/<pid>/personality:** Process-specific UNIX compatibility settings.
* **/proc/<pid>/projid_map:** Project ID, used by some filesystems like XFS.
* **/proc/<pid>/root:** Root of this process' filesystem, as set by chroot.
* **/proc/<pid>/sessionid:** Numerical identifier of the terminal session that
  spawned this process.
* **/proc/<pid>/setgroups:** Permission to change a process' group membership.
* **/proc/<pid>/stack:** Symbolic trace of a process' kernel stack.
* **/proc/<pid>/syscall:** Current system call being executed, and its args.
* **/proc/<pid>/task/<tid>/children:** Space-separated list of task children.
  Not reliable unless the process is frozen.
* **/proc/<pid>/timerslack_ns:** Current process timer slack, used to save power
  by grouping timer interrupts for different processes. Editable.
* **/proc/<pid>/uid_map:** Like gid_map, but for user IDs.
* **/proc/<pid>/wchan:** Symbolic name corresponding to a location in the kernel
  where a process is sleeping.
* **/proc/acpi/:** Most ACPI-related stuff has moved to sysfs, and on my PC
  this folder only tells which peripheral may wake up the system from sleep.
* **/proc/cgroups:** While the process isolation brought by cgroups has the
  potential to make it harder to reason about performance, this basic list of
  cgroups does not look useful for performance studies.
* **/proc/consoles:** Describes active terminals, does not seem useful here.
* **/proc/crypto:** List of crypto algorithms implemented in the kernel.
* **/proc/dma:** List of ISA DMA channels. Generally obsolete.
* **/proc/execdomains:** Various UNIX compatibility layers. Rarely used.
* **/proc/fb:** List of active framebuffers.
* **/proc/filesystems:** List of filesystems supported by the active kernel.
* **/proc/i8k:** A small bunch of Dell-specific BIOS metadata.
* **/proc/iomem:** Map of memory-mapped IO.
* **/proc/ioports:** Mapping of CPU IO ports to kernel drivers.
* **/proc/kallsyms:** Root-only. Kernel symbol table, used by dylibs and perf.
* **/proc/kcore:** Root-only. Raw access to the kernel' virtual address space.
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
* **/proc/mounts:** Just a mirror of /proc/self/mounts (backwards compatible).
* **/proc/mtrr:** Physical RAM bus caching settings, as set by the CPU's memory
  type range registers. Intel-specific.
* **/proc/self:** Symlink to PID-specific info for the active process.
* **/proc/sysrq-trigger:** Procfs-based equivalent of the Magic SysRq requests.
  Blunt and violent tools, only suitable for rough kernel debugging.
* **/proc/thread-self:** Symlink to thread-specific info for the active thread.
* **/proc/tty:** Low-level metadata on virtual consoles and serial ports.
* **/proc/vmallocinfo:** Root-only. Detailed map of virtual memory allocations.

## To be investigated further

I should take a deeper look at these before taking a hard decision about them:

* **/proc/<pid>/net/:** This describes the network configuration, as seen by the
  current process. I have started studying it, but do not yet have complete
  coverage of it in the lists above.
* **/proc/asound/:** This is some data about the ALSA sound infrastructure. It is
  unclear at this point whether there is something useful in there for the
  purpose of performance studies.
* **/proc/bus/:** A detailed description of various hardware buses.
* **/proc/config.gz:** The kernel configuration. Can be interesting, but is
  somewhat hard to parse (due to gzip compression) and may not be available
  depending on kernel configuration. Probably not a priority.
* **/proc/devices:** A mapping between device names and numbers.
* **/proc/fs/:** Various data about active filesystem drivers.
* **/proc/irq/:** Various data about interrupt sources.
* **/proc/misc:** A string-to-integer mapping of unclear purpose.
* **/proc/partitions:** A description of the disk partitioning setup.
* **/proc/scsi/:** Various data about SCSI devices.
* **/proc/sys/:** Lots of unrelated things. Includes useful configuration for
  interfacing perf.
* **/proc/sysvipc/:** Statistics about the System V interprocess communication
  mechanisms: pipes, shared memory regions, queues...
