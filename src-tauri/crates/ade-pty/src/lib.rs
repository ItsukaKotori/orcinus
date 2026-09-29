pub mod cpr;
pub mod server;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct ThroughputReport {
    pub received_bytes: u64,
    pub chunk_bytes: usize,
    pub elapsed: Duration,
    pub mb_per_second: f64,
}

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
        // cpr::scan_and_reply, leaves the pipe silent); a stall must fail, not hang.
        let remaining = READ_DEADLINE.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match chunk_rx.recv_timeout(remaining) {
            Ok(chunk) => {
                cpr::scan_and_reply(&mut scan_tail, &chunk, &mut writer)?;
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

/// 仅面向本 crate 集成测试的辅助：echo 行为的 WS server。Task 7 换会话路由后，
/// 集成测试应改用显式 handler，此模块届时一并退役。
#[doc(hidden)]
pub mod test_support {
    use futures_util::{SinkExt, StreamExt};

    /// 起 echo server（绑 127.0.0.1:0），返回 `(port, token, serve 任务句柄)`。
    /// 须在 tokio 运行时上下文内调用（集成测试用 `#[tokio::test]`）。
    pub fn start_echo_server() -> (u16, String, tokio::task::JoinHandle<()>) {
        // WHY: `Handle::block_on` 在运行时上下文内调用会 panic（"Cannot start a
        // runtime from within a runtime"），故先以 std 绑定端口，再在当前运行时
        // 上下文里注册为异步 listener——签名与行为不变。
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
        let token = crate::server::generate_token();
        let handler: crate::server::ConnectionHandler = std::sync::Arc::new(|_id, ws| {
            tokio::spawn(async move {
                let (mut tx, mut rx) = ws.split();
                while let Some(Ok(msg)) = rx.next().await {
                    if let tokio_tungstenite::tungstenite::Message::Binary(b) = msg {
                        if tx
                            .send(tokio_tungstenite::tungstenite::Message::Binary(b))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            });
        });
        let handle = tokio::spawn(crate::server::serve(listener, token.clone(), handler));
        (port, token, handle)
    }
}
