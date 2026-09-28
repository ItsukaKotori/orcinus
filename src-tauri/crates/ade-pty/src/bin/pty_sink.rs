fn main() {
    let total: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024 * 1024);
    let chunk = vec![b'x'; 64 * 1024];
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut written = 0usize;
    while written < total {
        let n = chunk.len().min(total - written);
        if std::io::Write::write_all(&mut out, &chunk[..n]).is_err() {
            break;
        }
        written += n;
    }
    let _ = std::io::Write::flush(&mut out);
}
