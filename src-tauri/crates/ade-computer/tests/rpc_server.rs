//! Runtime RPC server anchors (task brief step 1): metadata lifecycle, token
//! admission, bad JSON, oversized frames, `status.get` shape, provider and
//! permissions dispatch, the feature-recording matrix, keepalive on a slow
//! provider and the connection cap. Every test binds a real Unix socket in a
//! temp dir and drives it with the fake provider / permissions api / recorder
//! below; no helper app or real runtime is involved and all limits are
//! injected short.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ade_computer::permissions::{
    OpenSetupResult, PermissionId, PermissionState, PermissionStatus, PermissionStatusEntry,
};
use ade_computer::protocol::{ComputerError, ComputerErrorCode};
use ade_computer::provider::ComputerProvider;
use ade_computer::rpc::{
    feature_id_for_method, FeatureRecorder, PermissionsApi, RpcServer, RpcServerConfig,
    RpcServerHandle,
};
use serde_json::{json, Value};

const RUNTIME_ID: &str = "01JTESTRUNTIME";
const APP_VERSION: &str = "0.0.1-test";

// ---------------------------------------------------------------------------
// temp dir

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ade-computer-rpc-{}-{}",
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

fn mode(path: &Path) -> u32 {
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

// ---------------------------------------------------------------------------
// fake provider

#[derive(Default)]
struct ProviderState {
    calls: Mutex<Vec<(String, Value)>>,
    delay: Mutex<Duration>,
    fail_with: Mutex<Option<(ComputerErrorCode, String)>>,
}

struct FakeProvider {
    state: Arc<ProviderState>,
}

impl FakeProvider {
    fn delay(&self) -> Duration {
        *self.state.delay.lock().unwrap()
    }

    fn failure(&self) -> Option<(ComputerErrorCode, String)> {
        self.state.fail_with.lock().unwrap().clone()
    }
}

impl ComputerProvider for FakeProvider {
    fn capabilities(&mut self) -> Result<Value, ComputerError> {
        self.state
            .calls
            .lock()
            .unwrap()
            .push(("capabilities".to_string(), Value::Null));
        thread::sleep(self.delay());
        if let Some((code, message)) = self.failure() {
            return Err(ComputerError::new(code, message));
        }
        Ok(json!({"protocolVersion": 1}))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ComputerError> {
        self.state
            .calls
            .lock()
            .unwrap()
            .push((method.to_string(), params.clone()));
        thread::sleep(self.delay());
        if let Some((code, message)) = self.failure() {
            return Err(ComputerError::new(code, message));
        }
        Ok(json!({"method": method, "params": params}))
    }
}

// ---------------------------------------------------------------------------
// fake permissions api

#[derive(Default)]
struct FakePermissions {
    status_calls: AtomicUsize,
    open_setup_calls: Mutex<Vec<Option<PermissionId>>>,
}

impl PermissionsApi for FakePermissions {
    fn status(&self) -> PermissionStatus {
        self.status_calls.fetch_add(1, Ordering::SeqCst);
        fixture_status()
    }

    fn open_setup(&self, id: Option<PermissionId>) -> OpenSetupResult {
        self.open_setup_calls.lock().unwrap().push(id);
        fixture_open_setup(id)
    }
}

fn fixture_status() -> PermissionStatus {
    PermissionStatus {
        platform: "darwin".to_string(),
        helper_app_path: Some("/tmp/Orcinus Computer Use.app".to_string()),
        helper_unavailable_reason: None,
        permissions: vec![
            PermissionStatusEntry {
                id: PermissionId::Accessibility,
                status: PermissionState::Granted,
            },
            PermissionStatusEntry {
                id: PermissionId::Screenshots,
                status: PermissionState::NotGranted,
            },
        ],
    }
}

fn fixture_open_setup(id: Option<PermissionId>) -> OpenSetupResult {
    OpenSetupResult {
        platform: "darwin".to_string(),
        helper_app_path: Some("/tmp/Orcinus Computer Use.app".to_string()),
        permission_id: id,
        opened_settings: id.is_some(),
        launched_helper: true,
        permissions: Some(fixture_status().permissions),
        next_step: Some(
            "Grant Screen Recording to Orcinus Computer Use, then retry get-app-state.".to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// fake feature recorder

#[derive(Default)]
struct FakeRecorder {
    recorded: Mutex<Vec<String>>,
}

impl FakeRecorder {
    fn recorded(&self) -> Vec<String> {
        self.recorded.lock().unwrap().clone()
    }
}

impl FeatureRecorder for FakeRecorder {
    fn record(&self, id: &str) {
        self.recorded.lock().unwrap().push(id.to_string());
    }
}

// ---------------------------------------------------------------------------
// NDJSON client

struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl Client {
    fn connect(path: &Path) -> Self {
        let stream = UnixStream::connect(path).expect("connect to runtime socket");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        Self {
            stream,
            buffer: Vec::new(),
        }
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        (&self.stream).write_all(bytes).expect("write request");
        (&self.stream).flush().expect("flush request");
    }

    fn send_request(&mut self, id: &str, token: &str, method: &str, params: Value) {
        let request = json!({
            "id": id,
            "authToken": token,
            "method": method,
            "params": params,
        });
        let mut line = request.to_string();
        line.push('\n');
        self.send_raw(line.as_bytes());
    }

    fn read_frame(&mut self) -> Value {
        loop {
            if let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=position).collect();
                let text = String::from_utf8_lossy(&line[..line.len() - 1]);
                if text.trim().is_empty() {
                    continue;
                }
                return serde_json::from_str(text.trim()).expect("frame is JSON");
            }
            let mut chunk = [0u8; 4096];
            match (&self.stream).read(&mut chunk) {
                Ok(0) => panic!("connection closed before a frame arrived"),
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    panic!("timed out waiting for a frame")
                }
                Err(error) => panic!("read error: {error}"),
            }
        }
    }

    fn read_response(&mut self) -> Value {
        loop {
            let frame = self.read_frame();
            if frame.get("_keepalive") == Some(&Value::Bool(true)) {
                continue;
            }
            return frame;
        }
    }

    fn wait_for_close(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        // Why: macOS rejects SO_RCVTIMEO with EINVAL once the peer closed, so
        // poll in non-blocking mode instead of re-arming a read timeout.
        self.stream.set_nonblocking(true).expect("nonblocking mode");
        loop {
            let mut chunk = [0u8; 256];
            match (&self.stream).read(&mut chunk) {
                Ok(0) => return true,
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return true,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// server harness

struct ServerHarness {
    handle: Option<RpcServerHandle>,
    socket_path: PathBuf,
    metadata_path: PathBuf,
    auth_token: String,
    provider: Arc<ProviderState>,
    permissions: Arc<FakePermissions>,
    recorder: Arc<FakeRecorder>,
    _temp: TempDir,
}

impl ServerHarness {
    fn start() -> Self {
        Self::start_with(|_| {})
    }

    fn start_with(configure: impl FnOnce(&mut RpcServerConfig)) -> Self {
        let temp = TempDir::new();
        let auth_token = "0123456789abcdef".repeat(3);
        let socket_path = temp.0.join("runtime.sock");
        let metadata_path = temp.0.join("runtime.json");
        let mut config =
            RpcServerConfig::limits(RUNTIME_ID, temp.0.clone(), &auth_token, APP_VERSION);
        config.keepalive_interval = Duration::from_secs(5);
        config.idle_timeout = Duration::from_secs(5);
        configure(&mut config);

        let provider_state = Arc::new(ProviderState::default());
        let provider: Arc<Mutex<dyn ComputerProvider>> = Arc::new(Mutex::new(FakeProvider {
            state: Arc::clone(&provider_state),
        }));
        let permissions = Arc::new(FakePermissions::default());
        let recorder = Arc::new(FakeRecorder::default());
        let permissions_api: Arc<dyn PermissionsApi> = permissions.clone();
        let feature_recorder: Arc<dyn FeatureRecorder> = recorder.clone();

        let handle = RpcServer::start(config, provider, permissions_api, feature_recorder)
            .expect("runtime server starts");

        Self {
            handle: Some(handle),
            socket_path,
            metadata_path,
            auth_token,
            provider: provider_state,
            permissions,
            recorder,
            _temp: temp,
        }
    }

    fn connect(&self) -> Client {
        Client::connect(&self.socket_path)
    }

    fn shutdown(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.shutdown();
        }
    }
}

impl Drop for ServerHarness {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// tests

#[test]
fn default_limits_match_the_runtime_contract() {
    let config = RpcServerConfig::limits(RUNTIME_ID, "/tmp/ade-rpc-limits", "token", APP_VERSION);
    assert_eq!(config.max_message_bytes, 1024 * 1024);
    assert_eq!(config.idle_timeout, Duration::from_secs(30));
    assert_eq!(config.max_connections, 32);
    assert_eq!(config.keepalive_interval, Duration::from_secs(10));
}

#[test]
fn start_writes_metadata_and_socket_0600_and_shutdown_removes_both() {
    let mut harness = ServerHarness::start();

    assert_eq!(
        harness.handle.as_ref().unwrap().runtime_id,
        RUNTIME_ID,
        "handle exposes the runtime id"
    );
    assert_eq!(mode(&harness.metadata_path), 0o600, "metadata mode");
    assert_eq!(mode(&harness.socket_path), 0o600, "socket mode");

    let raw = fs::read_to_string(&harness.metadata_path).expect("metadata file");
    let metadata: Value = serde_json::from_str(&raw).expect("metadata json");
    assert_eq!(metadata["runtimeId"], RUNTIME_ID);
    assert_eq!(metadata["pid"].as_u64().unwrap(), std::process::id() as u64);
    assert_eq!(metadata["authToken"], harness.auth_token);
    assert_eq!(
        metadata["transports"],
        json!([{"kind": "unix", "endpoint": harness.socket_path.to_string_lossy()}]),
    );
    let started_at = metadata["startedAt"].as_str().expect("startedAt string");
    assert_eq!(started_at.len(), 24, "startedAt: {started_at}");
    assert!(started_at.ends_with('Z'), "startedAt: {started_at}");
    assert_eq!(&started_at[4..5], "-");
    assert_eq!(&started_at[10..11], "T");

    harness.shutdown();
    assert!(
        !harness.metadata_path.exists(),
        "metadata removed on shutdown"
    );
    assert!(!harness.socket_path.exists(), "socket removed on shutdown");
}

#[test]
fn missing_and_mismatched_tokens_are_unauthorized() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    client.send_raw(b"{\"id\":\"missing\",\"method\":\"status.get\",\"params\":{}}\n");
    let response = client.read_response();
    assert_eq!(response["id"], "missing");
    assert!(!response["ok"].as_bool().unwrap());
    assert_eq!(response["error"]["code"], "unauthorized");
    assert_eq!(response["error"]["message"], "Missing auth token");

    client.send_request("mismatch", "not-the-token", "status.get", json!({}));
    let response = client.read_response();
    assert_eq!(response["id"], "mismatch");
    assert_eq!(response["error"]["code"], "unauthorized");
    assert_eq!(response["error"]["message"], "Invalid auth token");

    client.send_request("valid", &harness.auth_token, "status.get", json!({}));
    let response = client.read_response();
    assert!(
        response["ok"].as_bool().unwrap(),
        "admission failures must not close the connection: {response}"
    );
    assert_eq!(response["id"], "valid");
    assert_eq!(response["_meta"]["runtimeId"], RUNTIME_ID);

    harness.shutdown();
}

#[test]
fn bad_json_and_missing_fields_are_bad_request() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    client.send_raw(b"this is not json\n");
    let response = client.read_response();
    assert_eq!(response["id"], "unknown");
    assert!(!response["ok"].as_bool().unwrap());
    assert_eq!(response["error"]["code"], "bad_request");
    assert_eq!(response["error"]["message"], "Invalid JSON request");

    let no_id = format!(
        "{{\"authToken\":\"{}\",\"method\":\"status.get\"}}\n",
        harness.auth_token
    );
    client.send_raw(no_id.as_bytes());
    let response = client.read_response();
    assert_eq!(response["id"], "unknown");
    assert_eq!(response["error"]["code"], "bad_request");
    assert_eq!(response["error"]["message"], "Missing request id");

    let no_method = format!(
        "{{\"id\":\"m\",\"authToken\":\"{}\"}}\n",
        harness.auth_token
    );
    client.send_raw(no_method.as_bytes());
    let response = client.read_response();
    assert_eq!(response["id"], "m");
    assert_eq!(response["error"]["code"], "bad_request");
    assert_eq!(response["error"]["message"], "Missing RPC method");

    harness.shutdown();
}

#[test]
fn oversized_lines_are_request_too_large_and_close_the_connection() {
    let mut harness = ServerHarness::start_with(|config| config.max_message_bytes = 1024);
    let mut client = harness.connect();

    let huge = "x".repeat(4096);
    client.send_request(
        "big",
        &harness.auth_token,
        "computer.typeText",
        json!({"text": huge}),
    );
    let response = client.read_response();
    assert_eq!(response["id"], "unknown");
    assert!(!response["ok"].as_bool().unwrap());
    assert_eq!(response["error"]["code"], "request_too_large");
    assert!(
        client.wait_for_close(Duration::from_secs(2)),
        "oversized connections must be closed"
    );

    harness.shutdown();
}

#[test]
fn status_get_reports_app_runtime_and_placeholder_graph() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    client.send_request("s1", &harness.auth_token, "status.get", json!({}));
    let response = client.read_response();
    assert_eq!(response["id"], "s1");
    assert!(response["ok"].as_bool().unwrap());
    assert_eq!(response["_meta"], json!({"runtimeId": RUNTIME_ID}));
    assert_eq!(
        response["result"],
        json!({
            "app": {"running": true, "pid": std::process::id(), "desktopWindowStatus": "available"},
            "runtime": {
                "state": "ready",
                "reachable": true,
                "runtimeId": RUNTIME_ID,
                "appVersion": APP_VERSION,
            },
            "graph": {"state": "not_ready"},
        })
    );
    assert!(
        harness.provider.calls.lock().unwrap().is_empty(),
        "status.get must not touch the provider"
    );
    assert_eq!(harness.permissions.status_calls.load(Ordering::SeqCst), 0);
    assert!(
        harness.recorder.recorded().is_empty(),
        "status.get must not record a feature"
    );

    harness.shutdown();
}

#[test]
fn computer_get_app_state_is_forwarded_to_the_provider() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    client.send_request(
        "d1",
        &harness.auth_token,
        "computer.getAppState",
        json!({"app": "Finder"}),
    );
    let response = client.read_response();
    assert_eq!(response["id"], "d1");
    assert!(response["ok"].as_bool().unwrap());
    assert_eq!(
        response["result"],
        json!({"method": "computer.getAppState", "params": {"app": "Finder"}})
    );
    assert_eq!(
        harness.provider.calls.lock().unwrap().clone(),
        vec![("computer.getAppState".to_string(), json!({"app": "Finder"}))]
    );

    harness.shutdown();
}

#[test]
fn computer_permissions_status_uses_the_permissions_api() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    client.send_request(
        "p1",
        &harness.auth_token,
        "computer.permissionsStatus",
        json!({}),
    );
    let response = client.read_response();
    assert!(response["ok"].as_bool().unwrap());
    assert_eq!(
        response["result"],
        serde_json::to_value(fixture_status()).unwrap()
    );
    assert_eq!(harness.permissions.status_calls.load(Ordering::SeqCst), 1);
    assert!(
        harness.recorder.recorded().is_empty(),
        "permissionsStatus must not record a feature"
    );

    harness.shutdown();
}

#[test]
fn computer_permissions_calls_open_setup_with_the_requested_id() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    client.send_request(
        "p2",
        &harness.auth_token,
        "computer.permissions",
        json!({"id": "accessibility"}),
    );
    let response = client.read_response();
    assert!(response["ok"].as_bool().unwrap());
    assert_eq!(
        response["result"],
        serde_json::to_value(fixture_open_setup(Some(PermissionId::Accessibility))).unwrap()
    );
    assert_eq!(
        harness.permissions.open_setup_calls.lock().unwrap().clone(),
        vec![Some(PermissionId::Accessibility)]
    );
    assert_eq!(harness.recorder.recorded(), vec!["computer-use-setup"]);

    client.send_request(
        "p3",
        &harness.auth_token,
        "computer.permissions",
        json!({"id": "camera"}),
    );
    let response = client.read_response();
    assert_eq!(response["error"]["code"], "invalid_argument");
    assert_eq!(
        harness.permissions.open_setup_calls.lock().unwrap().len(),
        1,
        "an invalid id must not reach the permissions api"
    );

    harness.shutdown();
}

#[test]
fn feature_ids_follow_the_method_matrix() {
    assert_eq!(
        feature_id_for_method("computer.click"),
        Some("computer-use")
    );
    assert_eq!(
        feature_id_for_method("computer.typeText"),
        Some("computer-use")
    );
    assert_eq!(
        feature_id_for_method("computer.permissions"),
        Some("computer-use-setup")
    );
    assert_eq!(feature_id_for_method("computer.capabilities"), None);
    assert_eq!(feature_id_for_method("computer.permissionsStatus"), None);
    assert_eq!(feature_id_for_method("status.get"), None);
    assert_eq!(feature_id_for_method("skills.get"), None);
}

#[test]
fn feature_recording_follows_the_dispatched_method_matrix() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    for (id, method, params) in [
        ("f1", "computer.getAppState", json!({"app": "Finder"})),
        (
            "f2",
            "computer.click",
            json!({"app": "Finder", "elementIndex": 0}),
        ),
        ("f3", "computer.permissions", json!({})),
        ("f4", "computer.capabilities", json!({})),
        ("f5", "computer.permissionsStatus", json!({})),
        ("f6", "status.get", json!({})),
    ] {
        client.send_request(id, &harness.auth_token, method, params);
        let response = client.read_response();
        assert!(
            response["ok"].as_bool().unwrap(),
            "{method} must succeed: {response}"
        );
    }
    assert_eq!(
        harness.recorder.recorded(),
        vec!["computer-use", "computer-use", "computer-use-setup"]
    );

    harness.shutdown();
}

#[test]
fn provider_errors_pass_through_and_still_record_the_feature() {
    let mut harness = ServerHarness::start();
    *harness.provider.fail_with.lock().unwrap() = Some((
        ComputerErrorCode::PermissionDenied,
        "no accessibility permission".to_string(),
    ));
    let mut client = harness.connect();

    client.send_request(
        "e1",
        &harness.auth_token,
        "computer.capabilities",
        json!({}),
    );
    let response = client.read_response();
    assert!(!response["ok"].as_bool().unwrap());
    assert_eq!(response["error"]["code"], "permission_denied");
    assert_eq!(response["error"]["message"], "no accessibility permission");
    assert_eq!(response["_meta"]["runtimeId"], RUNTIME_ID);

    client.send_request(
        "e2",
        &harness.auth_token,
        "computer.click",
        json!({"app": "Finder", "elementIndex": 0}),
    );
    let response = client.read_response();
    assert!(!response["ok"].as_bool().unwrap());
    assert_eq!(response["error"]["code"], "permission_denied");
    assert_eq!(
        harness.recorder.recorded(),
        vec!["computer-use"],
        "failures of action methods still record the feature"
    );

    harness.shutdown();
}

#[test]
fn unknown_methods_are_method_not_found() {
    let mut harness = ServerHarness::start();
    let mut client = harness.connect();

    client.send_request(
        "u1",
        &harness.auth_token,
        "computer.unknownThing",
        json!({}),
    );
    let response = client.read_response();
    assert!(!response["ok"].as_bool().unwrap());
    assert_eq!(response["error"]["code"], "method_not_found");
    assert_eq!(
        response["error"]["message"],
        "Unknown method: computer.unknownThing"
    );

    client.send_request("u2", &harness.auth_token, "skills.get", json!({}));
    let response = client.read_response();
    assert_eq!(response["error"]["code"], "method_not_found");

    assert!(
        harness.provider.calls.lock().unwrap().is_empty(),
        "unknown methods must not reach the provider"
    );

    harness.shutdown();
}

#[test]
fn slow_requests_emit_keepalive_frames_before_the_result() {
    let mut harness = ServerHarness::start_with(|config| {
        config.keepalive_interval = Duration::from_millis(50);
    });
    *harness.provider.delay.lock().unwrap() = Duration::from_millis(200);
    let mut client = harness.connect();

    client.send_request(
        "k1",
        &harness.auth_token,
        "computer.getAppState",
        json!({"app": "Finder"}),
    );

    let first = client.read_frame();
    assert_eq!(
        first,
        json!({"_keepalive": true}),
        "the first frame must be a keepalive while the provider is slow"
    );

    let response = client.read_response();
    assert_eq!(response["id"], "k1");
    assert!(response["ok"].as_bool().unwrap());
    assert_eq!(
        response["result"],
        json!({"method": "computer.getAppState", "params": {"app": "Finder"}})
    );

    harness.shutdown();
}

#[test]
fn connections_beyond_the_cap_get_runtime_busy() {
    let mut harness = ServerHarness::start_with(|config| config.max_connections = 2);
    let mut first = harness.connect();
    let mut second = harness.connect();

    for (client, id) in [(&mut first, "c1"), (&mut second, "c2")] {
        client.send_request(id, &harness.auth_token, "status.get", json!({}));
        let response = client.read_response();
        assert!(
            response["ok"].as_bool().unwrap(),
            "connection {id} must be admitted: {response}"
        );
    }

    let mut third = harness.connect();
    let response = third.read_frame();
    assert_eq!(response["id"], "unknown");
    assert!(!response["ok"].as_bool().unwrap());
    assert_eq!(response["error"]["code"], "runtime_busy");
    assert!(
        third.wait_for_close(Duration::from_secs(2)),
        "a rejected connection must be closed"
    );

    first.send_request("c1b", &harness.auth_token, "status.get", json!({}));
    let response = first.read_response();
    assert!(
        response["ok"].as_bool().unwrap(),
        "a rejected connection must not affect live ones: {response}"
    );

    drop(second);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut probe = harness.connect();
        probe.send_request("probe", &harness.auth_token, "status.get", json!({}));
        let response = probe.read_response();
        if response["ok"] == Value::Bool(true) {
            break;
        }
        assert_eq!(response["error"]["code"], "runtime_busy");
        assert!(
            Instant::now() < deadline,
            "a freed connection slot must be reusable"
        );
        thread::sleep(Duration::from_millis(20));
    }

    harness.shutdown();
}

#[test]
fn idle_connections_are_closed_after_the_idle_timeout() {
    let mut harness = ServerHarness::start_with(|config| {
        config.idle_timeout = Duration::from_millis(150);
    });
    let mut client = harness.connect();

    client.send_request("i1", &harness.auth_token, "status.get", json!({}));
    let response = client.read_response();
    assert!(response["ok"].as_bool().unwrap());

    assert!(
        client.wait_for_close(Duration::from_secs(2)),
        "idle connections must be closed"
    );

    harness.shutdown();
}
