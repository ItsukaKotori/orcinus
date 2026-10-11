//! Fake runtime server for `orcinus` CLI integration tests. This harness is
//! independent of `ade_computer::rpc`: it reads the wire shape from
//! `ade_computer::protocol` (metadata fields, envelope keys, error codes) and
//! serves NDJSON over a real Unix socket, so the CLI is exercised end to end
//! without launching the app or the real runtime RPC server.
//!
//! `FakeRuntime::start` writes `runtime.json` (0600) into a temp user-data dir
//! and answers `status.get` with a fixed snapshot. Tests can override the auth
//! token the CLI reads, delay responses, or run without a server at all.

#![allow(dead_code)]

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ade_computer::protocol::{RuntimeMetadata, RuntimeTransport, RUNTIME_METADATA_FILE};
use serde_json::{json, Value};

const ACCEPT_POLL: Duration = Duration::from_millis(2);
const READ_POLL: Duration = Duration::from_millis(20);

pub const RUNTIME_ID: &str = "01JCLITESTRUNTIME";
pub const AUTH_TOKEN: &str = "test-auth-token-0123456789abcdef";
pub const APP_VERSION: &str = "0.0.1-cli-test";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "orcinus-cli-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp root");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One NDJSON request the fake runtime received.
#[derive(Debug, Clone)]
pub struct FakeRequest {
    pub method: String,
    pub params: Value,
    pub auth_token: Option<String>,
}

struct ServerState {
    shutdown: AtomicBool,
    delay: std::sync::Mutex<Duration>,
    auth_token: std::sync::Mutex<String>,
}

/// A fake runtime listening on a real Unix socket with `runtime.json` written.
pub struct FakeRuntime {
    root: TempRoot,
    socket_path: PathBuf,
    metadata_path: PathBuf,
    state: Arc<ServerState>,
    listener: Option<UnixListener>,
    accept_thread: Option<JoinHandle<()>>,
    requests: std::sync::mpsc::Receiver<FakeRequest>,
}

impl FakeRuntime {
    pub fn start() -> Self {
        let root = TempRoot::new();
        let socket_path = root.0.join("runtime.sock");
        let metadata_path = root.0.join(RUNTIME_METADATA_FILE);
        let state = Arc::new(ServerState {
            shutdown: AtomicBool::new(false),
            delay: std::sync::Mutex::new(Duration::ZERO),
            auth_token: std::sync::Mutex::new(AUTH_TOKEN.to_string()),
        });
        let (tx, requests) = std::sync::mpsc::channel();

        let listener = UnixListener::bind(&socket_path).expect("bind fake runtime socket");
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .expect("chmod fake socket");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fake listener");
        let accept_thread = {
            let state = Arc::clone(&state);
            let server_listener = listener.try_clone().expect("clone listener");
            thread::spawn(move || accept_loop(server_listener, state, tx))
        };

        let runtime = Self {
            root,
            socket_path,
            metadata_path,
            state,
            listener: Some(listener),
            accept_thread: Some(accept_thread),
            requests,
        };
        runtime.write_metadata(AUTH_TOKEN, RUNTIME_ID);
        runtime
    }

    fn write_metadata(&self, auth_token: &str, runtime_id: &str) {
        let metadata = RuntimeMetadata {
            runtime_id: runtime_id.to_string(),
            pid: std::process::id(),
            transports: vec![RuntimeTransport::Unix {
                endpoint: self.socket_path.to_string_lossy().into_owned(),
            }],
            auth_token: auth_token.to_string(),
            started_at: "2026-10-11T10:00:00.000Z".to_string(),
        };
        let bytes = serde_json::to_vec(&metadata).expect("serialize metadata");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.metadata_path)
            .expect("write metadata");
        file.write_all(&bytes).expect("write metadata bytes");
    }

    /// Rewrite `runtime.json` so the CLI reads a token the server rejects.
    pub fn set_metadata_auth_token(&self, auth_token: &str) {
        self.write_metadata(auth_token, RUNTIME_ID);
    }

    /// Override the token the server accepts for every request.
    pub fn set_server_auth_token(&self, auth_token: &str) {
        *self.state.auth_token.lock().unwrap() = auth_token.to_string();
    }

    /// Delay every response (used to drive the CLI 1s status timeout).
    pub fn set_response_delay(&self, delay: Duration) {
        *self.state.delay.lock().unwrap() = delay;
    }

    pub fn user_data_path(&self) -> &Path {
        &self.root.0
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub fn next_request(&self, timeout: Duration) -> FakeRequest {
        self.requests
            .recv_timeout(timeout)
            .expect("fake runtime received no request in time")
    }

    pub fn no_request_within(&self, timeout: Duration) -> bool {
        self.requests.recv_timeout(timeout).is_err()
    }

    /// Delete `runtime.json` (used for the app-not-running anchors).
    pub fn remove_metadata(&self) {
        let _ = fs::remove_file(&self.metadata_path);
    }
}

impl Drop for FakeRuntime {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
        let _ = self.listener.take();
        let _ = fs::remove_file(&self.socket_path);
    }
}

fn accept_loop(
    listener: UnixListener,
    state: Arc<ServerState>,
    tx: std::sync::mpsc::Sender<FakeRequest>,
) {
    loop {
        if state.shutdown.load(Ordering::SeqCst) {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if state.shutdown.load(Ordering::SeqCst) {
                    return;
                }
                let state = Arc::clone(&state);
                let tx = tx.clone();
                thread::spawn(move || handle_connection(stream, &state, &tx));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => return,
        }
    }
}

fn handle_connection(
    mut stream: UnixStream,
    state: &Arc<ServerState>,
    tx: &std::sync::mpsc::Sender<FakeRequest>,
) {
    let _ = stream.set_read_timeout(Some(READ_POLL));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if state.shutdown.load(Ordering::SeqCst) {
            return;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => {
                pending.extend_from_slice(&chunk[..read]);
                while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = pending.drain(..=newline).collect();
                    let raw = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
                    if raw.trim().is_empty() {
                        continue;
                    }
                    respond(&stream, &raw, state, tx);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

fn respond(
    stream: &UnixStream,
    raw: &str,
    state: &Arc<ServerState>,
    tx: &std::sync::mpsc::Sender<FakeRequest>,
) {
    let Ok(value) = serde_json::from_str::<Value>(raw.trim()) else {
        return;
    };
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    let auth_token = value
        .get("authToken")
        .and_then(Value::as_str)
        .map(str::to_string);
    let _ = tx.send(FakeRequest {
        method: method.clone(),
        params,
        auth_token: auth_token.clone(),
    });

    let expected = state.auth_token.lock().unwrap().clone();
    if auth_token.as_deref() != Some(expected.as_str()) {
        write_frame(
            stream,
            &failure_frame(&id, "unauthorized", "Invalid auth token"),
        );
        return;
    }

    // Why: delay only the reply so the request is observable immediately while
    // the client-side timeout still fires mid-flight.
    let delay = *state.delay.lock().unwrap();
    if !delay.is_zero() {
        thread::sleep(delay);
    }

    match method.as_str() {
        "status.get" => {
            write_frame(stream, &success_frame(&id, status_result()));
        }
        other => {
            write_frame(
                stream,
                &failure_frame(&id, "method_not_found", &format!("Unknown method: {other}")),
            );
        }
    }
}

fn status_result() -> Value {
    json!({
        "app": {
            "running": true,
            "pid": std::process::id(),
            "desktopWindowStatus": "available",
        },
        "runtime": {
            "state": "ready",
            "reachable": true,
            "runtimeId": RUNTIME_ID,
            "appVersion": APP_VERSION,
        },
        "graph": {"state": "not_ready"},
    })
}

fn success_frame(id: &str, result: Value) -> Value {
    json!({
        "id": id,
        "ok": true,
        "result": result,
        "_meta": {"runtimeId": RUNTIME_ID},
    })
}

fn failure_frame(id: &str, code: &str, message: &str) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": {"code": code, "message": message},
        "_meta": {"runtimeId": RUNTIME_ID},
    })
}

fn write_frame(stream: &UnixStream, frame: &Value) -> bool {
    let mut line = match serde_json::to_vec(frame) {
        Ok(line) => line,
        Err(_) => return false,
    };
    line.push(b'\n');
    let mut writer = stream;
    writer.write_all(&line).is_ok() && writer.flush().is_ok()
}

/// Wait for a path to appear (used to sequence the CLI against a socket).
pub fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    path.exists()
}
