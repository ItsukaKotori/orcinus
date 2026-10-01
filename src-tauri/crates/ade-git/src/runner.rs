//! Tolerant git process execution.
//!
//! Unlike the fail-fast helpers in the crate root, [`run_git_in`] keeps a
//! non-zero exit status as an `Ok` value so callers can inspect stderr and
//! decide what a failure means. Both cancellation and timeouts kill the child.

use ade_core::errors::CoreError;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

pub fn run_git_in(
    cwd: &str,
    args: &[&str],
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> Result<GitOutput, CoreError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args(args);
    match run_process(&mut command, timeout, cancel) {
        Ok(output) => Ok(GitOutput {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
            Err(CoreError::GitCommandCancelled {
                command: args.join(" "),
            })
        }
        Err(error) => Err(error.into()),
    }
}

/// Spawns `command`, captures both streams on reader threads, and polls every
/// 10ms for completion, cancellation, or the deadline. The child is killed and
/// reaped on cancellation (`Interrupted`) and on timeout (`TimedOut`).
pub(crate) fn run_process(
    command: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> std::io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = std::thread::spawn(move || read_all(stdout));
    let stderr_reader = std::thread::spawn(move || read_all(stderr));

    let status = match wait_with_timeout(&mut child, timeout, cancel) {
        Ok(Some(status)) => status,
        Ok(None) => {
            reap(&mut child);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "git command timed out",
            ));
        }
        Err(error) => {
            reap(&mut child);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(error);
        }
    };

    Ok(Output {
        status,
        stdout: stdout_reader.join().expect("stdout reader panicked")?,
        stderr: stderr_reader.join().expect("stderr reader panicked")?,
    })
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if cancel.is_some_and(CancelToken::is_cancelled) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "git command cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_all(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn run_git_in_keeps_nonzero_exit_as_ok() {
        let dir = std::env::temp_dir();
        let output = run_git_in(
            dir.to_str().unwrap(),
            &["rev-parse", "--is-inside-work-tree"],
            Duration::from_secs(5),
            None,
        )
        .expect("process ran");
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn cancel_token_kills_running_process() {
        let token = CancelToken::new();
        let canceller = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            canceller.cancel();
        });
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = Instant::now();
        let error = run_process(&mut command, Duration::from_secs(10), Some(&token)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_still_wins_over_no_cancel() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let error = run_process(&mut command, Duration::from_millis(200), None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }
}
