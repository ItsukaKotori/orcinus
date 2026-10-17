//! `orcinus` CLI anchors: status (metadata-absent / unreadable / readable
//! states; JSON + human shapes) and open (launch via the env chain, JSON +
//! human shapes). Every test spawns the compiled binary with
//! `env!("CARGO_BIN_EXE_orcinus")`. The runtime RPC was removed with the
//! computer-use trim, so no fake runtime server is involved.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_orcinus");

struct CliOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

impl CliOutput {
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim()).unwrap_or_else(|error| {
            panic!(
                "stdout is not JSON ({error}); stdout={:?} stderr={:?}",
                self.stdout, self.stderr
            )
        })
    }

    fn exit_code(&self) -> i32 {
        self.status.code().expect("process exited with a code")
    }
}

fn run(user_data_dir: &Path, args: &[&str]) -> CliOutput {
    run_with_env(user_data_dir, args, &[])
}

fn run_with_env(user_data_dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> CliOutput {
    let mut command = Command::new(BIN);
    command
        .env("ORCINUS_USER_DATA_PATH", user_data_dir)
        .env_remove("ORCINUS_OPEN_COMMAND")
        .env_remove("ORCINUS_APP_EXECUTABLE");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output: Output = command.output().expect("run orcinus");
    CliOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Temp user-data dir with cleanup.
struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "orcinus-cli-empty-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create temp user data dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// status

#[test]
fn status_json_without_metadata_reports_not_running_and_exits_zero() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["status", "--json"]);

    assert_eq!(output.exit_code(), 0, "stdout={:?}", output.stdout);
    let envelope = output.json();
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["result"]["runtime"]["state"], "not_running");
    assert_eq!(envelope["result"]["runtime"]["reachable"], false);
    assert_eq!(envelope["result"]["runtime"]["runtimeId"], Value::Null);
    assert_eq!(envelope["_meta"]["runtimeId"], "none");
}

#[test]
fn status_json_with_unreadable_metadata_reports_stale_bootstrap() {
    let dir = TestDir::new();
    std::fs::write(dir.path().join("runtime.json"), b"{ this is not json")
        .expect("write broken metadata");

    let output = run(dir.path(), &["status", "--json"]);
    assert_eq!(output.exit_code(), 0);
    let envelope = output.json();
    assert_eq!(envelope["result"]["runtime"]["state"], "stale_bootstrap");
    assert_eq!(envelope["result"]["runtime"]["reachable"], false);
    assert_eq!(envelope["result"]["runtime"]["runtimeId"], Value::Null);
}

#[test]
fn status_json_with_readable_metadata_reports_stale_bootstrap_and_id() {
    let dir = TestDir::new();
    std::fs::write(
        dir.path().join("runtime.json"),
        br#"{"runtimeId":"RID","pid":1,"transports":[{"kind":"unix","endpoint":"/tmp/x.sock"}],"authToken":"t","startedAt":"2026-01-01T00:00:00.000Z"}"#,
    )
    .expect("write metadata");

    let output = run(dir.path(), &["status", "--json"]);
    assert_eq!(output.exit_code(), 0);
    let envelope = output.json();
    assert_eq!(envelope["result"]["runtime"]["state"], "stale_bootstrap");
    assert_eq!(envelope["result"]["runtime"]["reachable"], false);
    assert_eq!(envelope["result"]["runtime"]["runtimeId"], "RID");
    assert_eq!(envelope["_meta"]["runtimeId"], "RID");
}

#[test]
fn status_human_prints_metadata_state_lines() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["status"]);

    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    assert!(output.stdout.contains("runtimeState: not_running"));
    assert!(output.stdout.contains("runtimeReachable: false"));
    assert!(output.stdout.contains("runtimeId: none"));
}

// ---------------------------------------------------------------------------
// open

#[test]
fn open_human_launches_without_waiting_for_a_runtime() {
    let dir = TestDir::new();
    let output = run_with_env(dir.path(), &["open"], &[("ORCINUS_OPEN_COMMAND", "true")]);

    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    assert!(output.stdout.contains("Orcinus launched."));
    assert!(output.stderr.is_empty(), "stderr={:?}", output.stderr);
}

#[test]
fn open_json_returns_the_launched_envelope() {
    let dir = TestDir::new();
    let output = run_with_env(
        dir.path(),
        &["open", "--json"],
        &[("ORCINUS_OPEN_COMMAND", "true")],
    );

    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let envelope = output.json();
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["result"]["launched"], true);
    assert_eq!(envelope["_meta"]["runtimeId"], "none");
}

#[test]
fn open_json_spawn_failure_exits_one_with_runtime_open_failed() {
    let dir = TestDir::new();
    let output = run_with_env(
        dir.path(),
        &["open", "--json"],
        &[("ORCINUS_APP_EXECUTABLE", "/definitely/not/a/real/orcinus-test-binary")],
    );

    assert_eq!(output.exit_code(), 1);
    let envelope = output.json();
    assert_eq!(envelope["id"], "local");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "runtime_open_failed");
}

// ---------------------------------------------------------------------------
// help / version

#[test]
fn help_and_version_flags_exist() {
    let dir = TestDir::new();

    let help = run(dir.path(), &["--help"]);
    assert_eq!(help.exit_code(), 0);
    for expected in ["status", "open"] {
        assert!(help.stdout.contains(expected), "--help missing {expected}");
    }
    // Why: nothing serves the runtime RPC anymore; help must not promise
    // reachability/observation the CLI cannot perform.
    assert!(!help.stdout.to_lowercase().contains("reachable"));

    let version = run(dir.path(), &["--version"]);
    assert_eq!(version.exit_code(), 0);
    assert!(version.stdout.contains("orcinus"), "{}", version.stdout);
}
