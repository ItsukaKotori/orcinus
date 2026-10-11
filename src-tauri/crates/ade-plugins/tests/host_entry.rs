//! Black-box test of `resources/plugins/host-entry.mjs` over real stdio.
//!
//! Spawns the system `node` (`ORCINUS_NODE_PATH` overrides) against a
//! throwaway plugin fixture in a tempdir and drives the NDJSON protocol like
//! the supervisor will. When no Node runs on the machine the tests record a
//! skip and pass instead of failing CI.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use ade_plugins::worker::protocol::{read_frame, write_frame, ChildFrame, LogLevel, ParentFrame};
use ade_plugins::PluginError;
use serde_json::json;

const FRAME_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

fn node_executable() -> Option<PathBuf> {
    let candidate = std::env::var_os("ORCINUS_NODE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("node"));
    match Command::new(&candidate).arg("--version").output() {
        Ok(output) if output.status.success() => Some(candidate),
        _ => None,
    }
}

fn host_entry_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/plugins/host-entry.mjs")
}

fn write_fixture(root: &Path, source: &str) -> PathBuf {
    let plugin_root = root.join("hello-plugin");
    std::fs::create_dir_all(&plugin_root).unwrap();
    std::fs::write(plugin_root.join("main.mjs"), source).unwrap();
    plugin_root
}

fn init_frame(plugin_root: &Path) -> ParentFrame {
    ParentFrame::Init {
        plugin_id: "pub.hello".to_owned(),
        plugin_root: plugin_root.to_string_lossy().into_owned(),
        main_entry: "main.mjs".to_owned(),
        granted_capabilities: vec!["storage".to_owned()],
    }
}

struct WorkerHarness {
    child: Child,
    stdin: ChildStdin,
    frames: Receiver<Result<Option<ChildFrame>, PluginError>>,
    stderr: Receiver<String>,
}

impl WorkerHarness {
    fn start(node: &Path, plugin_root: &Path) -> Self {
        let mut child = Command::new(node)
            .arg(host_entry_path())
            .current_dir(plugin_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn node host-entry.mjs");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let (frame_tx, frames) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_frame::<_, ChildFrame>(&mut reader) {
                    Ok(Some(frame)) => {
                        if frame_tx.send(Ok(Some(frame))).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = frame_tx.send(Ok(None));
                        break;
                    }
                    Err(error) => {
                        let _ = frame_tx.send(Err(error));
                        break;
                    }
                }
            }
        });

        let (stderr_tx, stderr_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            let _ = stderr_tx.send(text);
        });

        Self {
            child,
            stdin,
            frames,
            stderr: stderr_rx,
        }
    }

    fn send(&mut self, frame: &ParentFrame) {
        write_frame(&mut self.stdin, frame).expect("write parent frame");
    }

    fn send_raw(&mut self, line: &str) {
        use std::io::Write;
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .expect("write raw stdin");
    }

    fn next_frame(&mut self) -> ChildFrame {
        match self.frames.recv_timeout(FRAME_TIMEOUT) {
            Ok(Ok(Some(frame))) => frame,
            Ok(Ok(None)) => panic!("worker stdout closed while waiting for a frame"),
            Ok(Err(error)) => panic!("failed to read worker frame: {error}"),
            Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for worker frame"),
            Err(RecvTimeoutError::Disconnected) => panic!("worker frame reader dropped"),
        }
    }

    fn wait_exit(&mut self) -> i32 {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status.code().unwrap_or(-1);
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("timed out waiting for worker exit");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn stderr_text(&self) -> String {
        self.stderr
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_default()
    }
}

impl Drop for WorkerHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const HELLO_PLUGIN: &str = r#"
import { writeFileSync } from 'node:fs'

export default async function activate(orca) {
  console.log('stdout noise that must not corrupt frames')
  orca.log('hello from plugin')
  orca.events.on('worktree.created', (payload) => {
    orca.log(`event:${payload.worktreeId}`)
  })
  orca.commands.register('echo', async (args) => {
    const value = await orca.host.call('storage.get', { key: args.key })
    return { echoed: value }
  })
  orca.commands.register('fail', async () => {
    await orca.host.call('secrets.get', { key: 'x' })
  })
}

export function deactivate() {
  writeFileSync(new URL('./deactivated.txt', import.meta.url), 'bye')
}
"#;

#[test]
fn hello_plugin_worker_round_trip_over_ndjson() {
    let Some(node) = node_executable() else {
        eprintln!("SKIP hello_plugin_worker_round_trip_over_ndjson: no node on PATH (set ORCINUS_NODE_PATH)");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let plugin_root = write_fixture(temp.path(), HELLO_PLUGIN);
    let mut worker = WorkerHarness::start(&node, &plugin_root);

    worker.send(&init_frame(&plugin_root));
    assert_eq!(
        worker.next_frame(),
        ChildFrame::Log {
            level: LogLevel::Info,
            message: "hello from plugin".to_owned(),
        }
    );
    assert_eq!(
        worker.next_frame(),
        ChildFrame::Ready {
            commands: vec!["echo".to_owned(), "fail".to_owned()],
        }
    );

    worker.send(&ParentFrame::InvokeCommand {
        call_id: 1,
        command_id: "echo".to_owned(),
        args: Some(json!({ "key": "greeting" })),
    });
    assert_eq!(
        worker.next_frame(),
        ChildFrame::HostCall {
            call_id: 0,
            method: "storage.get".to_owned(),
            params: Some(json!({ "key": "greeting" })),
        }
    );
    worker.send(&ParentFrame::HostResult {
        call_id: 0,
        ok: true,
        value: Some(json!({ "stored": "hi" })),
        error_code: None,
        error: None,
    });
    assert_eq!(
        worker.next_frame(),
        ChildFrame::CommandResult {
            call_id: 1,
            ok: true,
            value: Some(json!({ "echoed": { "stored": "hi" } })),
            error: None,
        }
    );

    worker.send(&ParentFrame::InvokeCommand {
        call_id: 2,
        command_id: "fail".to_owned(),
        args: None,
    });
    assert_eq!(
        worker.next_frame(),
        ChildFrame::HostCall {
            call_id: 1,
            method: "secrets.get".to_owned(),
            params: Some(json!({ "key": "x" })),
        }
    );
    worker.send(&ParentFrame::HostResult {
        call_id: 1,
        ok: false,
        value: None,
        error_code: Some("capability_denied".to_owned()),
        error: Some("denied by gate".to_owned()),
    });
    match worker.next_frame() {
        ChildFrame::CommandResult {
            call_id: 2,
            ok: false,
            error: Some(error),
            ..
        } => assert!(
            error.contains("denied by gate"),
            "unexpected error: {error}"
        ),
        other => panic!("unexpected frame: {other:?}"),
    }

    worker.send(&ParentFrame::InvokeCommand {
        call_id: 3,
        command_id: "nope".to_owned(),
        args: None,
    });
    assert_eq!(
        worker.next_frame(),
        ChildFrame::CommandResult {
            call_id: 3,
            ok: false,
            value: None,
            error: Some("no handler registered for command nope".to_owned()),
        }
    );

    worker.send(&ParentFrame::DeliverEvent {
        event_id: 5,
        event: "worktree.created".to_owned(),
        payload: json!({ "worktreeId": "wt-1" }),
    });
    assert_eq!(
        worker.next_frame(),
        ChildFrame::Log {
            level: LogLevel::Info,
            message: "event:wt-1".to_owned(),
        }
    );
    assert_eq!(worker.next_frame(), ChildFrame::EventAck { event_id: 5 });

    worker.send_raw("this is not a frame\n");
    assert_eq!(
        worker.next_frame(),
        ChildFrame::Log {
            level: LogLevel::Warn,
            message: "ignoring malformed parent message".to_owned(),
        }
    );

    worker.send(&ParentFrame::Shutdown);
    assert_eq!(worker.wait_exit(), 0);
    assert_eq!(
        std::fs::read_to_string(plugin_root.join("deactivated.txt")).unwrap(),
        "bye"
    );
    assert!(worker.stderr_text().contains("stdout noise"));
}

#[test]
fn activate_failure_reports_fatal_and_exits_one() {
    let Some(node) = node_executable() else {
        eprintln!("SKIP activate_failure_reports_fatal_and_exits_one: no node on PATH (set ORCINUS_NODE_PATH)");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let plugin_root = write_fixture(
        temp.path(),
        "export default async function activate() { throw new Error('boom activate') }\n",
    );
    let mut worker = WorkerHarness::start(&node, &plugin_root);

    worker.send(&init_frame(&plugin_root));
    match worker.next_frame() {
        ChildFrame::Fatal { error } => assert!(error.contains("boom activate"), "got: {error}"),
        other => panic!("unexpected frame: {other:?}"),
    }
    assert_eq!(worker.wait_exit(), 1);
}
