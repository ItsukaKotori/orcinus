//! CLI error type and output envelopes. Success output is
//! `{"id":<uuid>,"ok":true,"result":…,"_meta":{"runtimeId":…}}`; failures are
//! `{"id":"local","ok":false,"error":{code,message,data?}}` (+ `_meta` once a
//! runtime id is known) and always exit 1 in `--json` mode. Human mode prints
//! the readable message only.

use serde_json::{json, Value};

/// One CLI failure: a stable `code`, a human `message` and optional structured
/// `data` (mirrors `orca:src/cli/cli-error.ts`).
#[derive(Debug, Clone)]
pub struct CliError {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
    /// Runtime id once a metadata read succeeded, so `--json` failures carry
    /// `_meta.runtimeId` like the oracle.
    pub runtime_id: Option<String>,
}

impl CliError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: None,
            runtime_id: None,
        }
    }

    pub fn with_runtime_id(mut self, runtime_id: impl Into<String>) -> Self {
        self.runtime_id = Some(runtime_id.into());
        self
    }

    /// Failure envelope for `--json` mode (`id: "local"`).
    pub fn envelope(&self) -> Value {
        let mut error = json!({
            "code": self.code,
            "message": self.message,
        });
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        let mut envelope = json!({
            "id": "local",
            "ok": false,
            "error": error,
        });
        if let Some(runtime_id) = &self.runtime_id {
            envelope["_meta"] = json!({"runtimeId": runtime_id});
        }
        envelope
    }
}

/// Success envelope for `--json` mode.
pub fn success_envelope(id: &str, runtime_id: &str, result: Value) -> Value {
    json!({
        "id": id,
        "ok": true,
        "result": result,
        "_meta": {"runtimeId": runtime_id},
    })
}

/// The standard not-running failure (spec §3.6): missing/incomplete metadata
/// or a failed connection.
pub fn runtime_unavailable(message: impl Into<String>) -> CliError {
    CliError::new("runtime_unavailable", message)
}

/// Human-readable form of the runtime_unavailable message with the recovery
/// hint appended, matching the oracle wording.
pub const RUNTIME_UNAVAILABLE_HINT: &str = "Orcinus is not running. Run 'orcinus open' first.";
