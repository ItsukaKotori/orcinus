//! Helper transport (spec §3.2): spawn the vendored macOS helper, connect to
//! its Unix socket, exchange single-line NDJSON envelopes, and restart it
//! after any transport failure. Mirrors
//! `orca:src/main/computer/macos-native-provider-transport.ts` (spawn, token
//! file, socket directory, restart) and the request/response framing of
//! `macos-native-provider-client.ts`.

use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::paths::helper_executable;
use crate::protocol::{ComputerError, ComputerErrorCode};
use crate::socket::{connect_with_retry, DEFAULT_ATTEMPT_INTERVAL, DEFAULT_MAX_ATTEMPTS};

/// Request timeout used when the caller does not inject one (spec §3.2).
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Grace period between SIGTERM and SIGKILL when tearing a helper down.
const TERMINATE_GRACE: Duration = Duration::from_secs(1);
/// `$TMPDIR` socket-directory prefix, matching the oracle transport.
const SOCKET_DIR_PREFIX: &str = "orcinus-computer-use-";
/// Random bytes per agent token (hex-encoded to 48 characters on disk).
const TOKEN_BYTES: usize = 24;
const SOCKET_FILE_NAME: &str = "provider.sock";
const TOKEN_FILE_NAME: &str = "provider.token";
const SOCKET_DIR_ATTEMPTS: u32 = 64;
const SOCKET_DIR_SUFFIX_LEN: usize = 6;
const KILL_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Timeout applied to a single helper request when the caller does not inject
/// one (60s; tests inject a short timeout through `HelperTransport::start`).
pub fn helper_request_timeout() -> Duration {
    DEFAULT_REQUEST_TIMEOUT
}

/// NDJSON transport to a spawned macOS helper process.
///
/// The helper is spawned eagerly by [`HelperTransport::start`] and restarted
/// lazily after any IO/timeout failure: the failed helper is terminated and
/// its socket directory removed immediately, and the next request spawns a
/// fresh process with a new socket and token.
pub struct HelperTransport {
    helper_app: PathBuf,
    request_timeout: Duration,
    next_id: u64,
    next_generation: u64,
    active: Option<ActiveHelper>,
}

struct ActiveHelper {
    generation: u64,
    directory: PathBuf,
    token: String,
    child: Child,
    stream: UnixStream,
    read_buffer: Vec<u8>,
}

impl HelperTransport {
    /// Spawn the helper, write its token file and connect to its socket.
    ///
    /// On any failure the spawned process is terminated and the socket
    /// directory is removed before returning the error.
    pub fn start(helper_app: &Path, request_timeout: Duration) -> Result<Self, ComputerError> {
        let mut transport = Self {
            helper_app: helper_app.to_path_buf(),
            request_timeout,
            next_id: 1,
            next_generation: 0,
            active: None,
        };
        transport.ensure_helper()?;
        Ok(transport)
    }

    /// Send one NDJSON request and wait for its response.
    ///
    /// Any IO or timeout failure invalidates the current helper generation
    /// (terminating the process and removing its socket directory); the failed
    /// request returns the mapped error and the next request restarts the
    /// helper. A late response carrying a different id is ignored.
    pub fn request(&mut self, method: &str, params: &Value) -> Result<Value, ComputerError> {
        self.ensure_helper()?;
        let id = self.next_id;
        self.next_id += 1;
        let (generation, line) = {
            let active = self.active.as_ref().expect("helper started above");
            (
                active.generation,
                format!(
                    "{}\n",
                    json!({"id": id, "method": method, "params": params, "token": active.token})
                ),
            )
        };
        let outcome = {
            let active = self.active.as_mut().expect("helper started above");
            exchange(active, self.request_timeout, id, method, &line)
        };
        match outcome {
            Ok(result) => Ok(result),
            Err(error) => {
                self.invalidate_if_current(generation);
                Err(error)
            }
        }
    }

    /// Best-effort `terminate` request, then SIGTERM, reap, and remove the
    /// socket directory. Idempotent.
    pub fn shutdown(&mut self) {
        let Some(mut active) = self.active.take() else {
            return;
        };
        let id = self.next_id;
        self.next_id += 1;
        let line = format!(
            "{}\n",
            json!({"id": id, "method": "terminate", "params": {}, "token": active.token})
        );
        let _ = active.stream.write_all(line.as_bytes());
        let _ = active.stream.shutdown(std::net::Shutdown::Write);
        terminate_child(&mut active.child);
        reap_child(&mut active.child, TERMINATE_GRACE);
        let _ = fs::remove_dir_all(&active.directory);
    }

    fn ensure_helper(&mut self) -> Result<(), ComputerError> {
        if self.active.is_some() {
            return Ok(());
        }
        let directory = create_socket_directory().map_err(|error| {
            ComputerError::new(
                ComputerErrorCode::AccessibilityError,
                format!("failed to create computer-use socket directory: {error}"),
            )
        })?;
        let socket_path = directory.join(SOCKET_FILE_NAME);
        let token_path = directory.join(TOKEN_FILE_NAME);
        let token = generate_token();
        if let Err(error) = write_token_file(&token_path, &token) {
            let _ = fs::remove_dir_all(&directory);
            return Err(ComputerError::new(
                ComputerErrorCode::AccessibilityError,
                format!("failed to write computer-use agent token: {error}"),
            ));
        }
        let executable = helper_executable(&self.helper_app);
        let mut child = match spawn_helper(&executable, &socket_path, &token_path) {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                return Err(ComputerError::new(
                    ComputerErrorCode::AccessibilityError,
                    format!("native macOS helper app failed to start: {error}"),
                ));
            }
        };
        match connect_with_retry(&socket_path, DEFAULT_ATTEMPT_INTERVAL, DEFAULT_MAX_ATTEMPTS) {
            Ok(stream) => {
                // The helper read the token at startup; it is only needed to
                // authenticate the connection, so it is deleted right away.
                let _ = fs::remove_file(&token_path);
                self.next_generation += 1;
                self.active = Some(ActiveHelper {
                    generation: self.next_generation,
                    directory,
                    token,
                    child,
                    stream,
                    read_buffer: Vec::new(),
                });
                Ok(())
            }
            Err(error) => {
                let exited = child.try_wait().ok().flatten();
                terminate_child(&mut child);
                reap_child(&mut child, TERMINATE_GRACE);
                let _ = fs::remove_dir_all(&directory);
                let (code, message) = match exited {
                    Some(status) => (
                        ComputerErrorCode::AccessibilityError,
                        format!(
                            "native macOS helper app exited before connecting: {}",
                            describe_exit(&status)
                        ),
                    ),
                    None => (
                        ComputerErrorCode::ActionTimeout,
                        format!("native macOS helper app did not open its socket: {error}"),
                    ),
                };
                Err(ComputerError::new(code, message))
            }
        }
    }

    /// Generation guard: only tear down the helper the failed request was
    /// talking to, never a newer replacement.
    fn invalidate_if_current(&mut self, generation: u64) {
        let Some(active) = self.active.take() else {
            return;
        };
        if active.generation != generation {
            self.active = Some(active);
            return;
        }
        let mut active = active;
        terminate_child(&mut active.child);
        reap_child(&mut active.child, TERMINATE_GRACE);
        // Dropping `active` drops the socket stream too: a late response from
        // the old helper can no longer be read by anyone.
        let _ = fs::remove_dir_all(&active.directory);
    }
}

impl Drop for HelperTransport {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            terminate_child(&mut active.child);
            reap_child(&mut active.child, TERMINATE_GRACE);
            let _ = fs::remove_dir_all(&active.directory);
        }
    }
}

/// Response envelope from the helper: `{"id":n,"ok":true,"result":…}` or
/// `{"id":n,"ok":false,"error":{"code","message"}}`.
#[derive(Deserialize)]
struct NativeResponse {
    id: u64,
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<NativeError>,
}

#[derive(Deserialize)]
struct NativeError {
    code: String,
    message: String,
}

impl NativeResponse {
    fn resolve(self) -> Result<Value, ComputerError> {
        if self.ok {
            return Ok(self.result.unwrap_or(Value::Null));
        }
        let Some(error) = self.error else {
            return Err(ComputerError::new(
                ComputerErrorCode::AccessibilityError,
                "native macOS helper app returned an error response without details",
            ));
        };
        match serde_json::from_value::<ComputerErrorCode>(Value::String(error.code.clone())) {
            Ok(code) => Err(ComputerError::new(code, error.message)),
            Err(_) => Err(ComputerError::new(
                ComputerErrorCode::AccessibilityError,
                format!("{} (provider code {})", error.message, error.code),
            )),
        }
    }
}

fn exchange(
    active: &mut ActiveHelper,
    timeout: Duration,
    id: u64,
    method: &str,
    line: &str,
) -> Result<Value, ComputerError> {
    if let Err(error) = active.stream.write_all(line.as_bytes()) {
        return Err(ComputerError::new(
            ComputerErrorCode::AccessibilityError,
            format!("failed to write to native macOS helper app: {error}"),
        ));
    }
    let deadline = Instant::now() + timeout;
    loop {
        while let Some(response_line) = take_line(&mut active.read_buffer) {
            let Ok(response) = serde_json::from_str::<NativeResponse>(&response_line) else {
                continue;
            };
            if response.id != id {
                // Stale response from another request/generation: ignore.
                continue;
            }
            return response.resolve();
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(action_timeout(method));
        }
        if let Err(error) = active.stream.set_read_timeout(Some(remaining)) {
            return Err(ComputerError::new(
                ComputerErrorCode::AccessibilityError,
                format!("failed to set native macOS helper read timeout: {error}"),
            ));
        }
        let mut chunk = [0u8; 8192];
        match active.stream.read(&mut chunk) {
            Ok(0) => {
                return Err(ComputerError::new(
                    ComputerErrorCode::AccessibilityError,
                    "native macOS helper app connection closed",
                ))
            }
            Ok(read) => active.read_buffer.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(action_timeout(method));
            }
            Err(error) => {
                return Err(ComputerError::new(
                    ComputerErrorCode::AccessibilityError,
                    format!("native macOS helper app transport error: {error}"),
                ))
            }
        }
    }
}

fn action_timeout(method: &str) -> ComputerError {
    ComputerError::new(
        ComputerErrorCode::ActionTimeout,
        format!("native macOS provider {method} timed out"),
    )
}

fn take_line(buffer: &mut Vec<u8>) -> Option<String> {
    let newline = buffer.iter().position(|byte| *byte == b'\n')?;
    let line: Vec<u8> = buffer.drain(..=newline).collect();
    let text = String::from_utf8_lossy(&line[..line.len() - 1])
        .trim_end_matches('\r')
        .to_string();
    Some(text)
}

fn create_socket_directory() -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..SOCKET_DIR_ATTEMPTS {
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(SOCKET_DIR_SUFFIX_LEN)
            .map(char::from)
            .collect();
        let path = base.join(format!("{SOCKET_DIR_PREFIX}{suffix}"));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o700)) {
                    let _ = fs::remove_dir_all(&path);
                    return Err(error);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique computer-use socket directory",
    ))
}

fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill(&mut bytes);
    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    token
}

fn write_token_file(path: &Path, token: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    file.flush()
}

fn spawn_helper(executable: &Path, socket_path: &Path, token_path: &Path) -> io::Result<Child> {
    let mut command = Command::new(executable);
    command
        .arg("--agent")
        .arg(socket_path)
        .arg("--token-file")
        .arg(token_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Why: the helper is detached from the app so it survives the request
    // that launched it and does not receive the app's terminal signals.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

fn terminate_child(child: &mut Child) {
    let pid = child.id() as libc::pid_t;
    // SIGTERM first: the helper is expected to shut down gracefully.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

fn reap_child(child: &mut Child, grace: Duration) {
    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => thread::sleep(KILL_POLL_INTERVAL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

fn describe_exit(status: &ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("code {code}");
    }
    if let Some(signal) = status.signal() {
        return format!("signal {signal}");
    }
    "unknown status".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_request_timeout_is_sixty_seconds() {
        assert_eq!(helper_request_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn generated_tokens_are_48_lowercase_hex_chars() {
        let token = generate_token();
        assert_eq!(token.len(), 48);
        assert!(token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert_ne!(token, generate_token(), "tokens must be random");
    }

    #[test]
    fn take_line_splits_ndjson_lines_and_keeps_partial_data() {
        let mut buffer = b"{\"id\":1}\n{\"id\":2}".to_vec();
        assert_eq!(take_line(&mut buffer).as_deref(), Some("{\"id\":1}"));
        assert_eq!(take_line(&mut buffer), None);
        buffer.extend_from_slice(b"}\n");
        assert_eq!(take_line(&mut buffer).as_deref(), Some("{\"id\":2}}"));
        assert_eq!(take_line(&mut buffer), None);
    }

    #[test]
    fn native_error_response_maps_code_and_message() {
        let response: NativeResponse = serde_json::from_str(
            r#"{"id":7,"ok":false,"error":{"code":"permission_denied","message":"nope"}}"#,
        )
        .unwrap();
        let error = response.resolve().unwrap_err();
        assert_eq!(error.code, ComputerErrorCode::PermissionDenied);
        assert_eq!(error.message, "nope");
    }

    #[test]
    fn native_unknown_error_code_falls_back_to_accessibility_error() {
        let response: NativeResponse = serde_json::from_str(
            r#"{"id":7,"ok":false,"error":{"code":"mystery","message":"who knows"}}"#,
        )
        .unwrap();
        let error = response.resolve().unwrap_err();
        assert_eq!(error.code, ComputerErrorCode::AccessibilityError);
        assert!(error.message.contains("mystery"), "{}", error.message);
    }

    #[test]
    fn native_ok_response_returns_result_and_defaults_to_null() {
        let response: NativeResponse =
            serde_json::from_str(r#"{"id":3,"ok":true,"result":{"a":1}}"#).unwrap();
        assert_eq!(response.resolve().unwrap(), json!({"a": 1}));
        let response: NativeResponse = serde_json::from_str(r#"{"id":3,"ok":true}"#).unwrap();
        assert_eq!(response.resolve().unwrap(), Value::Null);
    }
}
