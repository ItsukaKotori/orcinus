use ade_pty::measure_pty_throughput;
use std::path::Path;

#[test]
fn pty_delivers_expected_bytes_within_budget() {
    let total_bytes = 8 * 1024 * 1024;
    let sink_exe = Path::new(env!("CARGO_BIN_EXE_pty_sink"));
    let report = measure_pty_throughput(sink_exe, total_bytes).expect("pty run");
    let delta = report.received_bytes as i64 - total_bytes as i64;
    println!(
        "pty throughput: {:.1} MB/s, received={} total={} delta={}, chunk={} bytes, elapsed={:?}",
        report.mb_per_second,
        report.received_bytes,
        total_bytes,
        delta,
        report.chunk_bytes,
        report.elapsed
    );
    assert!(
        report.received_bytes >= total_bytes as u64,
        "received {} bytes, expected at least {}",
        report.received_bytes,
        total_bytes
    );
}
