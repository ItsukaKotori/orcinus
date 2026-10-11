//! Unix-socket connect with retry (spec §3.2), mirroring the timing of
//! `orca:src/main/computer/macos-native-provider-socket.ts` (100ms cadence,
//! ~10s budget). A missing/not-yet-bound helper socket is expected during
//! startup, so every connect failure is retried until the budget runs out.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

/// Delay between connect attempts used by `HelperTransport`.
pub const DEFAULT_ATTEMPT_INTERVAL: Duration = Duration::from_millis(100);
/// Maximum connect attempts used by `HelperTransport` (~9.9s of retrying).
pub const DEFAULT_MAX_ATTEMPTS: u32 = 100;

/// Try to connect to `path`, retrying `max_attempts - 1` times with
/// `attempt_interval` in between. Returns the error from the last attempt when
/// every attempt fails; `max_attempts == 0` fails immediately with
/// `ErrorKind::TimedOut`.
pub fn connect_with_retry(
    path: &Path,
    attempt_interval: Duration,
    max_attempts: u32,
) -> io::Result<UnixStream> {
    if max_attempts == 0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "no connect attempts were made",
        ));
    }
    let mut last_error = io::Error::new(io::ErrorKind::TimedOut, "connect never attempted");
    for attempt in 0..max_attempts {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = error,
        }
        if attempt + 1 < max_attempts {
            sleep(attempt_interval);
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ade-computer-socket-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn connects_immediately_when_the_listener_is_present() {
        let temp = TempDir::new();
        let socket_path = temp.0.join("provider.sock");
        let _listener = UnixListener::bind(&socket_path).expect("bind");
        let stream = connect_with_retry(&socket_path, Duration::from_millis(1), 1);
        assert!(stream.is_ok());
    }

    #[test]
    fn retries_until_the_listener_appears() {
        let temp = TempDir::new();
        let socket_path = temp.0.join("provider.sock");
        let listener_path = socket_path.clone();
        let listener = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            UnixListener::bind(&listener_path).expect("bind")
        });
        let stream = connect_with_retry(&socket_path, Duration::from_millis(5), 100)
            .expect("connect succeeds after retries");
        drop(stream);
        let _listener = listener.join().expect("listener thread");
    }

    #[test]
    fn fails_with_the_last_error_after_every_attempt() {
        let temp = TempDir::new();
        let missing = temp.0.join("missing.sock");
        let error = connect_with_retry(&missing, Duration::from_millis(1), 3)
            .expect_err("missing socket must fail");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn zero_attempts_is_a_timeout() {
        let temp = TempDir::new();
        let missing = temp.0.join("missing.sock");
        let error = connect_with_retry(&missing, Duration::from_millis(1), 0)
            .expect_err("zero attempts must fail");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
