//! CLI error type and output envelopes. Success output is
//! `{"id":<uuid>,"ok":true,"result":…,"_meta":{"runtimeId":…}}`; failures are
//! `{"id":"local","ok":false,"error":{code,message,data?}}` and always exit 1
//! in `--json` mode. Human mode prints the readable message only.

use serde_json::{json, Value};

/// One CLI failure: a stable `code`, a human `message` and optional structured
/// `data` (mirrors `orca:src/cli/cli-error.ts`).
#[derive(Debug, Clone)]
pub struct CliError {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

impl CliError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: None,
        }
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
        json!({
            "id": "local",
            "ok": false,
            "error": error,
        })
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
