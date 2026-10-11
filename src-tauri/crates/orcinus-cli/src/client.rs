//! Runtime client (spec §3.6): read `runtime.json`, connect to the app's Unix
//! socket, send one NDJSON request and map the response envelope. Mirrors
//! `orca:src/cli/runtime/{metadata,transport}.ts` (local transport only):
//! request `{id,authToken,method,params}`, success `{id,ok,result,_meta}`,
//! failure `{id,ok:false,error}`; `{"_keepalive":true}` frames refresh the
//! deadline. A response with a mismatched id or `_meta.runtimeId` is
//! `invalid_runtime_response`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ade_computer::protocol::{RuntimeMetadata, RuntimeTransport, RUNTIME_METADATA_FILE};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::errors::{runtime_unavailable, CliError, RUNTIME_UNAVAILABLE_HINT};

/// Default request timeout (spec §3.6): requests 60s, `status.get` 1s.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// `status.get` timeout used by `status`/`open` reachability probes.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(1);

/// Environment override for the app data dir (spec §3.5/§3.6).
pub const USER_DATA_PATH_ENV: &str = "ORCINUS_USER_DATA_PATH";
/// Default app data dir under `~/Library/Application Support`.
pub const DEFAULT_APP_DIR_NAME: &str = "dev.itsuka.orcinus";

/// Runtime metadata path: `ORCINUS_USER_DATA_PATH` override else
/// `~/Library/Application Support/dev.itsuka.orcinus/runtime.json`.
pub fn metadata_path() -> PathBuf {
    if let Ok(path) = std::env::var(USER_DATA_PATH_ENV) {
        if !path.trim().is_empty() {
            return PathBuf::from(path).join(RUNTIME_METADATA_FILE);
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(DEFAULT_APP_DIR_NAME)
        .join(RUNTIME_METADATA_FILE)
}

/// Metadata read state: absent (never started), unparseable/incomplete
/// (stale bootstrap) or usable.
#[derive(Debug)]
pub enum MetadataRead {
    Missing,
    Unreadable(String),
    Ready(RuntimeMetadata),
}

/// Read `runtime.json`. `Missing` only when the file does not exist;
/// unparseable or incomplete metadata (no unix transport, no auth token) is
/// `Unreadable` so `status` can distinguish "never started" from "started with
/// a broken bootstrap".
pub fn read_metadata(path: &Path) -> Result<MetadataRead, CliError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MetadataRead::Missing)
        }
        Err(_) => {
            return Ok(MetadataRead::Unreadable(format!(
                "Could not read Orcinus runtime metadata at {}",
                path.display()
            )))
        }
    };
    let metadata: RuntimeMetadata = match serde_json::from_str(&raw) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(MetadataRead::Unreadable(format!(
                "Orcinus runtime metadata is unreadable at {}",
                path.display()
            )))
        }
    };
    if metadata.auth_token.is_empty() || socket_endpoint(&metadata).is_none() {
        return Ok(MetadataRead::Unreadable(format!(
            "Orcinus runtime metadata is incomplete at {}",
            path.display()
        )));
    }
    Ok(MetadataRead::Ready(metadata))
}

/// The unix socket endpoint advertised by the metadata, if any.
pub fn socket_endpoint(metadata: &RuntimeMetadata) -> Option<&str> {
    metadata
        .transports
        .iter()
        .find_map(|transport| match transport {
            RuntimeTransport::Unix { endpoint } if !endpoint.is_empty() => Some(endpoint.as_str()),
            _ => None,
        })
}

#[derive(Debug, Deserialize)]
struct ResponseEnvelope {
    id: String,
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<ResponseError>,
    #[serde(default, rename = "_meta")]
    meta: Option<ResponseMeta>,
}

#[derive(Debug, Deserialize)]
struct ResponseError {
    code: String,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ResponseMeta {
    #[serde(default, rename = "runtimeId")]
    runtime_id: Option<Value>,
}

/// One connected runtime (metadata parsed, not yet connected). Each `call`
/// opens its own short-lived socket connection, like the oracle CLI.
pub struct RuntimeClient {
    metadata: RuntimeMetadata,
    socket_path: PathBuf,
}

impl RuntimeClient {
    /// Read metadata and validate it. Missing/unreadable/incomplete metadata →
    /// `runtime_unavailable`; use [`read_metadata`] directly for `status`
    /// synthesis.
    #[allow(dead_code)]
    pub fn connect() -> Result<Self, CliError> {
        Self::connect_at(&metadata_path())
    }

    pub fn connect_at(path: &Path) -> Result<Self, CliError> {
        let metadata = match read_metadata(path)? {
            MetadataRead::Ready(metadata) => metadata,
            MetadataRead::Missing => {
                return Err(runtime_unavailable(format!(
                    "Could not read Orcinus runtime metadata at {}. {RUNTIME_UNAVAILABLE_HINT}",
                    path.display()
                )))
            }
            MetadataRead::Unreadable(message) => {
                return Err(runtime_unavailable(format!(
                    "{message}. {RUNTIME_UNAVAILABLE_HINT}"
                )))
            }
        };
        let socket_path = PathBuf::from(
            socket_endpoint(&metadata)
                .expect("read_metadata validates the transport")
                .to_string(),
        );
        Ok(Self {
            metadata,
            socket_path,
        })
    }

    /// Send one request with the default 60s timeout. The status/open
    /// skeleton uses `call_with_timeout` only; the `computer *` slice builds
    /// on this.
    #[allow(dead_code)]
    pub fn call(&self, method: &str, params: Value) -> Result<Value, CliError> {
        self.call_with_timeout(method, params, DEFAULT_REQUEST_TIMEOUT)
    }

    /// Send one request and wait for the matching response frame.
    pub fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CliError> {
        self.call_frames("unix", method, params, timeout)
    }

    fn call_frames(
        &self,
        transport_kind: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CliError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            runtime_unavailable(format!(
                "Could not connect to the running Orcinus app at {}: {error}",
                self.socket_path.display()
            ))
            .with_runtime_id(&self.metadata.runtime_id)
        })?;

        let request_id = Uuid::new_v4().to_string();
        let request = json!({
            "id": request_id,
            "authToken": self.metadata.auth_token,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_vec(&request)
            .map_err(|error| CliError::new("runtime_error", error.to_string()))?;
        line.push(b'\n');

        let deadline = Instant::now() + timeout;
        let mut writer = stream
            .try_clone()
            .map_err(|error| CliError::new("runtime_error", error.to_string()))?;
        let _ = writer.set_write_timeout(Some(timeout));
        writer.write_all(&line).map_err(|error| {
            runtime_unavailable(format!(
                "Could not send the request to the Orcinus app: {error}"
            ))
            .with_runtime_id(&self.metadata.runtime_id)
        })?;
        let _ = writer.flush();

        let mut reader = BufReader::new(stream);
        loop {
            let remaining = match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) if !remaining.is_zero() => remaining,
                _ => return Err(self.timeout_error(transport_kind)),
            };
            let _ = reader
                .get_ref()
                .set_read_timeout(Some(remaining.max(Duration::from_millis(1))));

            let mut frame = String::new();
            match reader.read_line(&mut frame) {
                Ok(0) => {
                    return Err(runtime_unavailable(
                        "The Orcinus runtime closed the connection before responding. Restart Orcinus and try again.",
                    )
                    .with_runtime_id(&self.metadata.runtime_id))
                }
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(self.timeout_error(transport_kind))
                }
                Err(error) => {
                    return Err(runtime_unavailable(format!(
                        "The Orcinus runtime closed the connection before responding: {error}"
                    ))
                    .with_runtime_id(&self.metadata.runtime_id))
                }
            }

            let trimmed = frame.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed).map_err(|_| {
                invalid_response(
                    "The Orcinus runtime returned an invalid response frame.",
                    &self.metadata.runtime_id,
                )
            })?;
            if value.get("_keepalive") == Some(&Value::Bool(true)) {
                continue;
            }

            let response: ResponseEnvelope = serde_json::from_value(value).map_err(|_| {
                invalid_response(
                    "The Orcinus runtime returned an invalid response frame.",
                    &self.metadata.runtime_id,
                )
            })?;
            if response.id != request_id {
                return Err(invalid_response(
                    "The Orcinus runtime returned a mismatched response id.",
                    &self.metadata.runtime_id,
                ));
            }
            let response_runtime_id = response
                .meta
                .as_ref()
                .and_then(|meta| meta.runtime_id.as_ref())
                .and_then(Value::as_str);
            if response_runtime_id != Some(self.metadata.runtime_id.as_str()) {
                return Err(invalid_response(
                    "The Orcinus runtime changed while the request was in flight. Retry the command.",
                    &self.metadata.runtime_id,
                ));
            }

            if response.ok {
                return Ok(response.result);
            }
            let error = response.error.ok_or_else(|| {
                invalid_response(
                    "The Orcinus runtime returned an invalid response frame.",
                    &self.metadata.runtime_id,
                )
            })?;
            let mut cli_error =
                CliError::new(error.code, error.message).with_runtime_id(&self.metadata.runtime_id);
            cli_error.data = error.data;
            return Err(cli_error);
        }
    }

    fn timeout_error(&self, transport_kind: &str) -> CliError {
        runtime_unavailable(format!(
            "Timed out waiting for the Orcinus runtime to respond (transport: {transport_kind})."
        ))
        .with_runtime_id(&self.metadata.runtime_id)
    }
}

fn invalid_response(message: &str, runtime_id: &str) -> CliError {
    CliError::new("invalid_runtime_response", message).with_runtime_id(runtime_id)
}
