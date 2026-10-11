//! Task 12 anchors for `orcinus computer *` (brief step 1): one legal call per
//! subcommand with method/params assertions and verbatim response passthrough,
//! the flag XOR matrix with oracle-verbatim messages and exit 1, `--json`
//! screenshot spillover (0600, field replacement, stale cleanup, inline
//! fallback on write failure) and RPC error passthrough. Every test spawns the
//! compiled binary and drives the scripted fake runtime in `tests/common`; the
//! real app and helper are never launched.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{FakeRuntime, RUNTIME_ID};
use serde_json::{json, Value};

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

    fn error_message(&self) -> String {
        let envelope = self.json();
        assert_eq!(envelope["ok"], false, "expected a failure envelope");
        assert_eq!(envelope["id"], "local");
        assert_eq!(envelope["error"]["code"], "invalid_argument");
        envelope["error"]["message"]
            .as_str()
            .expect("message")
            .to_string()
    }
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

fn run(user_data_dir: &Path, args: &[&str]) -> CliOutput {
    run_with_env(user_data_dir, args, &[])
}

/// Temp dir with cleanup for cases that need no runtime (validation, tempdirs).
struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "orcinus-cli-computer-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
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

fn assert_runtime_envelope(output: &CliOutput, expected_result: &Value) {
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let envelope = output.json();
    assert_eq!(envelope["ok"], true, "envelope={envelope}");
    assert_eq!(envelope["_meta"]["runtimeId"], RUNTIME_ID);
    assert_eq!(envelope["result"], *expected_result);
}

fn snapshot_result() -> Value {
    json!({
        "snapshot": {
            "app": {"name": "Finder", "pid": 123, "bundleId": "com.apple.finder"},
            "window": {
                "id": 7,
                "index": 0,
                "title": "Desktop",
                "width": 800,
                "height": 600,
                "x": 0,
                "y": 0,
            },
            "focusedElementId": null,
            "elementCount": 12,
            "coordinateSpace": "screen",
            "treeText": "- window \"Desktop\"\n  - button \"OK\"",
        },
        "screenshotStatus": {"state": "skipped"},
    })
}

fn action_result() -> Value {
    json!({
        "action": {
            "path": "synthetic",
            "verification": {"state": "unverified", "reason": "synthetic_input"},
        },
        "snapshot": {
            "app": {"name": "Finder", "pid": 123, "bundleId": "com.apple.finder"},
            "window": {"id": 7, "index": 0, "title": "Desktop"},
            "elementCount": 5,
        },
        "screenshotStatus": {"state": "skipped"},
    })
}

// ---------------------------------------------------------------------------
// happy path: 14 subcommands, method + params, verbatim result passthrough

#[test]
fn capabilities_calls_computer_capabilities_with_empty_params() {
    let runtime = FakeRuntime::start();
    let result = json!({
        "provider": "macos",
        "platform": "darwin",
        "protocolVersion": 1,
        "supports": {
            "apps": {"list": true, "bundleIds": true, "pids": true},
            "windows": {"list": true, "targetById": true, "targetByIndex": true},
            "observation": {"screenshot": true, "elementFrames": true, "annotatedScreenshot": false},
            "actions": {"click": true, "scroll": true},
        },
    });
    runtime.set_method_result("computer.capabilities", result.clone());

    let output = run(
        runtime.user_data_path(),
        &["computer", "capabilities", "--json"],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.capabilities");
    assert_eq!(request.params, json!({}));
}

#[test]
fn list_apps_calls_computer_list_apps_with_empty_params() {
    let runtime = FakeRuntime::start();
    let result = json!({"apps": [{"name": "Finder", "pid": 1, "bundleId": "com.apple.finder"}]});
    runtime.set_method_result("computer.listApps", result.clone());

    let output = run(
        runtime.user_data_path(),
        &["computer", "list-apps", "--json"],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.listApps");
    assert_eq!(request.params, json!({}));
}

#[test]
fn permissions_passes_the_requested_id_through() {
    let runtime = FakeRuntime::start();
    let result = json!({
        "platform": "darwin",
        "helperAppPath": "/Applications/Orcinus Computer Use.app",
        "openedSettings": true,
        "launchedHelper": true,
        "permissionId": "screenshots",
    });
    runtime.set_method_result("computer.permissions", result.clone());

    let output = run(
        runtime.user_data_path(),
        &["computer", "permissions", "--id", "screenshots", "--json"],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.permissions");
    assert_eq!(request.params, json!({"id": "screenshots"}));
}

#[test]
fn permissions_without_id_sends_empty_params() {
    let runtime = FakeRuntime::start();
    let result = json!({
        "platform": "darwin",
        "helperAppPath": null,
        "openedSettings": false,
        "launchedHelper": false,
    });
    runtime.set_method_result("computer.permissions", result.clone());

    let output = run(
        runtime.user_data_path(),
        &["computer", "permissions", "--json"],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.permissions");
    assert_eq!(request.params, json!({}));
}

#[test]
fn list_windows_calls_computer_list_windows_with_app() {
    let runtime = FakeRuntime::start();
    let result = json!({
        "app": {"name": "Finder", "bundleId": "com.apple.finder"},
        "windows": [{
            "id": 7, "index": 0, "title": "Desktop", "width": 800, "height": 600,
            "x": 0, "y": 0, "screenIndex": 0, "isMinimized": false, "isOffscreen": false,
        }],
    });
    runtime.set_method_result("computer.listWindows", result.clone());

    let output = run(
        runtime.user_data_path(),
        &["computer", "list-windows", "--app", "Finder", "--json"],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.listWindows");
    assert_eq!(request.params, json!({"app": "Finder"}));
}

#[test]
fn get_app_state_assembles_target_and_observe_flags() {
    let runtime = FakeRuntime::start();
    let result = snapshot_result();
    runtime.set_method_result("computer.getAppState", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "get-app-state",
            "--app",
            "Finder",
            "--session",
            "ses-1",
            "--window-index",
            "1",
            "--restore-window",
            "--no-screenshot",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.getAppState");
    assert_eq!(
        request.params,
        json!({
            "app": "Finder",
            "session": "ses-1",
            "windowIndex": 1,
            "restoreWindow": true,
            "noScreenshot": true,
        })
    );
}

#[test]
fn click_assembles_action_target_and_observe_flags() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.click", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "click",
            "--app",
            "Finder",
            "--element-index",
            "3",
            "--click-count",
            "2",
            "--mouse-button",
            "right",
            "--modifiers",
            "CmdOrCtrl+Shift",
            "--window-id",
            "9",
            "--worktree",
            "main",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.click");
    assert_eq!(
        request.params,
        json!({
            "app": "Finder",
            "worktree": "main",
            "windowId": 9,
            "elementIndex": 3,
            "clickCount": 2,
            "mouseButton": "right",
            "modifiers": "CmdOrCtrl+Shift",
        })
    );
}

#[test]
fn click_coordinates_are_passed_as_numbers() {
    let runtime = FakeRuntime::start();
    runtime.set_method_result("computer.click", action_result());

    run(
        runtime.user_data_path(),
        &[
            "computer", "click", "--app", "X", "--x", "12.5", "--y", "30", "--json",
        ],
    );

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.params, json!({"app": "X", "x": 12.5, "y": 30}));
}

#[test]
fn perform_secondary_action_sends_element_index_and_action() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.performSecondaryAction", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "perform-secondary-action",
            "--app",
            "Notes",
            "--element-index",
            "5",
            "--action",
            "AXPress",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.performSecondaryAction");
    assert_eq!(
        request.params,
        json!({"app": "Notes", "elementIndex": 5, "action": "AXPress"})
    );
}

#[test]
fn scroll_sends_element_direction_and_pages() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.scroll", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "scroll",
            "--app",
            "Safari",
            "--x",
            "10",
            "--y",
            "20",
            "--direction",
            "down",
            "--pages",
            "1.5",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.scroll");
    assert_eq!(
        request.params,
        json!({"app": "Safari", "x": 10, "y": 20, "direction": "down", "pages": 1.5})
    );
}

#[test]
fn drag_sends_the_element_pair() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.drag", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "drag",
            "--app",
            "Finder",
            "--from-element-index",
            "1",
            "--to-element-index",
            "2",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.drag");
    assert_eq!(
        request.params,
        json!({"app": "Finder", "fromElementIndex": 1, "toElementIndex": 2})
    );
}

#[test]
fn drag_sends_all_four_coordinates() {
    let runtime = FakeRuntime::start();
    runtime.set_method_result("computer.drag", action_result());

    run(
        runtime.user_data_path(),
        &[
            "computer", "drag", "--app", "X", "--from-x", "1", "--from-y", "2", "--to-x", "3.5",
            "--to-y", "4", "--json",
        ],
    );

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(
        request.params,
        json!({"app": "X", "fromX": 1, "fromY": 2, "toX": 3.5, "toY": 4})
    );
}

#[test]
fn type_text_sends_the_text() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.typeText", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "type-text",
            "--app",
            "TextEdit",
            "--text",
            "hello world",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.typeText");
    assert_eq!(
        request.params,
        json!({"app": "TextEdit", "text": "hello world"})
    );
}

#[test]
fn press_key_sends_the_key() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.pressKey", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "press-key",
            "--app",
            "X",
            "--key",
            "Return",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.pressKey");
    assert_eq!(request.params, json!({"app": "X", "key": "Return"}));
}

#[test]
fn hotkey_sends_the_combination() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.hotkey", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "hotkey",
            "--app",
            "X",
            "--key",
            "CmdOrCtrl+A",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.hotkey");
    assert_eq!(request.params, json!({"app": "X", "key": "CmdOrCtrl+A"}));
}

#[test]
fn paste_text_sends_the_text() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.pasteText", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "paste-text",
            "--app",
            "X",
            "--text",
            "pasted",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.pasteText");
    assert_eq!(request.params, json!({"app": "X", "text": "pasted"}));
}

#[test]
fn set_value_sends_element_index_and_allows_an_empty_value() {
    let runtime = FakeRuntime::start();
    let result = action_result();
    runtime.set_method_result("computer.setValue", result.clone());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "set-value",
            "--app",
            "X",
            "--element-index",
            "0",
            "--value",
            "",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(request.method, "computer.setValue");
    assert_eq!(
        request.params,
        json!({"app": "X", "elementIndex": 0, "value": ""})
    );
}

// ---------------------------------------------------------------------------
// flag XOR matrix and oracle validation messages (exit 1, invalid_argument)

fn assert_invalid(user_data_dir: &Path, args: &[&str], expected_message: &str) {
    let output = run(user_data_dir, args);
    assert_eq!(
        output.exit_code(),
        1,
        "args={args:?} stdout={:?} stderr={:?}",
        output.stdout,
        output.stderr
    );
    assert_eq!(output.error_message(), expected_message, "args={args:?}");
}

#[test]
fn click_requires_element_index_or_both_coordinates() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &["computer", "click", "--app", "X", "--json"],
        "Click requires --element-index or both --x and --y",
    );
    assert_invalid(
        dir.path(),
        &["computer", "click", "--app", "X", "--x", "1", "--json"],
        "Click requires --element-index or both --x and --y",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "click",
            "--app",
            "X",
            "--element-index",
            "0",
            "--x",
            "1",
            "--json",
        ],
        "Click coordinates require both --x and --y",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "click",
            "--app",
            "X",
            "--element-index",
            "0",
            "--x",
            "1",
            "--y",
            "2",
            "--json",
        ],
        "Click accepts either --element-index or coordinate flags, not both",
    );
}

#[test]
fn click_rejects_invalid_mouse_button_and_modifiers() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "click",
            "--app",
            "X",
            "--element-index",
            "0",
            "--mouse-button",
            "back",
            "--json",
        ],
        "Unsupported --mouse-button; expected left, right, or middle",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "click",
            "--app",
            "X",
            "--element-index",
            "0",
            "--modifiers",
            "Cmd+A",
            "--json",
        ],
        "Click modifiers accept modifier keys only, e.g. CmdOrCtrl or CmdOrCtrl+Shift.",
    );
}

#[test]
fn scroll_flag_matrix_reports_oracle_messages() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "scroll",
            "--app",
            "X",
            "--direction",
            "up",
            "--json",
        ],
        "Scroll requires --element-index or both --x and --y",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "scroll",
            "--app",
            "X",
            "--y",
            "1",
            "--direction",
            "up",
            "--json",
        ],
        "Scroll requires --element-index or both --x and --y",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "scroll",
            "--app",
            "X",
            "--element-index",
            "0",
            "--y",
            "1",
            "--direction",
            "up",
            "--json",
        ],
        "Scroll coordinates require both --x and --y",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "scroll",
            "--app",
            "X",
            "--element-index",
            "0",
            "--x",
            "1",
            "--y",
            "2",
            "--direction",
            "up",
            "--json",
        ],
        "Scroll accepts either --element-index or coordinate flags, not both",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "scroll",
            "--app",
            "X",
            "--element-index",
            "0",
            "--direction",
            "diagonal",
            "--json",
        ],
        "Unsupported --direction; expected up, down, left, or right",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "scroll",
            "--app",
            "X",
            "--element-index",
            "0",
            "--direction",
            "up",
            "--pages",
            "0",
            "--json",
        ],
        "Invalid positive number for --pages",
    );
}

#[test]
fn drag_flag_matrix_reports_oracle_messages() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &["computer", "drag", "--app", "X", "--json"],
        "Drag requires --from-element-index and --to-element-index, or all coordinate flags",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "drag",
            "--app",
            "X",
            "--from-element-index",
            "0",
            "--json",
        ],
        "Drag element targeting requires both --from-element-index and --to-element-index",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer", "drag", "--app", "X", "--from-x", "1", "--from-y", "2", "--json",
        ],
        "Drag coordinates require --from-x, --from-y, --to-x, and --to-y",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "drag",
            "--app",
            "X",
            "--from-element-index",
            "0",
            "--to-element-index",
            "1",
            "--from-x",
            "1",
            "--from-y",
            "2",
            "--to-x",
            "3",
            "--to-y",
            "4",
            "--json",
        ],
        "Drag accepts either element indexes or coordinate flags, not both",
    );
}

#[test]
fn window_and_session_targets_are_exclusive() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "get-app-state",
            "--app",
            "X",
            "--window-id",
            "1",
            "--window-index",
            "0",
            "--json",
        ],
        "Window targeting accepts either --window-id or --window-index, not both",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "get-app-state",
            "--app",
            "X",
            "--session",
            "a",
            "--worktree",
            "b",
            "--json",
        ],
        "Computer-use targeting accepts either --session or --worktree, not both",
    );
}

#[test]
fn type_text_and_paste_text_reject_conflicting_payload_flags() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "type-text",
            "--app",
            "X",
            "--text",
            "hi",
            "--text-stdin",
            "--json",
        ],
        "Use either --text or --text-stdin, not both",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "paste-text",
            "--app",
            "X",
            "--text",
            "hi",
            "--text-stdin",
            "--json",
        ],
        "Use either --text or --text-stdin, not both",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "set-value",
            "--app",
            "X",
            "--element-index",
            "0",
            "--value",
            "v",
            "--value-stdin",
            "--json",
        ],
        "Use either --value or --value-stdin, not both",
    );
}

#[test]
fn empty_stdin_payload_is_rejected_for_text_but_allowed_for_value() {
    let runtime = FakeRuntime::start();
    let dir = TestDir::new();

    let output = run(
        dir.path(),
        &[
            "computer",
            "type-text",
            "--app",
            "X",
            "--text-stdin",
            "--json",
        ],
    );
    assert_eq!(output.exit_code(), 1);
    assert_eq!(output.error_message(), "Missing text from stdin");

    let result = action_result();
    runtime.set_method_result("computer.setValue", result.clone());
    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "set-value",
            "--app",
            "X",
            "--element-index",
            "0",
            "--value-stdin",
            "--json",
        ],
    );
    assert_runtime_envelope(&output, &result);

    let request = runtime.next_request(REQUEST_WAIT);
    assert_eq!(
        request.params,
        json!({"app": "X", "elementIndex": 0, "value": ""})
    );
}

#[test]
fn key_specs_match_the_oracle_hints() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &["computer", "press-key", "--app", "X", "--key", "Cmd+A", "--json"],
        "Press-key accepts one key only, e.g. Return, Escape, Tab, or +. Use hotkey for modifier combinations.",
    );
    assert_invalid(
        dir.path(),
        &["computer", "hotkey", "--app", "X", "--key", "A", "--json"],
        "Hotkey requires a modifier and one key, e.g. CmdOrCtrl+A. Use press-key for a single key.",
    );
}

#[test]
fn missing_required_flags_report_oracle_messages() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &["computer", "list-windows", "--json"],
        "Missing required --app",
    );
    assert_invalid(
        dir.path(),
        &["computer", "click", "--app", "X", "--json"],
        "Click requires --element-index or both --x and --y",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "perform-secondary-action",
            "--app",
            "X",
            "--action",
            "AXPress",
            "--json",
        ],
        "Missing required --element-index",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "perform-secondary-action",
            "--app",
            "X",
            "--element-index",
            "0",
            "--json",
        ],
        "Missing required --action",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "scroll",
            "--app",
            "X",
            "--element-index",
            "0",
            "--json",
        ],
        "Missing required --direction",
    );
    assert_invalid(
        dir.path(),
        &["computer", "press-key", "--app", "X", "--json"],
        "Missing required --key",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "set-value",
            "--app",
            "X",
            "--element-index",
            "0",
            "--json",
        ],
        "Missing required --value",
    );
}

#[test]
fn numeric_flag_errors_match_the_oracle_text() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "get-app-state",
            "--app",
            "X",
            "--window-id",
            "abc",
            "--json",
        ],
        "Invalid numeric value for --window-id",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "click",
            "--app",
            "X",
            "--element-index",
            "-1",
            "--json",
        ],
        "Invalid non-negative integer for --element-index",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "click",
            "--app",
            "X",
            "--element-index",
            "0",
            "--click-count",
            "0",
            "--json",
        ],
        "Invalid positive integer for --click-count",
    );
    assert_invalid(
        dir.path(),
        &[
            "computer",
            "click",
            "--app",
            "X",
            "--element-index",
            "0",
            "--modifiers",
            "CmdOrCtrl+Shift+X+Alt+Z",
            "--json",
        ],
        "Click modifiers accept modifier keys only, e.g. CmdOrCtrl or CmdOrCtrl+Shift.",
    );
}

#[test]
fn permissions_rejects_an_unknown_setup_id() {
    let dir = TestDir::new();
    assert_invalid(
        dir.path(),
        &["computer", "permissions", "--id", "camera", "--json"],
        "--id must be \"accessibility\" or \"screenshots\"",
    );
}

#[test]
fn validation_errors_do_not_require_a_running_runtime() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["computer", "click", "--app", "X", "--json"]);
    assert_eq!(output.exit_code(), 1);
    assert_eq!(
        output.error_message(),
        "Click requires --element-index or both --x and --y"
    );
}

#[test]
fn human_mode_prints_error_code_and_message_and_exits_one() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["computer", "click", "--app", "X"]);
    assert_eq!(output.exit_code(), 1);
    assert!(output.stdout.is_empty(), "stdout={:?}", output.stdout);
    assert_eq!(
        output.stderr.trim(),
        "error invalid_argument: Click requires --element-index or both --x and --y"
    );
}

// ---------------------------------------------------------------------------
// screenshot spillover (--json only)

fn screenshot_result(snapshot_id: Option<&str>) -> Value {
    let mut snapshot = json!({
        "app": {"name": "Finder", "pid": 123, "bundleId": "com.apple.finder"},
        "window": {"id": 7, "index": 0, "title": "Desktop", "width": 800, "height": 600},
        "elementCount": 12,
        "coordinateSpace": "screen",
        "treeText": "- window",
    });
    if let Some(id) = snapshot_id {
        snapshot["id"] = json!(id);
    }
    json!({
        "snapshot": snapshot,
        "screenshot": {"data": "aGVsbG8=", "format": "png", "width": 100, "height": 50, "scale": 1},
        "screenshotStatus": {"state": "captured"},
    })
}

fn parse_rfc3339_millis(value: &str) -> Option<u128> {
    let bytes = value.as_bytes();
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[23] != b'Z'
    {
        return None;
    }
    let year: i64 = value[0..4].parse().ok()?;
    let month: u32 = value[5..7].parse().ok()?;
    let day: u32 = value[8..10].parse().ok()?;
    let hour: u128 = value[11..13].parse().ok()?;
    let minute: u128 = value[14..16].parse().ok()?;
    let second: u128 = value[17..19].parse().ok()?;
    let millis: u128 = value[20..23].parse().ok()?;
    let days = days_from_civil(year, month, day);
    Some(((days as u128 * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000) + millis)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis()
}

#[test]
fn json_screenshot_spills_to_a_0600_file_and_replaces_the_inline_data() {
    let runtime = FakeRuntime::start();
    let shot_dir = TestDir::new();
    runtime.set_method_result("computer.getAppState", screenshot_result(Some("snap-42")));

    let output = run_with_env(
        runtime.user_data_path(),
        &["computer", "get-app-state", "--app", "Finder", "--json"],
        &[(
            "ORCINUS_COMPUTER_SCREENSHOT_TMPDIR",
            shot_dir.path().to_str().expect("utf8 path"),
        )],
    );
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let envelope = output.json();

    let screenshot = &envelope["result"]["screenshot"];
    assert!(
        screenshot.get("data").is_none(),
        "inline data must be omitted: {screenshot}"
    );
    assert_eq!(screenshot["dataOmitted"], true);
    let path = PathBuf::from(screenshot["path"].as_str().expect("path string"));
    assert_eq!(path, shot_dir.path().join("snap-42-screenshot.png"));
    assert_eq!(
        std::fs::read(&path).expect("read spilt screenshot"),
        b"hello"
    );
    let mode = std::fs::metadata(&path)
        .expect("stat spilt screenshot")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "screenshot file must be 0600");

    let expires_at = screenshot["expiresAt"].as_str().expect("expiresAt string");
    let expires_ms = parse_rfc3339_millis(expires_at).expect("RFC3339 expiresAt");
    let target = now_millis() + 24 * 60 * 60 * 1000;
    assert!(
        expires_ms.abs_diff(target) < 300_000,
        "expiresAt {expires_at} must be ~24h from now"
    );
}

#[test]
fn json_screenshot_falls_back_to_the_envelope_id_without_a_snapshot_id() {
    let runtime = FakeRuntime::start();
    let shot_dir = TestDir::new();
    runtime.set_method_result("computer.getAppState", screenshot_result(None));

    let output = run_with_env(
        runtime.user_data_path(),
        &["computer", "get-app-state", "--app", "Finder", "--json"],
        &[(
            "ORCINUS_COMPUTER_SCREENSHOT_TMPDIR",
            shot_dir.path().to_str().expect("utf8 path"),
        )],
    );
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let envelope = output.json();
    let id = envelope["id"].as_str().expect("envelope id");
    let expected = shot_dir.path().join(format!("{id}-screenshot.png"));
    assert_eq!(
        envelope["result"]["screenshot"]["path"],
        expected.to_str().unwrap()
    );
    assert!(expected.exists(), "spilt file must use the request id");
}

#[test]
fn json_screenshot_cleanup_removes_files_older_than_24h_and_keeps_the_rest() {
    let runtime = FakeRuntime::start();
    let shot_dir = TestDir::new();
    let stale = shot_dir.path().join("stale-screenshot.png");
    std::fs::write(&stale, b"old").expect("write stale screenshot");
    let keep = shot_dir.path().join("keep.txt");
    std::fs::write(&keep, b"keep").expect("write keep file");
    let stale_age = SystemTime::now() - Duration::from_secs(25 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .expect("open stale screenshot")
        .set_modified(stale_age)
        .expect("age stale screenshot");

    runtime.set_method_result("computer.getAppState", screenshot_result(Some("snap-1")));
    let output = run_with_env(
        runtime.user_data_path(),
        &["computer", "get-app-state", "--app", "Finder", "--json"],
        &[(
            "ORCINUS_COMPUTER_SCREENSHOT_TMPDIR",
            shot_dir.path().to_str().expect("utf8 path"),
        )],
    );
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);

    assert!(!stale.exists(), "25h-old screenshot must be swept");
    assert!(keep.exists(), "unrelated files must survive cleanup");
    assert!(
        shot_dir.path().join("snap-1-screenshot.png").exists(),
        "the fresh screenshot must be written"
    );
    let marker_mode = std::fs::metadata(shot_dir.path().join(".last-cleanup"))
        .expect("cleanup marker")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(marker_mode, 0o600);
}

#[test]
fn json_screenshot_write_failure_keeps_the_inline_data() {
    let runtime = FakeRuntime::start();
    runtime.set_method_result("computer.getAppState", screenshot_result(Some("snap-9")));

    let output = run_with_env(
        runtime.user_data_path(),
        &["computer", "get-app-state", "--app", "Finder", "--json"],
        &[(
            "ORCINUS_COMPUTER_SCREENSHOT_TMPDIR",
            "/dev/null/orcinus-nope",
        )],
    );
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    let screenshot = &output.json()["result"]["screenshot"];
    assert_eq!(screenshot["data"], "aGVsbG8=");
    assert!(screenshot.get("path").is_none(), "no path on failure");
    assert!(screenshot.get("dataOmitted").is_none());
}

// ---------------------------------------------------------------------------
// human mode success summaries

#[test]
fn human_get_app_state_prints_app_window_elements_and_tree() {
    let runtime = FakeRuntime::start();
    runtime.set_method_result("computer.getAppState", snapshot_result());

    let output = run(
        runtime.user_data_path(),
        &["computer", "get-app-state", "--app", "Finder"],
    );
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    assert!(output.stdout.contains("Finder"), "{}", output.stdout);
    assert!(output.stdout.contains("pid 123"), "{}", output.stdout);
    assert!(output.stdout.contains("Desktop"), "{}", output.stdout);
    assert!(
        output.stdout.contains("Visible elements: 12"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("button \"OK\""), "{}", output.stdout);
}

#[test]
fn human_action_prints_action_and_verification_summary() {
    let runtime = FakeRuntime::start();
    runtime.set_method_result("computer.click", action_result());

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "click",
            "--app",
            "Finder",
            "--element-index",
            "3",
        ],
    );
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    assert!(
        output
            .stdout
            .contains("action=synthetic verification=unverified"),
        "{}",
        output.stdout
    );
}

#[test]
fn human_list_apps_prints_rows_and_capabilities_prints_key_fields() {
    let runtime = FakeRuntime::start();
    runtime.set_method_result(
        "computer.listApps",
        json!({"apps": [
            {"name": "Finder", "pid": 1, "bundleId": "com.apple.finder"},
            {"name": "Notes", "pid": 2},
        ]}),
    );

    let output = run(runtime.user_data_path(), &["computer", "list-apps"]);
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    assert!(
        output.stdout.contains("Finder  pid:1  com.apple.finder"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("Notes  pid:2"), "{}", output.stdout);

    runtime.set_method_result(
        "computer.capabilities",
        json!({
            "provider": "macos",
            "platform": "darwin",
            "protocolVersion": 1,
            "supports": {
                "apps": {"list": true, "bundleIds": false, "pids": true},
                "windows": {"list": true, "targetById": true, "targetByIndex": true},
                "observation": {"screenshot": true, "elementFrames": false, "annotatedScreenshot": false},
                "actions": {"click": true, "scroll": false},
            },
        }),
    );
    let output = run(runtime.user_data_path(), &["computer", "capabilities"]);
    assert_eq!(output.exit_code(), 0, "stderr={:?}", output.stderr);
    assert!(
        output.stdout.contains("macos (darwin, protocol 1)"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("Actions: click"),
        "{}",
        output.stdout
    );
}

// ---------------------------------------------------------------------------
// RPC error passthrough

#[test]
fn rpc_errors_pass_code_message_and_data_through_and_exit_one() {
    let runtime = FakeRuntime::start();
    runtime.set_method_error_with_data(
        "computer.click",
        "element_not_found",
        "Element 7 is no longer available.",
        Some(json!({"elementIndex": 7})),
    );

    let output = run(
        runtime.user_data_path(),
        &[
            "computer",
            "click",
            "--app",
            "Finder",
            "--element-index",
            "7",
            "--json",
        ],
    );
    assert_eq!(output.exit_code(), 1);
    let envelope = output.json();
    assert_eq!(envelope["id"], "local");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "element_not_found");
    assert_eq!(
        envelope["error"]["message"],
        "Element 7 is no longer available."
    );
    assert_eq!(envelope["error"]["data"], json!({"elementIndex": 7}));
    assert_eq!(envelope["_meta"]["runtimeId"], RUNTIME_ID);
}

#[test]
fn rpc_errors_in_human_mode_print_code_and_message() {
    let runtime = FakeRuntime::start();
    runtime.set_method_error(
        "computer.listApps",
        "permission_denied",
        "Accessibility is not granted.",
    );

    let output = run(runtime.user_data_path(), &["computer", "list-apps"]);
    assert_eq!(output.exit_code(), 1);
    assert!(output.stdout.is_empty(), "stdout={:?}", output.stdout);
    assert_eq!(
        output.stderr.trim(),
        "error permission_denied: Accessibility is not granted."
    );
}

#[test]
fn computer_without_a_running_runtime_is_runtime_unavailable() {
    let dir = TestDir::new();
    let output = run(dir.path(), &["computer", "list-apps", "--json"]);
    assert_eq!(output.exit_code(), 1);
    let envelope = output.json();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "runtime_unavailable");
}
