//! `orcinus` CLI anchors (task brief step 1): status (not running / stale /
//! reachable / timeout / human exit codes), open (launch + poll success, launch
//! timeout) and `skills get` (JSON shape, `--full` equivalence, unknown topic).
//! Every test spawns the compiled binary with `env!("CARGO_BIN_EXE_orcinus")`
//! and drives it against the fake runtime server in `tests/common`; the real
//! app is never launched.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use common::{FakeRuntime, AUTH_TOKEN, RUNTIME_ID};
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_orcinus");
const REQUEST_WAIT: Duration = Duration::from_secs(5);

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
    command.env("ORCINUS_USER_DATA_PATH", user_data_dir);
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
    assert_eq!(envelope["result"]["app"]["running"], false);
    assert_eq!(envelope["result"]["runtime"]["state"], "not_running");
    assert_eq!(envelope["result"]["runtime"]["reachable"], false);
    assert_eq!(envelope["result"]["runtime"]["runtimeId"], Value::Null);
}

#[test]
fn status_json_with_unreadable_metadata_reports_stale_bootstrap() {
    let dir = TestDir::new();
    std::fs::write(dir.path().join("runtime.json"), b"{ this is not json")
        .expect("write broken metadata");

    let output = run(dir.path(), &["status", "--json"]);
    assert_eq!(output.exit_code(), 0);
    let envelope = output.json();
    assert_eq!(envelope["result"]["app"]["running"], false);
    assert_eq!(envelope["result"]["runtime"]["state"], "stale_bootstrap");
    assert_eq!(envelope["result"]["runtime"]["reachable"], false);
}

#[test]
fn status_json_against_running_runtime_projects_status_get() {
    let runtime = FakeRuntime::start();
    let output = run(runtime.user_data_path(), &["status", "--json"]);

    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let envelope = output.json();
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["_meta"]["runtimeId"], RUNTIME_ID);
    assert_eq!(envelope["result"]["app"]["running"], true);
    assert_eq!(envelope["result"]["runtime"]["reachable"], true);
    assert_eq!(envelope["result"]["runtime"]["state"], "ready");
    assert_eq!(envelope["result"]["runtime"]["runtimeId"], RUNTIME_ID);
    assert_eq!(
        envelope["result"]["runtime"]["appVersion"],
        common::APP_VERSION
    );
    assert_eq!(envelope["result"]["graph"]["state"], "not_ready");

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "status.get");
    assert_eq!(request.auth_token.as_deref(), Some(AUTH_TOKEN));
}

#[test]
fn status_human_without_metadata_prints_state_and_exits_one() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["status"]);

    assert_eq!(output.exit_code(), 1, "stdout={:?}", output.stdout);
    assert!(
        output.stdout.contains("runtimeState: not_running"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("runtimeReachable: false"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("appRunning: false"),
        "{}",
        output.stdout
    );
}

#[test]
fn status_human_against_running_runtime_exits_zero() {
    let runtime = FakeRuntime::start();
    let output = run(runtime.user_data_path(), &["status"]);

    assert_eq!(output.exit_code(), 0, "stdout={:?}", output.stdout);
    assert!(
        output.stdout.contains("runtimeReachable: true"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains(&format!("runtimeId: {RUNTIME_ID}")),
        "{}",
        output.stdout
    );
}

#[test]
fn status_times_out_when_status_get_does_not_answer() {
    let runtime = FakeRuntime::start();
    runtime.set_response_delay(Duration::from_secs(3));

    let started = std::time::Instant::now();
    let output = run(runtime.user_data_path(), &["status", "--json"]);
    let elapsed = started.elapsed();

    assert_eq!(
        output.exit_code(),
        0,
        "a timeout still synthesizes a status"
    );
    let envelope = output.json();
    assert_eq!(envelope["result"]["runtime"]["reachable"], false);
    assert!(
        elapsed < Duration::from_millis(2500),
        "1s status timeout must fire before the 3s server delay: {elapsed:?}"
    );
}

#[test]
fn status_answers_from_metadata_even_when_runtime_is_absent() {
    let runtime = FakeRuntime::start();
    let metadata = std::fs::read_to_string(runtime.metadata_path()).expect("read metadata");
    drop(runtime);

    let dir = TestDir::new();
    std::fs::write(dir.path().join("runtime.json"), metadata).expect("write metadata");

    let output = run(dir.path(), &["status", "--json"]);
    assert_eq!(output.exit_code(), 0);
    let envelope = output.json();
    assert_eq!(envelope["result"]["app"]["running"], false);
    assert_eq!(envelope["result"]["runtime"]["reachable"], false);
    assert_eq!(envelope["result"]["runtime"]["state"], "stale_bootstrap");
    assert_eq!(envelope["result"]["runtime"]["runtimeId"], Value::Null);
}

// ---------------------------------------------------------------------------
// open

#[test]
fn open_json_returns_existing_runtime_without_relaunching() {
    let runtime = FakeRuntime::start();
    let output = run(runtime.user_data_path(), &["open", "--json"]);

    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let envelope = output.json();
    assert_eq!(envelope["result"]["app"]["running"], true);
    assert_eq!(envelope["result"]["runtime"]["reachable"], true);
    assert_eq!(envelope["_meta"]["runtimeId"], RUNTIME_ID);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "status.get");
}

#[test]
fn open_human_without_a_runtime_times_out_with_runtime_open_timeout() {
    let dir = TestDir::new();
    let output = run_with_env(dir.path(), &["open"], &[("ORCINUS_OPEN_COMMAND", "true")]);

    assert_eq!(output.exit_code(), 1, "stdout={:?}", output.stdout);
    assert!(
        output.stderr.contains("runtime_open_timeout") || output.stderr.contains("Timed out"),
        "stderr={:?}",
        output.stderr
    );
    assert!(
        output.stderr.contains("15") || output.stderr.contains("open"),
        "stderr={:?}",
        output.stderr
    );
}

#[test]
fn open_json_without_a_runtime_times_out_with_runtime_open_timeout_envelope() {
    let dir = TestDir::new();
    let output = run_with_env(
        dir.path(),
        &["open", "--json"],
        &[("ORCINUS_OPEN_COMMAND", "true")],
    );

    assert_eq!(output.exit_code(), 1);
    let envelope = output.json();
    assert_eq!(envelope["id"], "local");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "runtime_open_timeout");
}

// ---------------------------------------------------------------------------
// skills get

#[test]
fn skills_get_computer_use_json_returns_the_embedded_guide() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["skills", "get", "computer-use", "--json"]);

    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let envelope = output.json();
    assert_eq!(envelope["ok"], true);
    let result = &envelope["result"];
    assert_eq!(result["name"], "computer-use");
    assert_eq!(result["full"], true);
    let markdown = result["markdown"].as_str().expect("markdown string");
    assert!(!markdown.trim().is_empty(), "guide must be embedded");
    assert!(
        markdown.contains("orcinus"),
        "guide must be adapted to orcinus"
    );
    assert!(
        !markdown.contains("ORCA "),
        "ORCA placeholder must be replaced"
    );
    assert!(
        markdown.contains("computer list-apps"),
        "guide body expected"
    );
}

#[test]
fn skills_get_human_prints_the_guide_markdown() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["skills", "get", "computer-use"]);

    assert_eq!(output.exit_code(), 0);
    assert!(
        output.stdout.contains("# Computer Use"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("orcinus"), "{}", output.stdout);
}

#[test]
fn skills_get_full_flag_is_equivalent_to_the_default() {
    let dir = TestDir::new();
    let default = run(dir.path(), &["skills", "get", "computer-use", "--json"]);
    let full = run(
        dir.path(),
        &["skills", "get", "computer-use", "--full", "--json"],
    );

    assert_eq!(default.exit_code(), 0);
    assert_eq!(full.exit_code(), 0);
    assert_eq!(default.json()["result"], full.json()["result"]);
    assert_eq!(full.json()["result"]["full"], true);
}

#[test]
fn skills_get_unknown_topic_is_invalid_argument_with_available_topics() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["skills", "get", "browsing", "--json"]);

    assert_eq!(output.exit_code(), 1);
    let envelope = output.json();
    assert_eq!(envelope["id"], "local");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    let message = envelope["error"]["message"].as_str().expect("message");
    assert!(message.contains("browsing"), "message={message}");
    assert!(message.contains("computer-use"), "message={message}");
}

#[test]
fn skills_get_unknown_topic_human_exits_one_with_readable_error() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["skills", "get", "browsing"]);

    assert_eq!(output.exit_code(), 1);
    assert!(
        output.stderr.contains("computer-use"),
        "stderr={:?}",
        output.stderr
    );
    assert!(output.stdout.is_empty(), "stdout={:?}", output.stdout);
}

// ---------------------------------------------------------------------------
// connection failures

#[test]
fn wrong_auth_token_maps_to_runtime_unavailable() {
    let runtime = FakeRuntime::start();
    runtime.set_metadata_auth_token("not-the-token");

    // `open` requires the runtime, so a rejected token surfaces instead of
    // being masked by the unreachable snapshot `status` synthesizes.
    let output = run_with_env(
        runtime.user_data_path(),
        &["open", "--json"],
        &[("ORCINUS_OPEN_COMMAND", "true")],
    );
    assert_eq!(output.exit_code(), 1);
    let envelope = output.json();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "runtime_unavailable");

    let output = run(runtime.user_data_path(), &["status", "--json"]);
    assert_eq!(
        output.exit_code(),
        0,
        "status never fails hard on transport errors"
    );
    assert_eq!(output.json()["result"]["runtime"]["reachable"], false);
    assert_eq!(
        output.json()["result"]["runtime"]["state"],
        "stale_bootstrap"
    );
}

#[test]
fn help_and_version_flags_exist() {
    let dir = TestDir::new();

    let help = run(dir.path(), &["--help"]);
    assert_eq!(help.exit_code(), 0);
    for expected in ["status", "open", "skills"] {
        assert!(help.stdout.contains(expected), "--help missing {expected}");
    }

    let version = run(dir.path(), &["--version"]);
    assert_eq!(version.exit_code(), 0);
    assert!(version.stdout.contains("orcinus"), "{}", version.stdout);
}
