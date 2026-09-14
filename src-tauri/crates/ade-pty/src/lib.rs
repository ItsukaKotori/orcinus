use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct ThroughputReport {
    pub received_bytes: u64,
    pub chunk_bytes: usize,
    pub elapsed: Duration,
    pub mb_per_second: f64,
}

const CPR_QUERY: &[u8] = b"\x1b[6n";
const CPR_REPLY: &[u8] = b"\x1b[1;1R";

/// Upper bound for one measurement run; on expiry the sink is killed so a stalled
/// PTY fails as `received_bytes < total_bytes` instead of hanging the caller.
pub const READ_DEADLINE: Duration = Duration::from_secs(30);

pub fn measure_pty_throughput(
    sink_exe: &Path,
    total_bytes: usize,
) -> std::io::Result<ThroughputReport> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(std::io::Error::other)?;
    let mut cmd = CommandBuilder::new(sink_exe);
    cmd.arg(total_bytes.to_string());
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(std::io::Error::other)?;
    drop(pair.slave);
    let mut writer = pair.master.take_writer().map_err(std::io::Error::other)?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(std::io::Error::other)?;
    let chunk_bytes = 64 * 1024;
    let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = vec![0u8; chunk_bytes];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if chunk_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let started = Instant::now();
    let mut received = 0usize;
    let mut scan_tail: Vec<u8> = Vec::new();
    while received < total_bytes {
        // WHY: bounded wait guards the ConPTY startup block (an unanswered CPR query, see
        // reply_to_cursor_query, leaves the pipe silent); a stall must fail, not hang.
        let remaining = READ_DEADLINE.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match chunk_rx.recv_timeout(remaining) {
            Ok(chunk) => {
                reply_to_cursor_query(&mut scan_tail, &chunk, &mut writer)?;
                received += chunk.len();
            }
            Err(_) => break,
        }
    }
    let elapsed = started.elapsed();
    let _ = child.kill();
    drop(pair.master);
    // NOTE: not joined on purpose; on Windows the ConPTY pipe can stay open after the
    // child is killed, so joining would reintroduce the hang. The reader dies with the process.
    drop(reader_thread);
    Ok(ThroughputReport {
        received_bytes: received as u64,
        chunk_bytes,
        elapsed,
        mb_per_second: (received as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64(),
    })
}

fn reply_to_cursor_query(
    scan_tail: &mut Vec<u8>,
    chunk: &[u8],
    writer: &mut dyn Write,
) -> std::io::Result<()> {
    scan_tail.extend_from_slice(chunk);
    if scan_tail.windows(CPR_QUERY.len()).any(|w| w == CPR_QUERY) {
        writer.write_all(CPR_REPLY)?;
        writer.flush()?;
        scan_tail.clear();
    } else {
        let keep = scan_tail.len().min(CPR_QUERY.len() - 1);
        let drop = scan_tail.len() - keep;
        scan_tail.drain(..drop);
    }
    Ok(())
}
