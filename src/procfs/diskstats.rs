//! This module contains a sampling parser for /proc/diskstats


// TODO: Mechanism for sampling measurements from /proc/meminfo


/// Data samples from /proc/meminfo, in structure-of-array layout
///
/// TODO: Complete this description after providing an initial layout
struct DiskStatsData {
    // TODO: File-ordered list of untagged records (as in MemInfoData), with
    //       storage optimization for zero (as in InterruptStatData)
    
    // TODO: Indexes which allow someone in possession of a device name or of
    //       a major and minor device number to find the associated record
    // TODO: Decide whether to expose the major/minor nesting in the API or to
    //       flatten the hierarchy and query by both at the same time
}


// TODO: Sampled records from /proc/distats, with a zero-record optimization


// TODO: Unit tests


/// Performance benchmarks
///
/// See the lib-wide benchmarks module for details on how to use these.
///
#[cfg(test)]
mod benchmarks {
    use ::reader::ProcFileReader;
    use testbench;

    /// Benchmark for the raw diskstats readout overhead
    #[test]
    #[ignore]
    fn readout_overhead() {
        let mut reader =
            ProcFileReader::open("/proc/diskstats")
                           .expect("Failed to open disk stats");
        testbench::benchmark(90_000, || {
            reader.sample(|_| {}).expect("Failed to read disk stats");
        });
    }

    // TODO: Benchmark for the full diskstats sampling overhead
    /* #[test]
    #[ignore]
    fn sampling_overhead() {
        let mut stat =
            DiskStatsSampler::new()
                             .expect("Failed to create a disk stats sampler");
        testbench::benchmark(400_000, || {
            stat.sample().expect("Failed to sample disk stats");
        });
    } */
}
