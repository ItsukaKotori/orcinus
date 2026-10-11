//! Runtime RPC server (spec §3.5): the NDJSON Unix-socket endpoint the app
//! serves to the `orcinus` CLI. Envelope shapes, admission error codes
//! (`bad_request`, `unauthorized`, `request_too_large`, `runtime_busy`) and
//! the `{"_keepalive":true}` frame mirror
//! `orca:src/main/runtime/rpc/*` and `src/shared/runtime-rpc-envelope.ts`;
//! `runtime.json` bootstrap fields mirror `shared/runtime-bootstrap.ts`.
//!
//! The server is pure Rust with injectable limits so tests never wait on the
//! production values (1 MB frames, 30 s idle, 32 connections, 10 s keepalive).

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::permissions::{OpenSetupResult, PermissionId, PermissionStatus};
use crate::protocol::{
    ComputerError, RuntimeMetadata, RuntimeTransport, COMPUTER_RPC_METHODS, RUNTIME_METADATA_FILE,
    RUNTIME_SOCKET_FILE,
};
use crate::provider::ComputerProvider;

/// Default frame cap: requests longer than this are `request_too_large`.
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Default idle timeout: a connection without any data is dropped.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Default connection cap (oracle `MAX_RUNTIME_RPC_CONNECTIONS`).
pub const DEFAULT_MAX_CONNECTIONS: usize = 32;
/// Default keepalive cadence for slow dispatches (`{"_keepalive":true}`).
pub const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// Accept-loop poll cadence (the listener is non-blocking).
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(2);
/// Idle/connection poll cadence, capped by the configured idle timeout.
const CONNECTION_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Shutdown/keepalive poll cadence while a dispatch is pending.
const DISPATCH_POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Bound on a single frame write so a stuck client cannot wedge a worker.
const CONNECTION_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Feature-interaction sink (spec §3.10). The app wires this to
/// `UiStateStore::record_feature_interaction_in_memory`.
pub trait FeatureRecorder: Send + Sync {
    fn record(&self, id: &str);
}

/// Map one served RPC method to the feature-interaction id it records:
/// `computer.permissions` → `computer-use-setup`; the rest of the
/// `computer.*` payload surface (except `computer.capabilities` and
/// `computer.permissionsStatus`) → `computer-use`; everything else → none.
pub fn feature_id_for_method(method: &str) -> Option<&'static str> {
    if !method.starts_with("computer.") {
        return None;
    }
    match method {
        "computer.capabilities" | "computer.permissionsStatus" => None,
        "computer.permissions" => Some("computer-use-setup"),
        _ => Some("computer-use"),
    }
}

/// TCC permission surface consumed by `computer.permissionsStatus` and
/// `computer.permissions` (implemented by `ade-bridge` over the permissions
/// module).
pub trait PermissionsApi: Send + Sync {
    fn status(&self) -> PermissionStatus;
    fn open_setup(&self, id: Option<PermissionId>) -> OpenSetupResult;
}

/// Runtime RPC server configuration. [`RpcServerConfig::limits`] fills the
/// production boundaries; tests shorten every duration / cap.
#[derive(Debug, Clone)]
pub struct RpcServerConfig {
    pub runtime_id: String,
    pub user_data_dir: PathBuf,
    pub auth_token: String,
    pub app_version: String,
    pub max_message_bytes: usize,
    pub idle_timeout: Duration,
    pub max_connections: usize,
    pub keepalive_interval: Duration,
}

impl RpcServerConfig {
    /// Identity fields plus the production limits (1 MB / 30 s / 32 / 10 s).
    pub fn limits(
        runtime_id: impl Into<String>,
        user_data_dir: impl Into<PathBuf>,
        auth_token: impl Into<String>,
        app_version: impl Into<String>,
    ) -> Self {
        Self {
            runtime_id: runtime_id.into(),
            user_data_dir: user_data_dir.into(),
            auth_token: auth_token.into(),
            app_version: app_version.into(),
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            keepalive_interval: DEFAULT_KEEPALIVE_INTERVAL,
        }
    }

    pub fn socket_path(&self) -> PathBuf {
        self.user_data_dir.join(RUNTIME_SOCKET_FILE)
    }

    pub fn metadata_path(&self) -> PathBuf {
        self.user_data_dir.join(RUNTIME_METADATA_FILE)
    }
}

/// Handle returned by [`RpcServer::start`]. [`RpcServerHandle::shutdown`]
/// stops accepting, joins the workers and removes both socket and metadata.
pub struct RpcServerHandle {
    pub runtime_id: String,
    shared: Arc<ServerShared>,
    accept_thread: Option<JoinHandle<()>>,
}

impl RpcServerHandle {
    pub fn shutdown(mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        if let Some(accept_thread) = self.accept_thread.take() {
            let _ = accept_thread.join();
        }
        let streams: Vec<UnixStream> = {
            let mut streams = self
                .shared
                .streams
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            streams.drain().map(|(_, stream)| stream).collect()
        };
        for stream in streams {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let workers: Vec<JoinHandle<()>> = {
            let mut workers = self
                .shared
                .workers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *workers)
        };
        for worker in workers {
            let _ = worker.join();
        }
        let _ = fs::remove_file(self.shared.config.socket_path());
        let _ = fs::remove_file(self.shared.config.metadata_path());
    }
}

/// Runtime RPC server entry point.
pub struct RpcServer;

impl RpcServer {
    /// Write `runtime.json` (0600), bind `runtime.sock` (0600) and start the
    /// non-blocking accept loop.
    pub fn start(
        config: RpcServerConfig,
        provider: Arc<Mutex<dyn ComputerProvider>>,
        permissions: Arc<dyn PermissionsApi>,
        recorder: Arc<dyn FeatureRecorder>,
    ) -> io::Result<RpcServerHandle> {
        fs::create_dir_all(&config.user_data_dir)?;
        let socket_path = config.socket_path();
        let metadata_path = config.metadata_path();
        write_metadata(&metadata_path, &config, &socket_path)?;

        let listener = match bind_socket(&socket_path) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = fs::remove_file(&metadata_path);
                return Err(error);
            }
        };

        let shared = Arc::new(ServerShared {
            config,
            provider,
            permissions,
            recorder,
            shutdown: AtomicBool::new(false),
            active_connections: AtomicUsize::new(0),
            next_connection_id: AtomicUsize::new(0),
            streams: Mutex::new(HashMap::new()),
            workers: Mutex::new(Vec::new()),
        });
        let runtime_id = shared.config.runtime_id.clone();
        let accept_thread = thread::spawn({
            let shared = Arc::clone(&shared);
            move || accept_loop(listener, shared)
        });
        Ok(RpcServerHandle {
            runtime_id,
            shared,
            accept_thread: Some(accept_thread),
        })
    }
}

struct ServerShared {
    config: RpcServerConfig,
    provider: Arc<Mutex<dyn ComputerProvider>>,
    permissions: Arc<dyn PermissionsApi>,
    recorder: Arc<dyn FeatureRecorder>,
    shutdown: AtomicBool,
    active_connections: AtomicUsize,
    next_connection_id: AtomicUsize,
    streams: Mutex<HashMap<usize, UnixStream>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl ServerShared {
    fn provider(&self) -> std::sync::MutexGuard<'_, dyn ComputerProvider + 'static> {
        self.provider
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

struct ParsedRequest {
    id: String,
    method: String,
    params: Value,
}

struct AdmissionError {
    id: String,
    code: &'static str,
    message: &'static str,
}

struct ConnectionCleanup {
    shared: Arc<ServerShared>,
    connection_id: usize,
}

impl Drop for ConnectionCleanup {
    fn drop(&mut self) {
        let mut streams = self
            .shared
            .streams
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        streams.remove(&self.connection_id);
        self.shared
            .active_connections
            .fetch_sub(1, Ordering::SeqCst);
    }
}

fn write_metadata(
    metadata_path: &Path,
    config: &RpcServerConfig,
    socket_path: &Path,
) -> io::Result<()> {
    let metadata = RuntimeMetadata {
        runtime_id: config.runtime_id.clone(),
        pid: std::process::id(),
        transports: vec![RuntimeTransport::Unix {
            endpoint: socket_path.to_string_lossy().into_owned(),
        }],
        auth_token: config.auth_token.clone(),
        started_at: iso8601_utc(SystemTime::now()),
    };
    let bytes = serde_json::to_vec(&metadata)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(metadata_path)?;
    file.write_all(&bytes)?;
    drop(file);
    fs::set_permissions(metadata_path, fs::Permissions::from_mode(0o600))
}

fn bind_socket(socket_path: &Path) -> io::Result<UnixListener> {
    if socket_path.exists() {
        let _ = fs::remove_file(socket_path);
    }
    let listener = UnixListener::bind(socket_path)?;
    let result = fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))
        .and_then(|()| listener.set_nonblocking(true));
    match result {
        Ok(()) => Ok(listener),
        Err(error) => {
            drop(listener);
            let _ = fs::remove_file(socket_path);
            Err(error)
        }
    }
}

fn accept_loop(listener: UnixListener, shared: Arc<ServerShared>) {
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if shared.shutdown.load(Ordering::SeqCst) {
                    return;
                }
                let active = shared.active_connections.fetch_add(1, Ordering::SeqCst);
                if active >= shared.config.max_connections {
                    shared.active_connections.fetch_sub(1, Ordering::SeqCst);
                    reject_busy(&stream, &shared);
                    continue;
                }
                spawn_connection(stream, Arc::clone(&shared));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => thread::sleep(ACCEPT_POLL_INTERVAL),
        }
    }
}

fn reject_busy(stream: &UnixStream, shared: &ServerShared) {
    let frame = failure_frame(
        "unknown",
        shared,
        "runtime_busy",
        "RPC connection capacity reached; retry with backoff",
    );
    let _ = write_frame(stream, &frame);
}

fn spawn_connection(stream: UnixStream, shared: Arc<ServerShared>) {
    let connection_id = shared.next_connection_id.fetch_add(1, Ordering::SeqCst);
    if let Ok(tracked) = stream.try_clone() {
        let mut streams = shared
            .streams
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        streams.insert(connection_id, tracked);
    }
    let worker = thread::spawn({
        let shared = Arc::clone(&shared);
        move || run_connection(stream, shared, connection_id)
    });
    let mut workers = shared
        .workers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    workers.push(worker);
}

fn run_connection(mut stream: UnixStream, shared: Arc<ServerShared>, connection_id: usize) {
    let _cleanup = ConnectionCleanup {
        shared: Arc::clone(&shared),
        connection_id,
    };
    let _ = stream.set_write_timeout(Some(CONNECTION_WRITE_TIMEOUT));
    let poll = shared
        .config
        .idle_timeout
        .min(CONNECTION_POLL_INTERVAL)
        .max(Duration::from_millis(1));
    let _ = stream.set_read_timeout(Some(poll));

    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut last_activity = Instant::now();
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            return;
        }
        if last_activity.elapsed() >= shared.config.idle_timeout {
            return;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => {
                pending.extend_from_slice(&chunk[..read]);
                if !process_pending(&stream, &mut pending, &shared) {
                    return;
                }
                // Why: a long dispatch must not count as idle time afterwards.
                last_activity = Instant::now();
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return,
        }
    }
}

/// Slice complete lines out of `pending` and answer each one. Returns `false`
/// when the connection must be closed (oversized frame, write failure,
/// shutdown).
fn process_pending(stream: &UnixStream, pending: &mut Vec<u8>, shared: &Arc<ServerShared>) -> bool {
    while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
        if newline > shared.config.max_message_bytes {
            return send_too_large(stream, shared);
        }
        let line: Vec<u8> = pending.drain(..=newline).collect();
        let raw = trim_ascii(&line[..line.len() - 1]);
        if raw.is_empty() {
            continue;
        }
        if !handle_message(stream, raw, shared) {
            return false;
        }
    }
    if pending.len() > shared.config.max_message_bytes {
        return send_too_large(stream, shared);
    }
    true
}

fn send_too_large(stream: &UnixStream, shared: &Arc<ServerShared>) -> bool {
    let frame = failure_frame(
        "unknown",
        shared,
        "request_too_large",
        "RPC request exceeds the maximum size",
    );
    let _ = write_frame(stream, &frame);
    false
}

fn handle_message(stream: &UnixStream, raw: &[u8], shared: &Arc<ServerShared>) -> bool {
    match parse_request(raw, &shared.config.auth_token) {
        Err(error) => {
            let frame = failure_frame(&error.id, shared, error.code, error.message);
            write_frame(stream, &frame).is_ok()
        }
        Ok(request) => match dispatch_and_wait(&request, stream, shared) {
            Some(response) => write_frame(stream, &response).is_ok(),
            None => false,
        },
    }
}

fn parse_request(raw: &[u8], auth_token: &str) -> Result<ParsedRequest, AdmissionError> {
    let value: Value = match serde_json::from_slice(raw) {
        Ok(value) => value,
        Err(_) => {
            return Err(AdmissionError {
                id: "unknown".to_string(),
                code: "bad_request",
                message: "Invalid JSON request",
            })
        }
    };
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return Err(AdmissionError {
            id: "unknown".to_string(),
            code: "bad_request",
            message: "Missing request id",
        });
    };
    if id.is_empty() {
        return Err(AdmissionError {
            id: "unknown".to_string(),
            code: "bad_request",
            message: "Missing request id",
        });
    }
    let id = id.to_string();
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return Err(AdmissionError {
            id,
            code: "bad_request",
            message: "Missing RPC method",
        });
    };
    if method.is_empty() {
        return Err(AdmissionError {
            id,
            code: "bad_request",
            message: "Missing RPC method",
        });
    }
    let Some(token) = value.get("authToken").and_then(Value::as_str) else {
        return Err(AdmissionError {
            id,
            code: "unauthorized",
            message: "Missing auth token",
        });
    };
    if token.is_empty() {
        return Err(AdmissionError {
            id,
            code: "unauthorized",
            message: "Missing auth token",
        });
    }
    if token != auth_token {
        return Err(AdmissionError {
            id,
            code: "unauthorized",
            message: "Invalid auth token",
        });
    }
    Ok(ParsedRequest {
        id,
        method: method.to_string(),
        params: value.get("params").cloned().unwrap_or_else(|| json!({})),
    })
}

/// Run the dispatch on a helper thread so keepalive frames can be written
/// while the provider works; returns `None` when the server shuts down first.
fn dispatch_and_wait(
    request: &ParsedRequest,
    stream: &UnixStream,
    shared: &Arc<ServerShared>,
) -> Option<Value> {
    let (sender, receiver) = mpsc::channel();
    let dispatch_shared = Arc::clone(shared);
    let dispatch_request = ParsedRequest {
        id: request.id.clone(),
        method: request.method.clone(),
        params: request.params.clone(),
    };
    thread::spawn(move || {
        let response = dispatch(&dispatch_request, &dispatch_shared);
        let _ = sender.send(response);
    });

    let poll = shared
        .config
        .keepalive_interval
        .min(DISPATCH_POLL_INTERVAL)
        .max(Duration::from_millis(1));
    let mut last_keepalive = Instant::now();
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            return None;
        }
        match receiver.recv_timeout(poll) {
            Ok(response) => return Some(response),
            Err(RecvTimeoutError::Timeout) => {
                if last_keepalive.elapsed() >= shared.config.keepalive_interval {
                    last_keepalive = Instant::now();
                    let _ = write_frame(stream, &json!({"_keepalive": true}));
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Some(failure_frame(
                    &request.id,
                    shared,
                    "runtime_error",
                    "RPC dispatch thread terminated unexpectedly",
                ))
            }
        }
    }
}

fn dispatch(request: &ParsedRequest, shared: &ServerShared) -> Value {
    let method = request.method.as_str();
    let mut feature = None;
    let response = match method {
        "status.get" => success_frame(&request.id, shared, status_result(shared)),
        "computer.capabilities" => {
            feature = feature_id_for_method(method);
            match shared.provider().capabilities() {
                Ok(result) => success_frame(&request.id, shared, result),
                Err(error) => computer_failure_frame(&request.id, shared, &error),
            }
        }
        "computer.permissionsStatus" => {
            feature = feature_id_for_method(method);
            let result = serde_json::to_value(shared.permissions.status())
                .expect("permission status serializes");
            success_frame(&request.id, shared, result)
        }
        "computer.permissions" => {
            feature = feature_id_for_method(method);
            match permission_id_from_params(&request.params) {
                Ok(permission_id) => {
                    let result = serde_json::to_value(shared.permissions.open_setup(permission_id))
                        .expect("open setup result serializes");
                    success_frame(&request.id, shared, result)
                }
                Err(message) => failure_frame(&request.id, shared, "invalid_argument", &message),
            }
        }
        known if COMPUTER_RPC_METHODS.contains(&known) => {
            feature = feature_id_for_method(method);
            match shared.provider().request(known, request.params.clone()) {
                Ok(result) => success_frame(&request.id, shared, result),
                Err(error) => computer_failure_frame(&request.id, shared, &error),
            }
        }
        _ => failure_frame(
            &request.id,
            shared,
            "method_not_found",
            &format!("Unknown method: {method}"),
        ),
    };
    if let Some(feature) = feature {
        shared.recorder.record(feature);
    }
    response
}

fn permission_id_from_params(params: &Value) -> Result<Option<PermissionId>, String> {
    match params.get("id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if id == "accessibility" => Ok(Some(PermissionId::Accessibility)),
        Some(Value::String(id)) if id == "screenshots" => Ok(Some(PermissionId::Screenshots)),
        Some(other) => Err(format!(
            "Invalid enum value. Expected 'accessibility' | 'screenshots', received {other}"
        )),
    }
}

fn status_result(shared: &ServerShared) -> Value {
    json!({
        "app": {
            "running": true,
            "pid": std::process::id(),
            "desktopWindowStatus": "available",
        },
        "runtime": {
            "state": "ready",
            "reachable": true,
            "runtimeId": shared.config.runtime_id,
            "appVersion": shared.config.app_version,
        },
        "graph": {"state": "not_ready"},
    })
}

fn success_frame(id: &str, shared: &ServerShared, result: Value) -> Value {
    json!({
        "id": id,
        "ok": true,
        "result": result,
        "_meta": {"runtimeId": shared.config.runtime_id},
    })
}

fn failure_frame(id: &str, shared: &ServerShared, code: &str, message: &str) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": {"code": code, "message": message},
        "_meta": {"runtimeId": shared.config.runtime_id},
    })
}

fn computer_failure_frame(id: &str, shared: &ServerShared, error: &ComputerError) -> Value {
    let mut error_value = json!({
        "code": error.code.as_str(),
        "message": error.message,
    });
    if let Some(data) = &error.data {
        error_value["data"] = data.clone();
    }
    json!({
        "id": id,
        "ok": false,
        "error": error_value,
        "_meta": {"runtimeId": shared.config.runtime_id},
    })
}

fn write_frame(stream: &UnixStream, frame: &Value) -> io::Result<()> {
    let mut line = serde_json::to_vec(frame)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    line.push(b'\n');
    let mut writer = stream;
    writer.write_all(&line)?;
    writer.flush()
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |position| position + 1);
    &bytes[start..end]
}

/// UTC ISO-8601 with millisecond precision (`2026-10-11T10:00:00.000Z`),
/// matching the oracle `startedAt` format without pulling in a time crate.
fn iso8601_utc(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs() as i64;
    let millis = duration.subsec_millis();
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    )
}

/// Howard Hinnant's `civil_from_days` (days since the Unix epoch → y/m/d).
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_formats_epoch_and_known_instants() {
        assert_eq!(iso8601_utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso8601_utc(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123)),
            "2023-11-14T22:13:20.123Z"
        );
    }

    #[test]
    fn feature_ids_keep_the_computer_use_literals() {
        assert_eq!(
            feature_id_for_method("computer.performSecondaryAction"),
            Some("computer-use")
        );
        assert_eq!(
            feature_id_for_method("computer.permissions"),
            Some("computer-use-setup")
        );
        assert_eq!(feature_id_for_method("computer.capabilities"), None);
    }
}
