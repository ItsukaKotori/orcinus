use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use base64::Engine;
use serde_json::Value;

use crate::cache::{now_ms, CachedHookEvent, StatusCache};
use crate::endpoint::{self, EndpointFields};
use crate::installer::{self, HookInstallState};

pub const HOOK_REQUEST_MAX_BYTES: usize = 1_000_000;
pub const HOOK_REQUEST_SLOWLORIS: Duration = Duration::from_secs(5);
const MAX_HEADER_BYTES: usize = 64 * 1024;
const ACCEPT_POLL: Duration = Duration::from_millis(10);

pub type HookCallback = Box<dyn Fn(CachedHookEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct StartOptions {
    pub app_data_dir: PathBuf,
    pub home: String,
    pub env: String,
    pub install_enabled: bool,
}

pub struct AgentHookServer {
    port: u16,
    token: String,
    env: String,
    endpoint_path: PathBuf,
    pty_env_map: HashMap<String, String>,
    cache: Arc<StatusCache>,
    callback: Arc<dyn Fn(CachedHookEvent) + Send + Sync>,
    stop: Arc<AtomicBool>,
    accept_thread: Mutex<Option<JoinHandle<()>>>,
    install_state: Mutex<HookInstallState>,
    active: bool,
}

enum RequestError {
    Malformed,
    TooLarge,
    Timeout,
}

type SharedCallback = Arc<dyn Fn(CachedHookEvent) + Send + Sync>;

impl AgentHookServer {
    pub fn start(options: StartOptions, callback: HookCallback) -> Arc<Self> {
        let token = ade_core::ids::new_uuid();
        let cache = Arc::new(StatusCache::load(options.app_data_dir.join("last-status.json")));
        let callback: SharedCallback = Arc::from(callback);
        let endpoint_path = options.app_data_dir.join(endpoint::ENDPOINT_FILE_NAME);

        // 启动序（规格 §3.1）：hydrate（在 load 内）→ spool 重放 → settings
        // reconcile → bind。spool 重放先于 bind，避免与实时 POST 竞争。
        drain_spool(&options.app_data_dir.join("spool"), &cache, &callback);

        // 安装策略（规格 §3.3/§4 裁定）：启动关闭 = skip 不删；开启 = 安装/更新。
        let install_state = if options.install_enabled {
            let cli_present = installer::is_claude_cli_available(&options.home);
            installer::install_claude_hooks(&options.home, true, cli_present)
        } else {
            HookInstallState::Skipped(installer::HookInstallSkipReason::HooksDisabled)
        };

        let listener = bind_with_retry(3);
        let (port, active) = match listener.as_ref() {
            Some(listener) => {
                let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
                (port, true)
            }
            None => {
                eprintln!("[ade-hooks] failed to bind loopback after 3 attempts; hook server degraded (2A transcript fallback stays in charge)");
                (0, false)
            }
        };
        let fields = EndpointFields {
            port,
            token: token.clone(),
            env: options.env.clone(),
        };
        let endpoint_written = active
            && endpoint::write_endpoint_file(&options.app_data_dir, &fields).unwrap_or(false);
        let pty_env_map = if endpoint_written {
            endpoint::pty_env(&fields, &endpoint_path)
        } else {
            HashMap::new()
        };

        let server = Arc::new(AgentHookServer {
            port,
            token,
            env: options.env.clone(),
            endpoint_path,
            pty_env_map,
            cache,
            callback,
            stop: Arc::new(AtomicBool::new(false)),
            accept_thread: Mutex::new(None),
            install_state: Mutex::new(install_state),
            active,
        });

        if let Some(listener) = listener {
            let accept_server = Arc::clone(&server);
            let handle = thread::spawn(move || {
                listener
                    .set_nonblocking(true)
                    .expect("set nonblocking listener");
                while !accept_server.stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _addr)) => {
                            let connection_server = Arc::clone(&accept_server);
                            thread::spawn(move || {
                                let _ = connection_server.handle_connection(stream);
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(ACCEPT_POLL);
                        }
                        Err(error) => {
                            eprintln!("[ade-hooks] accept failed: {error}");
                            thread::sleep(ACCEPT_POLL);
                        }
                    }
                }
            });
            *server.accept_thread.lock().unwrap() = Some(handle);
        }
        server
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn env(&self) -> &str {
        &self.env
    }

    pub fn endpoint_path(&self) -> &Path {
        &self.endpoint_path
    }

    pub fn pty_env(&self) -> HashMap<String, String> {
        self.pty_env_map.clone()
    }

    pub fn snapshot(&self) -> Vec<CachedHookEvent> {
        self.cache.snapshot()
    }

    pub fn install_state(&self) -> HookInstallState {
        self.install_state.lock().unwrap().clone()
    }

    /// 显式开关切换（settings 写路径）：开 = 安装/更新；关 = 移除托管条目。
    pub fn set_hooks_enabled(&self, enabled: bool, home: &str) {
        let next = if enabled {
            let cli_present = installer::is_claude_cli_available(home);
            installer::install_claude_hooks(home, true, cli_present)
        } else {
            installer::remove_claude_hooks(home)
        };
        *self.install_state.lock().unwrap() = next;
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.cache.shutdown();
    }

    fn handle_connection(&self, stream: TcpStream) -> Result<(), RequestError> {
        let mut stream = stream;
        // BSD/darwin 的 accept 会继承 listener 的 O_NONBLOCK（Linux 不会），
        // 不显式改回阻塞会让首包未到时 read_line 直接 WouldBlock 丢连接。
        stream
            .set_nonblocking(false)
            .map_err(|_| RequestError::Malformed)?;
        stream
            .set_read_timeout(Some(HOOK_REQUEST_SLOWLORIS))
            .map_err(|_| RequestError::Malformed)?;
        // 同一个 BufReader 读写——跨两个 reader 会让头解析的预读字节丢掉。
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|_| RequestError::Malformed)?,
        );
        let (method, path, headers) = read_request_head(&mut reader)?;
        // 规格 §4：非 POST 与 token 失败都按鉴权失败计（403）；未知路由 404。
        if method != "POST" {
            respond(&mut stream, 403);
            return Ok(());
        }
        if path != "/hook/claude" {
            respond(&mut stream, 404);
            return Ok(());
        }
        let Some(token) = headers.get("x-orca-agent-hook-token") else {
            respond(&mut stream, 403);
            return Ok(());
        };
        if token != &self.token {
            respond(&mut stream, 403);
            return Ok(());
        }
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        if content_length > HOOK_REQUEST_MAX_BYTES {
            respond(&mut stream, 413);
            return Ok(());
        }
        let body = read_body(&mut reader, content_length)?;
        let event = build_cached_event(&headers, &body);
        match event {
            Some(event) => {
                self.cache.record(event.clone());
                (self.callback)(event);
                respond(&mut stream, 204);
            }
            None => respond(&mut stream, 204),
        }
        Ok(())
    }
}

fn bind_with_retry(attempts: u32) -> Option<TcpListener> {
    for _ in 0..attempts {
        match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => return Some(listener),
            Err(error) => {
                eprintln!("[ade-hooks] bind attempt failed: {error}");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    None
}

fn read_request_head(
    reader: &mut BufReader<TcpStream>,
) -> Result<(String, String, HashMap<String, String>), RequestError> {
    let mut raw = String::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|_| RequestError::Timeout)?;
        if read == 0 {
            return Err(RequestError::Malformed);
        }
        if raw.len() + read > MAX_HEADER_BYTES {
            return Err(RequestError::TooLarge);
        }
        raw.push_str(&line);
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    let mut lines = raw.split("\r\n");
    let request_line = lines.next().ok_or(RequestError::Malformed)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(RequestError::Malformed)?.to_string();
    let path = parts.next().ok_or(RequestError::Malformed)?.to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok((method, path, headers))
}

fn read_body(reader: &mut BufReader<TcpStream>, content_length: usize) -> Result<Vec<u8>, RequestError> {
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|_| RequestError::Timeout)?;
    Ok(body)
}

fn respond(stream: &mut TcpStream, status: u16) {
    let reason = match status {
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "No Content",
    };
    let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.flush();
}

fn meta_attribution(headers: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let encoding = headers
        .get("x-orca-agent-hook-meta-encoding")
        .map(|value| value.trim().to_ascii_lowercase());
    if encoding.as_deref() == Some("base64") {
        if let Some(decoded) = headers
            .get("x-orca-agent-hook-meta")
            .and_then(|value| decode_base64_header(value))
        {
            let fields: Vec<&str> = decoded.split('\u{1f}').collect();
            if fields.len() == 6 && !fields[0].is_empty() {
                for (key, value) in [
                    ("paneKey", fields[0]),
                    ("tabId", fields[1]),
                    ("launchToken", fields[2]),
                    ("worktreeId", fields[3]),
                    ("env", fields[4]),
                    ("version", fields[5]),
                ] {
                    if !value.is_empty() {
                        out.insert(key.to_string(), value.to_string());
                    }
                }
                return out;
            }
        }
    }
    for (header, key) in [
        ("x-orca-pane-key", "paneKey"),
        ("x-orca-tab-id", "tabId"),
        ("x-orca-launch-token", "launchToken"),
        ("x-orca-worktree-id", "worktreeId"),
        ("x-orca-agent-hook-env", "env"),
        ("x-orca-agent-hook-version", "version"),
    ] {
        if let Some(value) = headers.get(header).filter(|value| !value.is_empty()) {
            out.insert(key.to_string(), value.clone());
        }
    }
    out
}

fn decode_base64_header(value: &str) -> Option<String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value.as_bytes())
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let normalized_input = value.trim_end_matches('=');
    let round_trip = base64::engine::general_purpose::STANDARD
        .encode(text.as_bytes());
    if round_trip.trim_end_matches('=') == normalized_input {
        Some(text)
    } else {
        None
    }
}

fn parse_form_urlencoded(input: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in input.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(percent_decode(key), percent_decode(value));
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => match input.get(index + 1..index + 3) {
                Some(hex) => match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                },
                None => {
                    out.push(bytes[index]);
                    index += 1;
                }
            },
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn build_cached_event(headers: &HashMap<String, String>, body: &[u8]) -> Option<CachedHookEvent> {
    let is_form = headers
        .get("content-type")
        .map(|value| value.contains("application/x-www-form-urlencoded"))
        .unwrap_or(false);
    let (payload, attribution) = if is_form {
        let fields = parse_form_urlencoded(&String::from_utf8_lossy(body));
        let payload: Value = serde_json::from_str(fields.get("payload")?).ok()?;
        (payload, fields)
    } else {
        let payload: Value = serde_json::from_slice(body).ok()?;
        (payload, meta_attribution(headers))
    };
    let pane_key = attribution.get("paneKey").cloned().unwrap_or_default();
    if pane_key.trim().is_empty() {
        eprintln!("[ade-hooks] dropping hook event with empty paneKey");
        return None;
    }
    Some(CachedHookEvent {
        source: "claude".to_string(),
        payload,
        pane_key,
        tab_id: attribution.get("tabId").cloned(),
        worktree_id: attribution.get("worktreeId").cloned(),
        launch_token: attribution.get("launchToken").cloned(),
        received_at: now_ms(),
        restored: false,
    })
}

/// spool 重放（规格 §3.4）：启动时逐行 drain，成功行重放并从文件移除，
/// 失败行保留文件下轮再试；无失败行则清空文件。
fn drain_spool(dir: &Path, cache: &Arc<StatusCache>, callback: &SharedCallback) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut retained: Vec<&str> = Vec::new();
        for line in raw.lines().filter(|line| !line.trim().is_empty()) {
            match serde_json::from_str::<CachedHookEvent>(line) {
                Ok(mut event) => {
                    event.restored = false;
                    cache.record(event.clone());
                    callback(event);
                }
                Err(_) => retained.push(line),
            }
        }
        if retained.is_empty() {
            let _ = std::fs::write(&path, "");
        } else {
            let mut contents = retained.join("\n");
            contents.push('\n');
            let _ = std::fs::write(&path, contents);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_test_server(dir: &Path, callback: HookCallback) -> Arc<AgentHookServer> {
        AgentHookServer::start(
            StartOptions {
                app_data_dir: dir.to_path_buf(),
                home: dir.join("home").to_string_lossy().into_owned(),
                env: "development".to_string(),
                install_enabled: false,
            },
            callback,
        )
    }

    fn request(
        port: u16,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n");
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        (status, response)
    }

    fn captured() -> (Arc<Mutex<Vec<CachedHookEvent>>>, HookCallback) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        (
            events,
            Box::new(move |event| sink.lock().unwrap().push(event)),
        )
    }

    #[test]
    fn posts_authenticated_raw_json_with_packed_meta_and_forwards() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        assert!(server.active);
        assert!(server.port > 0);
        let pane_key = "t1:123e4567-e89b-42d3-a456-426614174000";
        let meta = [pane_key, "t1", "tok-1", "r1::/wt", "development", "1"].join("\u{1f}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(meta);
        let body = br#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#;
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[
                ("Content-Type", "application/json"),
                ("X-Orca-Agent-Hook-Token", &server.token),
                ("X-Orca-Agent-Hook-Meta-Encoding", "base64"),
                ("X-Orca-Agent-Hook-Meta", &encoded),
            ],
            body,
        );
        assert_eq!(status, 204);
        let forwarded = events.lock().unwrap();
        assert_eq!(forwarded.len(), 1);
        assert_eq!(forwarded[0].pane_key, pane_key);
        assert_eq!(forwarded[0].tab_id.as_deref(), Some("t1"));
        assert_eq!(forwarded[0].worktree_id.as_deref(), Some("r1::/wt"));
        assert_eq!(forwarded[0].launch_token.as_deref(), Some("tok-1"));
        assert_eq!(forwarded[0].source, "claude");
        assert_eq!(forwarded[0].payload["hook_event_name"], "Stop");
        assert!(!forwarded[0].restored);
        server.shutdown();
    }

    #[test]
    fn accepts_form_fallback_and_single_headers() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let body = "paneKey=t1%3A123e4567-e89b-42d3-a456-426614174000&tabId=t1&payload=%7B%22hook_event_name%22%3A%22UserPromptSubmit%22%7D";
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("X-Orca-Agent-Hook-Token", &server.token),
            ],
            body.as_bytes(),
        );
        assert_eq!(status, 204);
        assert_eq!(events.lock().unwrap()[0].payload["hook_event_name"], "UserPromptSubmit");
        // 单头回退（无 form 元数据）。
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[
                ("Content-Type", "application/json"),
                ("X-Orca-Agent-Hook-Token", &server.token),
                ("X-Orca-Pane-Key", "t2:123e4567-e89b-42d3-a456-426614174000"),
                ("X-Orca-Tab-Id", "t2"),
            ],
            br#"{"hook_event_name":"Stop"}"#,
        );
        assert_eq!(status, 204);
        assert_eq!(events.lock().unwrap()[1].pane_key, "t2:123e4567-e89b-42d3-a456-426614174000");
        server.shutdown();
    }

    #[test]
    fn form_body_with_multibyte_after_percent_stays_204_and_no_panic() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let body = "paneKey=%€&payload={}";
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("X-Orca-Agent-Hook-Token", &server.token),
            ],
            body.as_bytes(),
        );
        assert_eq!(status, 204);
        // `%` 后接 3 字节 UTF-8：`get(index+1..index+3)` 落在字符中间返回 None，
        // 按原样吐出 `%` 与后续字节（不再越界切片 panic）；paneKey 非空照常转发。
        let forwarded = events.lock().unwrap();
        assert_eq!(forwarded.len(), 1);
        assert_eq!(forwarded[0].pane_key, "%€");
        server.shutdown();
    }

    #[test]
    fn rejects_wrong_token_non_post_and_unknown_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[("X-Orca-Agent-Hook-Token", "wrong")],
            b"{}",
        );
        assert_eq!(status, 403);
        let (status, _) = request(server.port, "GET", "/hook/claude", &[], b"");
        assert_eq!(status, 403);
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/codex",
            &[("X-Orca-Agent-Hook-Token", &server.token)],
            b"{}",
        );
        assert_eq!(status, 404);
        assert!(events.lock().unwrap().is_empty());
        server.shutdown();
    }

    #[test]
    fn rejects_oversized_body_with_413_and_drops_empty_pane_key() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let oversized = format!(
            "POST /hook/claude HTTP/1.1\r\nHost: x\r\nX-Orca-Agent-Hook-Token: {}\r\nContent-Length: {}\r\n\r\n",
            server.token,
            HOOK_REQUEST_MAX_BYTES + 1
        )
        .into_bytes();
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        stream.write_all(&oversized).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 413"));
        // 空 paneKey：归因失败计数丢弃，回 204（fail-open）。
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[("X-Orca-Agent-Hook-Token", &server.token)],
            b"{\"hook_event_name\":\"Stop\"}",
        );
        assert_eq!(status, 204);
        assert!(events.lock().unwrap().is_empty());
        server.shutdown();
    }

    #[test]
    fn snapshot_and_cache_survive_restart_and_spool_replays_on_start() {
        let dir = tempfile::tempdir().unwrap();
        {
            let (_, callback) = captured();
            let server = start_test_server(dir.path(), callback);
            let meta = ["t9:123e4567-e89b-42d3-a456-426614174000", "t9", "", "", "development", "1"].join("\u{1f}");
            let encoded = base64::engine::general_purpose::STANDARD.encode(meta);
            let (status, _) = request(
                server.port,
                "POST",
                "/hook/claude",
                &[
                    ("X-Orca-Agent-Hook-Token", &server.token),
                    ("X-Orca-Agent-Hook-Meta-Encoding", "base64"),
                    ("X-Orca-Agent-Hook-Meta", &encoded),
                ],
                br#"{"hook_event_name":"Stop"}"#,
            );
            assert_eq!(status, 204);
            server.shutdown();
        }
        // spool 重放：模拟脚本落盘的行。
        let spool = dir.path().join("spool");
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::write(
            spool.join("pane-t9.jsonl"),
            "{\"paneKey\":\"t8:123e4567-e89b-42d3-a456-426614174000\",\"tabId\":\"t8\",\"worktreeId\":\"\",\"env\":\"development\",\"version\":\"1\",\"launchToken\":\"\",\"source\":\"claude\",\"receivedAt\":1,\"payload\":{\"hook_event_name\":\"Stop\"}}\n",
        )
        .unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let snapshot = server.snapshot();
        assert_eq!(snapshot.len(), 2);
        let hydrated = snapshot
            .iter()
            .find(|entry| entry.pane_key.starts_with("t9:"))
            .unwrap();
        let replayed = snapshot
            .iter()
            .find(|entry| entry.pane_key.starts_with("t8:"))
            .unwrap();
        // 磁盘 hydrate 的行标 restored；spool 重放的行是「迟到送达」，不标。
        assert!(hydrated.restored);
        assert!(!replayed.restored);
        assert!(events.lock().unwrap().iter().any(|entry| entry.pane_key.starts_with("t8:")));
        server.shutdown();
    }

    #[test]
    fn spool_drain_removes_replayed_lines_and_keeps_only_unparsed() {
        let dir = tempfile::tempdir().unwrap();
        let spool = dir.path().join("spool");
        std::fs::create_dir_all(&spool).unwrap();
        let spool_file = spool.join("pane-t7.jsonl");
        let valid = format!(
            "{}\n",
            serde_json::json!({
                "paneKey": "t7:123e4567-e89b-42d3-a456-426614174000",
                "tabId": "t7",
                "worktreeId": "",
                "env": "development",
                "version": "1",
                "launchToken": "",
                "source": "claude",
                "receivedAt": now_ms(),
                "payload": { "hook_event_name": "Stop" },
            })
        );
        let garbage = "not json\n";
        std::fs::write(&spool_file, format!("{valid}{garbage}")).unwrap();
        {
            let (events, callback) = captured();
            let server = start_test_server(dir.path(), callback);
            let snapshot = server.snapshot();
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].pane_key, "t7:123e4567-e89b-42d3-a456-426614174000");
            assert!(!snapshot[0].restored);
            assert_eq!(
                events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|entry| entry.pane_key.starts_with("t7:"))
                    .count(),
                1
            );
            // 合法行被消费移除；无法解析的行保留给下轮。
            assert_eq!(std::fs::read_to_string(&spool_file).unwrap(), garbage);
            server.shutdown();
        }
        // 重启：合法行不得重放；快照只来自 hydrate，回调不再触发。
        let (events_again, callback_again) = captured();
        let server = start_test_server(dir.path(), callback_again);
        let snapshot = server.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot[0].restored);
        assert!(events_again.lock().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&spool_file).unwrap(), garbage);
        server.shutdown();
    }
}
