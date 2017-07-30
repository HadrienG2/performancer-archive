# Performance analysis interface to procfs (and later sysfs)

When measuring system and application performance on Linux, a lot can be learned
just by reading or polling the contents of /proc. Unfortunately, doing so
programmatically is made needlessly difficult (and inefficient) by the UNIX
obsession that everything should be a human-readable text file.

This library aims to correct this by providing a Rust interface to the
procfs-provided system performance data which is easy to use and as efficient
as reasonably feasible, with an eye towards repeated sampling measurements.

See Scope.md for more details about the parts of procfs that have been
investigated, which parts are currently scheduled to be integrated, and which
will be left out unless someone steps in and makes a strong case for them.
