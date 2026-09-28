fn main() {
    let total_bytes: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(8 * 1024 * 1024);
    let sink = std::env::current_exe()
        .expect("current exe")
        .with_file_name(format!("pty_sink{}", std::env::consts::EXE_SUFFIX));
    if !sink.is_file() {
        eprintln!(
            "pty_bench: sink binary not found at {}\n\
             `cargo run --bin pty_bench` does not build sibling bins; build them first with:\n\
             cargo build -p ade-pty --bins{}",
            sink.display(),
            if cfg!(debug_assertions) {
                ""
            } else {
                " --release"
            }
        );
        std::process::exit(1);
    }
    let report = ade_pty::measure_pty_throughput(&sink, total_bytes).expect("pty run");
    println!(
        "pty throughput: {:.1} MB/s (received {} bytes in {:?}, sink={})",
        report.mb_per_second,
        report.received_bytes,
        report.elapsed,
        sink.display()
    );
}
