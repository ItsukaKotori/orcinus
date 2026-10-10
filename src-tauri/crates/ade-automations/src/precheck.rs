//! Automation precheck executor: runs the automation's shell command with a
//! timeout and process-group kill, producing the `AutomationPrecheckResult`
//! JSON the renderer consumes. Ported from
//! `orca:src/main/automations/precheck-runner.ts` (local target only).

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

pub const DEFAULT_PRECHECK_TIMEOUT_SECONDS: u64 = 60;
pub const MAX_PRECHECK_TIMEOUT_SECONDS: u64 = 600;
pub const MAX_PRECHECK_OUTPUT_CHARS: usize = 4000;
/// Reader threads retain at most this much per stream; beyond it the oldest
/// bytes are dropped so the tail is always preserved.
const MAX_PRECHECK_BUFFER_BYTES: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const FORCE_KILL_AFTER: Duration = Duration::from_secs(2);

pub fn resolve_timeout_seconds(timeout_seconds: Option<u64>) -> u64 {
    timeout_seconds
        .unwrap_or(DEFAULT_PRECHECK_TIMEOUT_SECONDS)
        .clamp(1, MAX_PRECHECK_TIMEOUT_SECONDS)
}

pub fn run_precheck(command: &str, cwd: Option<&str>, timeout_seconds: Option<u64>) -> Value {
    let started_at = epoch_ms();
    let timeout_seconds = resolve_timeout_seconds(timeout_seconds);
    let mut process = Command::new("sh");
    process.arg("-c").arg(command);
    if let Some(cwd) = cwd.filter(|value| !value.is_empty()) {
        process.current_dir(cwd);
    }
    process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Why: prechecks can spawn children; their own process group lets the
        // timeout path kill the whole tree instead of only the shell.
        process.process_group(0);
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => {
            return build_result(
                command,
                started_at,
                None,
                false,
                Some(error.to_string()),
                (String::new(), false),
                (String::new(), false),
            )
        }
    };
    let stdout_handle = child.stdout.take().map(spawn_reader);
    let stderr_handle = child.stderr.take().map(spawn_reader);

    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let mut timed_out = false;
    let mut wait_error = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                wait_error = Some(format!("precheck wait failed: {error}"));
                terminate_process_group(&mut child);
                break None;
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break terminate_process_group(&mut child);
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let (stdout, stdout_truncated) = join_reader(stdout_handle);
    let (stderr, stderr_truncated) = join_reader(stderr_handle);
    let error = if timed_out {
        Some(format!("Precheck timed out after {timeout_seconds}s."))
    } else {
        wait_error
    };
    build_result(
        command,
        started_at,
        status.and_then(|status| status.code()),
        timed_out,
        error,
        (stdout, stdout_truncated),
        (stderr, stderr_truncated),
    )
}

fn build_result(
    command: &str,
    started_at: u64,
    exit_code: Option<i32>,
    timed_out: bool,
    error: Option<String>,
    stdout: (String, bool),
    stderr: (String, bool),
) -> Value {
    let completed_at = epoch_ms();
    json!({
        "command": command,
        "exitCode": exit_code,
        "timedOut": timed_out,
        "durationMs": completed_at.saturating_sub(started_at),
        "stdout": stdout.0,
        "stderr": stderr.0,
        "stdoutTruncated": stdout.1,
        "stderrTruncated": stderr.1,
        "error": error,
        "startedAt": started_at,
        "completedAt": completed_at,
    })
}

/// SIGTERM the child's process group, give the whole group up to 2s to exit,
/// then SIGKILL the group before the caller joins the reader threads.
///
/// The group (not just the shell) must exit before skipping the SIGKILL: a
/// descendant that ignored SIGTERM can survive the shell's death while still
/// holding the stdio pipes, which would block the reader joins forever.
fn terminate_process_group(child: &mut Child) -> Option<ExitStatus> {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        signal_process_group(pid, libc::SIGTERM);
        let deadline = Instant::now() + FORCE_KILL_AFTER;
        while Instant::now() < deadline {
            // Reap the shell as soon as it exits so its zombie does not keep
            // the group probe alive.
            let _ = child.try_wait();
            if !process_group_alive(pid) {
                return child.wait().ok();
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        // Why: SIGKILL is uncatchable, so the remaining group members die and
        // release the stdio pipes, keeping the total timeout path bounded.
        signal_process_group(pid, libc::SIGKILL);
        child.wait().ok()
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let deadline = Instant::now() + FORCE_KILL_AFTER;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {}
                Err(_) => break,
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        child.wait().ok()
    }
}

#[cfg(unix)]
fn signal_process_group(pid: i32, signal: libc::c_int) {
    // Why: the child leads its own group (process_group(0)); signaling the
    // negative pid reaches shell grandchildren too.
    unsafe {
        libc::kill(-pid, signal);
    }
}

#[cfg(unix)]
fn process_group_alive(pid: i32) -> bool {
    // Signal 0 probes existence without delivering; ESRCH means no members.
    unsafe { libc::kill(-pid, 0) == 0 }
}

fn spawn_reader(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<(String, bool)> {
    std::thread::spawn(move || {
        let mut collector = TailCollector::default();
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => collector.push(&buffer[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        collector.finish()
    })
}

fn join_reader(handle: Option<std::thread::JoinHandle<(String, bool)>>) -> (String, bool) {
    match handle {
        Some(handle) => handle.join().unwrap_or_else(|_| (String::new(), false)),
        None => (String::new(), false),
    }
}

/// Bounded tail buffer: keeps at most `MAX_PRECHECK_BUFFER_BYTES` in memory
/// (dropping the oldest bytes) and reports the last `MAX_PRECHECK_OUTPUT_CHARS`
/// characters plus whether anything beyond that was ever seen.
#[derive(Default)]
struct TailCollector {
    bytes: Vec<u8>,
    dropped: bool,
}

impl TailCollector {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > MAX_PRECHECK_BUFFER_BYTES {
            let excess = self.bytes.len() - MAX_PRECHECK_BUFFER_BYTES;
            self.bytes.drain(..excess);
            self.dropped = true;
        }
    }

    fn finish(self) -> (String, bool) {
        // Decode once at the end so multi-byte sequences split across reads
        // stay intact.
        let decoded = String::from_utf8_lossy(&self.bytes);
        let truncated = self.dropped || decoded.chars().count() > MAX_PRECHECK_OUTPUT_CHARS;
        let tail: String = decoded
            .chars()
            .rev()
            .take(MAX_PRECHECK_OUTPUT_CHARS)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        (tail, truncated)
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ade-automations-precheck-{name}-{}-{unique}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn assert_process_dies(pid: i32) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "process {pid} survived the group kill"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn precheck_success_captures_stdout_and_exit_code() {
        let result = run_precheck("echo hello", None, None);
        assert_eq!(result["command"], "echo hello");
        assert_eq!(result["exitCode"], 0);
        assert_eq!(result["timedOut"], false);
        assert!(result["stdout"].as_str().unwrap().contains("hello"));
        assert_eq!(result["stderr"], "");
        assert_eq!(result["stdoutTruncated"], false);
        assert_eq!(result["stderrTruncated"], false);
        assert!(result["error"].is_null());
        let started_at = result["startedAt"].as_u64().unwrap();
        let completed_at = result["completedAt"].as_u64().unwrap();
        assert!(completed_at >= started_at);
        assert_eq!(
            result["durationMs"].as_u64().unwrap(),
            completed_at - started_at
        );
    }

    #[test]
    fn precheck_nonzero_exit_is_result_not_error() {
        let result = run_precheck("exit 3", None, None);
        assert_eq!(result["exitCode"], 3);
        assert_eq!(result["timedOut"], false);
        assert!(result["error"].is_null());
    }

    #[test]
    fn precheck_timeout_kills_process_group() {
        let dir = TestDir::new("timeout");
        let pid_file = dir.file("grandchild.pid");
        let command = format!("sleep 300 &\necho $! > '{}'\nsleep 300", pid_file.display());
        let started = Instant::now();
        let result = run_precheck(&command, None, Some(1));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must not wait for sleep 300"
        );
        assert_eq!(result["timedOut"], true);
        assert!(result["exitCode"].is_null());
        assert_eq!(result["error"], "Precheck timed out after 1s.");
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_process_dies(pid);
    }

    #[test]
    fn precheck_timeout_force_kills_sigterm_ignoring_process() {
        let dir = TestDir::new("sigterm-ignore");
        let pid_file = dir.file("shell.pid");
        let command = format!(
            "trap '' TERM; printf '%s' $$ > '{}'; sleep 300",
            pid_file.display()
        );
        let started = Instant::now();
        let result = run_precheck(&command, None, Some(1));
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(4),
            "timeout path must stay bounded, took {elapsed:?}"
        );
        assert_eq!(result["timedOut"], true);
        assert!(result["exitCode"].is_null());
        assert_eq!(result["error"], "Precheck timed out after 1s.");
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_process_dies(pid);
    }

    #[test]
    fn precheck_timeout_kills_sigterm_ignoring_grandchild_holding_pipes() {
        let dir = TestDir::new("stubborn-grandchild");
        let pid_file = dir.file("grandchild.pid");
        // Outer shell dies on SIGTERM while the background sh execs a `sleep`
        // that ignores SIGTERM and keeps the stdio pipes open; without the
        // unconditional group SIGKILL the reader joins would block until the
        // sleep exits (~300s).
        let command = format!(
            "sh -c \"trap '' TERM; printf '%s' \\$\\$ > '{}'; exec sleep 300\" &\nwait",
            pid_file.display()
        );
        let started = Instant::now();
        let result = run_precheck(&command, None, Some(1));
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(4),
            "timeout path must stay bounded, took {elapsed:?}"
        );
        assert_eq!(result["timedOut"], true);
        assert!(result["exitCode"].is_null());
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_process_dies(pid);
    }

    #[test]
    fn precheck_truncates_stdout_tail() {
        let result = run_precheck("printf '%*s' 4999 '' | tr ' ' 'a'; printf 'Z'", None, None);
        let stdout = result["stdout"].as_str().unwrap();
        assert_eq!(stdout.chars().count(), MAX_PRECHECK_OUTPUT_CHARS);
        assert!(stdout.starts_with('a'));
        assert!(stdout.ends_with('Z'));
        assert_eq!(result["stdoutTruncated"], true);
        assert_eq!(result["stderrTruncated"], false);
        assert_eq!(result["exitCode"], 0);
    }

    #[test]
    fn precheck_truncates_stderr_tail() {
        let result = run_precheck("printf '%*s' 5000 '' | tr ' ' 'b' >&2", None, None);
        let stderr = result["stderr"].as_str().unwrap();
        assert_eq!(stderr.chars().count(), MAX_PRECHECK_OUTPUT_CHARS);
        assert!(stderr.chars().all(|value| value == 'b'));
        assert_eq!(result["stderrTruncated"], true);
        assert_eq!(result["stdoutTruncated"], false);
    }

    #[test]
    fn precheck_flooding_output_returns_promptly_with_tail_suffix() {
        let started = Instant::now();
        let result = run_precheck(
            "printf '%*s' 3000000 '' | tr ' ' 'x'; printf 'END-MARKER'; sleep 0.5",
            None,
            None,
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "must not deadlock on >1MiB output"
        );
        assert_eq!(result["exitCode"], 0);
        assert_eq!(result["stdoutTruncated"], true);
        let stdout = result["stdout"].as_str().unwrap();
        assert_eq!(stdout.chars().count(), MAX_PRECHECK_OUTPUT_CHARS);
        assert!(stdout.ends_with("END-MARKER"));
        assert!(stdout
            .trim_end_matches("END-MARKER")
            .chars()
            .all(|value| value == 'x'));
    }

    #[test]
    fn precheck_runs_in_requested_cwd() {
        let dir = TestDir::new("cwd");
        let result = run_precheck("pwd -P", Some(dir.path.to_str().unwrap()), None);
        assert_eq!(result["exitCode"], 0);
        let expected = dir.path.canonicalize().unwrap();
        assert_eq!(
            result["stdout"].as_str().unwrap().trim(),
            expected.to_str().unwrap()
        );
    }

    #[test]
    fn precheck_spawn_failure_reports_error() {
        let dir = TestDir::new("spawn-error");
        let missing = dir.file("missing").join("nested");
        let result = run_precheck("echo nope", Some(missing.to_str().unwrap()), None);
        assert!(result["error"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert!(result["exitCode"].is_null());
        assert_eq!(result["timedOut"], false);
    }

    #[test]
    fn resolve_timeout_seconds_defaults_and_clamps() {
        assert_eq!(
            resolve_timeout_seconds(None),
            DEFAULT_PRECHECK_TIMEOUT_SECONDS
        );
        assert_eq!(resolve_timeout_seconds(None), 60);
        assert_eq!(
            resolve_timeout_seconds(Some(9999)),
            MAX_PRECHECK_TIMEOUT_SECONDS
        );
        assert_eq!(resolve_timeout_seconds(Some(9999)), 600);
        assert_eq!(resolve_timeout_seconds(Some(30)), 30);
        assert_eq!(resolve_timeout_seconds(Some(0)), 1);
    }

    #[test]
    fn tail_collector_joins_utf8_sequences_split_across_chunks() {
        let mut collector = TailCollector::default();
        let bytes = "你好世界".as_bytes();
        let (head, tail) = bytes.split_at(5);
        collector.push(head);
        collector.push(tail);
        let (content, truncated) = collector.finish();
        assert_eq!(content, "你好世界");
        assert!(!truncated);
    }

    #[test]
    fn tail_collector_keeps_tail_beyond_buffer_cap() {
        let mut collector = TailCollector::default();
        collector.push(&vec![b'a'; MAX_PRECHECK_BUFFER_BYTES]);
        collector.push(b"tail-marker");
        let (content, truncated) = collector.finish();
        assert!(truncated);
        assert!(content.ends_with("tail-marker"));
        assert_eq!(content.chars().count(), MAX_PRECHECK_OUTPUT_CHARS);
    }

    #[test]
    fn tail_collector_does_not_flag_exact_char_limit() {
        let mut collector = TailCollector::default();
        collector.push(&vec![b'x'; MAX_PRECHECK_OUTPUT_CHARS]);
        let (content, truncated) = collector.finish();
        assert!(!truncated);
        assert_eq!(content.chars().count(), MAX_PRECHECK_OUTPUT_CHARS);
    }
}
