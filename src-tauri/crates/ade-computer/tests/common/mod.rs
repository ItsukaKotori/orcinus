//! Scripted fake computer-use helper for transport/client integration tests.
//!
//! `HelperTransport` spawns a real process with
//! `--agent <socket> --token-file <token>` and then talks NDJSON over the Unix
//! socket at `<socket>`. This harness provides both halves without the Swift
//! helper or a real app:
//!
//! * the spawned executable is a tiny `/bin/sh` stub that records its pid and
//!   argv to `events.log` and then waits to be signalled (so tests can assert
//!   argv, SIGTERM delivery and process death);
//! * the NDJSON socket server runs on a supervisor thread inside the test
//!   process. The supervisor watches `events.log` for new spawns, binds the
//!   socket path the transport passed, reports every received request over a
//!   channel, and answers according to the script installed with
//!   [`FakeHelper::respond_with`].
//!
//! The transport still performs a real spawn, a real connect-with-retry, real
//! token handling and real restarts; only the helper binary is fake.

#![allow(dead_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ade_computer::paths::helper_executable;
use serde_json::{json, Value};

const POLL_INTERVAL: Duration = Duration::from_millis(2);
const CONNECTION_READ_TIMEOUT: Duration = Duration::from_millis(50);
const SPAWN_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const EXPECTED_ARG_COUNT: usize = 4;

/// One NDJSON request line received by the fake helper.
#[derive(Debug, Clone)]
pub struct FakeRequest {
    pub id: u64,
    pub method: String,
    pub params: Value,
    pub token: Option<String>,
    /// The raw line exactly as it arrived, including the trailing newline.
    pub raw: String,
    pub received_at: Instant,
}

/// One observed helper process spawn (pid + argv + transport state).
#[derive(Debug, Clone)]
pub struct FakeSpawn {
    pub pid: u32,
    pub args: Vec<String>,
    pub socket_path: PathBuf,
    pub token_path: PathBuf,
    pub directory: PathBuf,
    /// The token read from `token_path` when the helper started.
    pub token: String,
}

/// Scripted response behaviour, consumed one entry per request.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeResponse {
    /// Reply `{"ok":true,"result":{"method":<m>,"params":<p>}}`.
    Echo,
    /// Reply `{"ok":true,"result":<value>}`.
    Result(Value),
    /// Sleep, then behave like [`FakeResponse::Echo`].
    Delay(Duration),
    /// Close the connection without replying (mid-request disconnect).
    Disconnect,
    /// Reply `{"ok":false,"error":{"code":<code>,"message":<message>}}`.
    Error { code: String, message: String },
    /// Reply once with a different (stale) id, then with the real response.
    /// Asserts the peer discards responses it did not ask for.
    StaleThen(Value),
}

impl FakeResponse {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Default)]
struct ScriptState {
    responses: Vec<FakeResponse>,
    next: usize,
    expected_token: Option<String>,
}

impl ScriptState {
    fn take(&mut self) -> FakeResponse {
        let response = self
            .responses
            .get(self.next)
            .cloned()
            .unwrap_or(FakeResponse::Echo);
        self.next += 1;
        response
    }
}

struct Shared {
    script: Mutex<ScriptState>,
    spawns: Mutex<Vec<FakeSpawn>>,
    terminated: Mutex<Vec<u32>>,
    requests_tx: Sender<FakeRequest>,
    listeners: Mutex<Vec<JoinHandle<()>>>,
    shutdown: AtomicBool,
}

/// A scripted fake helper plus the fake `.app` the transport can spawn.
pub struct FakeHelper {
    root: PathBuf,
    app_path: PathBuf,
    shared: Arc<Shared>,
    requests_rx: Receiver<FakeRequest>,
    supervisor: Option<JoinHandle<()>>,
}

impl FakeHelper {
    /// Create a fake helper app bundle and start supervising it.
    pub fn start() -> Self {
        static INSTANCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ade-computer-fake-helper-{}-{}",
            std::process::id(),
            INSTANCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("create fake helper root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("chmod fake helper root");

        let app_path = root.join("Orcinus Computer Use.app");
        let executable = helper_executable(&app_path);
        fs::create_dir_all(executable.parent().expect("executable parent"))
            .expect("create fake app bundle");
        let events_path = root.join("events.log");
        write_helper_stub(&executable, &events_path);
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("chmod fake helper stub");

        let (requests_tx, requests_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            script: Mutex::new(ScriptState::default()),
            spawns: Mutex::new(Vec::new()),
            terminated: Mutex::new(Vec::new()),
            requests_tx,
            listeners: Mutex::new(Vec::new()),
            shutdown: AtomicBool::new(false),
        });
        let supervisor = thread::spawn({
            let shared = Arc::clone(&shared);
            let events_path = events_path.clone();
            move || supervise(shared, events_path)
        });

        Self {
            root,
            app_path,
            shared,
            requests_rx,
            supervisor: Some(supervisor),
        }
    }

    /// The fake `.app` bundle path to hand to `HelperTransport::start`.
    pub fn helper_app(&self) -> &Path {
        &self.app_path
    }

    /// Replace the response script (index reset to zero).
    pub fn respond_with(&self, script: impl IntoIterator<Item = FakeResponse>) {
        let mut state = self.shared.script.lock().unwrap();
        state.responses = script.into_iter().collect();
        state.next = 0;
    }

    /// Force the token the fake helper expects for every request. The default
    /// is the token the helper read from `--token-file` at spawn time.
    pub fn set_expected_token(&self, token: impl Into<String>) {
        self.shared.script.lock().unwrap().expected_token = Some(token.into());
    }

    /// All helper spawns observed so far.
    pub fn spawns(&self) -> Vec<FakeSpawn> {
        self.shared.spawns.lock().unwrap().clone()
    }

    /// Block until the `index`-th helper process has spawned.
    pub fn wait_for_spawn(&self, index: usize) -> FakeSpawn {
        let deadline = Instant::now() + SPAWN_WAIT_TIMEOUT;
        loop {
            if let Some(spawn) = self.shared.spawns.lock().unwrap().get(index).cloned() {
                return spawn;
            }
            assert!(
                Instant::now() < deadline,
                "fake helper never spawned (index {index})"
            );
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// The most recent helper spawn (waits until at least one was observed).
    pub fn last_spawn(&self) -> FakeSpawn {
        self.wait_for_spawn(0);
        self.spawns().last().cloned().expect("spawn observed")
    }

    /// Socket path the transport passed to the latest helper spawn.
    pub fn socket_path(&self) -> PathBuf {
        self.last_spawn().socket_path
    }

    /// Token-file path the transport passed to the latest helper spawn.
    pub fn token_path(&self) -> PathBuf {
        self.last_spawn().token_path
    }

    /// Block until a request line arrives, or panic on timeout.
    pub fn next_request(&self, timeout: Duration) -> FakeRequest {
        self.requests_rx
            .recv_timeout(timeout)
            .expect("fake helper did not receive a request in time")
    }

    /// Receiver for every request line the fake helper serves.
    pub fn requests_rx(&self) -> &Receiver<FakeRequest> {
        &self.requests_rx
    }

    /// Supervisor thread handle (the server stops when `FakeHelper` drops).
    pub fn join_handle(&self) -> Option<&JoinHandle<()>> {
        self.supervisor.as_ref()
    }

    /// Block until the stub recorded a SIGTERM for `pid`.
    pub fn wait_for_termination(&self, pid: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.shared.terminated.lock().unwrap().contains(&pid) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Block until `pid` is no longer alive.
    pub fn wait_for_process_exit(&self, pid: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if !process_alive(pid) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Block until `path` disappears.
    pub fn wait_for_removed(path: &Path, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if !path.exists() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for FakeHelper {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        for spawn in self.spawns() {
            if process_alive(spawn.pid) {
                unsafe {
                    libc::kill(spawn.pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
        let listeners = std::mem::take(&mut *self.shared.listeners.lock().unwrap());
        for listener in listeners {
            let _ = listener.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Whether `pid` is still alive (`kill(pid, 0)`).
pub fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn write_helper_stub(executable: &Path, events_path: &Path) {
    let script = format!(
        "#!/bin/sh\n\
         events='{events}'\n\
         {{\n\
         \tprintf 'spawn %s\\n' \"$$\"\n\
         \tfor arg in \"$@\"; do printf 'a %s\\n' \"$arg\"; done\n\
         }} >> \"$events\"\n\
         trap 'printf \"term %s\\n\" \"$$\" >> \"$events\"; kill \"$child\" 2>/dev/null; exit 143' TERM INT HUP\n\
         sleep 3600 &\n\
         child=$!\n\
         wait \"$child\"\n\
         exit 0\n",
        events = events_path.display()
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(executable)
        .expect("create fake helper stub");
    file.write_all(script.as_bytes())
        .expect("write fake helper stub");
}

enum Event {
    Spawn(u32),
    Arg(String),
    Term(u32),
}

fn parse_event(line: &str) -> Option<Event> {
    if let Some(rest) = line.strip_prefix("spawn ") {
        return rest.trim().parse().ok().map(Event::Spawn);
    }
    if let Some(rest) = line.strip_prefix("a ") {
        return Some(Event::Arg(rest.to_string()));
    }
    if let Some(rest) = line.strip_prefix("term ") {
        return rest.trim().parse().ok().map(Event::Term);
    }
    None
}

struct PendingSpawn {
    pid: u32,
    args: Vec<String>,
}

fn supervise(shared: Arc<Shared>, events_path: PathBuf) {
    let mut file: Option<File> = None;
    let mut carry = String::new();
    let mut pending: Option<PendingSpawn> = None;
    loop {
        if shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        if file.is_none() {
            file = File::open(&events_path).ok();
        }
        if let Some(reader) = file.as_mut() {
            let mut chunk = String::new();
            if matches!(reader.read_to_string(&mut chunk), Ok(read) if read > 0) {
                carry.push_str(&chunk);
                while let Some(newline) = carry.find('\n') {
                    let line: String = carry.drain(..=newline).collect();
                    match parse_event(line.trim_end()) {
                        Some(Event::Spawn(pid)) => {
                            if let Some(previous) = pending.take() {
                                finalize_spawn(&shared, previous);
                            }
                            pending = Some(PendingSpawn {
                                pid,
                                args: Vec::new(),
                            });
                        }
                        Some(Event::Arg(arg)) => {
                            if let Some(spawn) = pending.as_mut() {
                                spawn.args.push(arg);
                                if spawn.args.len() >= EXPECTED_ARG_COUNT {
                                    if let Some(spawn) = pending.take() {
                                        finalize_spawn(&shared, spawn);
                                    }
                                }
                            }
                        }
                        Some(Event::Term(pid)) => {
                            shared.terminated.lock().unwrap().push(pid);
                        }
                        None => {}
                    }
                }
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn finalize_spawn(shared: &Arc<Shared>, pending: PendingSpawn) {
    if pending.args.len() < EXPECTED_ARG_COUNT {
        return;
    }
    let socket_path = PathBuf::from(&pending.args[1]);
    let token_path = PathBuf::from(&pending.args[3]);
    let token = fs::read_to_string(&token_path)
        .map(|token| token.trim().to_string())
        .unwrap_or_default();
    shared.script.lock().unwrap().expected_token = Some(token.clone());
    let directory = socket_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    shared.spawns.lock().unwrap().push(FakeSpawn {
        pid: pending.pid,
        args: pending.args,
        socket_path: socket_path.clone(),
        token_path,
        directory,
        token,
    });
    spawn_listener(shared, socket_path);
}

fn spawn_listener(shared: &Arc<Shared>, socket_path: PathBuf) {
    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "fake helper could not bind {}: {error}",
                socket_path.display()
            );
            return;
        }
    };
    let _ = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600));
    let _ = listener.set_nonblocking(true);
    let listener_shared = Arc::clone(shared);
    let handle = thread::spawn(move || loop {
        if listener_shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => serve_connection(stream, &listener_shared),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(_) => return,
        }
    });
    shared.listeners.lock().unwrap().push(handle);
}

enum Flow {
    Continue,
    Close,
}

fn serve_connection(mut stream: UnixStream, shared: &Arc<Shared>) {
    let _ = stream.set_read_timeout(Some(CONNECTION_READ_TIMEOUT));
    let mut buffer = Vec::new();
    loop {
        if shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => {
                buffer.extend_from_slice(&chunk[..read]);
                while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=newline).collect();
                    let raw = String::from_utf8_lossy(&line).to_string();
                    match handle_line(&stream, shared, &raw) {
                        Flow::Continue => {}
                        Flow::Close => {
                            let _ = stream.shutdown(Shutdown::Both);
                            return;
                        }
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => return,
        }
    }
}

fn handle_line(stream: &UnixStream, shared: &Arc<Shared>, raw: &str) -> Flow {
    let Ok(value) = serde_json::from_str::<Value>(raw.trim()) else {
        return Flow::Continue;
    };
    let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    let token = value
        .get("token")
        .and_then(Value::as_str)
        .map(str::to_string);
    let _ = shared.requests_tx.send(FakeRequest {
        id,
        method: method.clone(),
        params: params.clone(),
        token: token.clone(),
        raw: raw.to_string(),
        received_at: Instant::now(),
    });

    let token_valid = {
        let script = shared.script.lock().unwrap();
        match script.expected_token.clone() {
            Some(expected) => token.as_deref() == Some(expected.as_str()),
            None => true,
        }
    };
    if !token_valid {
        write_response(
            stream,
            json!({
                "id": id,
                "ok": false,
                "error": {
                    "code": "permission_denied",
                    "message": "invalid computer-use agent token",
                },
            }),
        );
        return Flow::Continue;
    }

    if method == "terminate" {
        write_response(
            stream,
            json!({"id": id, "ok": true, "result": {"ok": true}}),
        );
        return Flow::Close;
    }

    let behavior = shared.script.lock().unwrap().take();

    match behavior {
        FakeResponse::Echo => {
            write_response(
                stream,
                json!({"id": id, "ok": true, "result": {"method": method, "params": params}}),
            );
            Flow::Continue
        }
        FakeResponse::Result(result) => {
            write_response(stream, json!({"id": id, "ok": true, "result": result}));
            Flow::Continue
        }
        FakeResponse::Delay(duration) => {
            thread::sleep(duration);
            write_response(
                stream,
                json!({"id": id, "ok": true, "result": {"method": method, "params": params}}),
            );
            Flow::Continue
        }
        FakeResponse::Disconnect => Flow::Close,
        FakeResponse::Error { code, message } => {
            write_response(
                stream,
                json!({"id": id, "ok": false, "error": {"code": code, "message": message}}),
            );
            Flow::Continue
        }
        FakeResponse::StaleThen(result) => {
            write_response(
                stream,
                json!({"id": id.wrapping_add(1000), "ok": true, "result": "stale"}),
            );
            write_response(stream, json!({"id": id, "ok": true, "result": result}));
            Flow::Continue
        }
    }
}

fn write_response(stream: &UnixStream, value: Value) {
    let mut writer = stream;
    let mut line = value.to_string();
    line.push('\n');
    let _ = writer.write_all(line.as_bytes());
    let _ = writer.flush();
}
