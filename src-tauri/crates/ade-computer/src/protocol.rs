//! Protocol types shared by the provider client, the runtime RPC server, and
//! the `orcinus` CLI (spec §3.2/§3.5). Error-code strings and RPC method
//! names must stay byte-for-byte identical to
//! `src/shared/computer-use-runtime-types.ts` (the web client contract).

use serde::{Deserialize, Serialize};

/// Canonical computer-use error-code catalog. The `as_str()` mapping is the
/// wire contract (`COMPUTER_ERROR_CODES` in the TS types module).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerErrorCode {
    AppNotFound,
    AppBlocked,
    WindowNotFound,
    WindowNotFocused,
    WindowStale,
    ProviderIncompatible,
    UnsupportedCapability,
    PermissionDenied,
    ElementNotFound,
    ElementNotClickable,
    ActionNotSupported,
    ValueNotSettable,
    InvalidArgument,
    ActionTimeout,
    ScreenshotFailed,
    AccessibilityError,
}

impl ComputerErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppNotFound => "app_not_found",
            Self::AppBlocked => "app_blocked",
            Self::WindowNotFound => "window_not_found",
            Self::WindowNotFocused => "window_not_focused",
            Self::WindowStale => "window_stale",
            Self::ProviderIncompatible => "provider_incompatible",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::PermissionDenied => "permission_denied",
            Self::ElementNotFound => "element_not_found",
            Self::ElementNotClickable => "element_not_clickable",
            Self::ActionNotSupported => "action_not_supported",
            Self::ValueNotSettable => "value_not_settable",
            Self::InvalidArgument => "invalid_argument",
            Self::ActionTimeout => "action_timeout",
            Self::ScreenshotFailed => "screenshot_failed",
            Self::AccessibilityError => "accessibility_error",
        }
    }
}

/// Provider/RPC error envelope (`{code, message, data?}`). `code` keeps the
/// canonical string form even when deserialized from a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerError {
    pub code: ComputerErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ComputerError {
    pub fn new(code: ComputerErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// Runtime RPC method names served by the app (spec §3.5), mirroring the
/// oracle `computer.*` surface. `status.get` is deliberately not part of this
/// list (it predates the computer-use slice).
pub const COMPUTER_RPC_METHODS: [&str; 15] = [
    "computer.capabilities",
    "computer.listApps",
    "computer.permissions",
    "computer.permissionsStatus",
    "computer.listWindows",
    "computer.getAppState",
    "computer.click",
    "computer.performSecondaryAction",
    "computer.scroll",
    "computer.drag",
    "computer.typeText",
    "computer.pressKey",
    "computer.hotkey",
    "computer.pasteText",
    "computer.setValue",
];

/// Runtime bootstrap metadata file name (spec §3.5) inside the app data dir.
pub const RUNTIME_METADATA_FILE: &str = "runtime.json";
/// Runtime RPC socket file name (spec §3.5) inside the app data dir.
pub const RUNTIME_SOCKET_FILE: &str = "runtime.sock";

/// One transport entry advertised in `runtime.json`. Only `unix` is produced
/// today; the shape matches oracle `shared/runtime-bootstrap.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RuntimeTransport {
    Unix { endpoint: String },
}

/// Runtime bootstrap metadata (`runtime.json`, 0600). Field names are
/// camelCase on the wire so the `orcinus` CLI (and the oracle-shaped client
/// logic) can read it directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadata {
    pub runtime_id: String,
    pub pid: u32,
    pub transports: Vec<RuntimeTransport>,
    pub auth_token: String,
    pub started_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn error_codes_match_typescript_catalog_verbatim() {
        let expected = [
            (ComputerErrorCode::AppNotFound, "app_not_found"),
            (ComputerErrorCode::AppBlocked, "app_blocked"),
            (ComputerErrorCode::WindowNotFound, "window_not_found"),
            (ComputerErrorCode::WindowNotFocused, "window_not_focused"),
            (ComputerErrorCode::WindowStale, "window_stale"),
            (
                ComputerErrorCode::ProviderIncompatible,
                "provider_incompatible",
            ),
            (
                ComputerErrorCode::UnsupportedCapability,
                "unsupported_capability",
            ),
            (ComputerErrorCode::PermissionDenied, "permission_denied"),
            (ComputerErrorCode::ElementNotFound, "element_not_found"),
            (
                ComputerErrorCode::ElementNotClickable,
                "element_not_clickable",
            ),
            (
                ComputerErrorCode::ActionNotSupported,
                "action_not_supported",
            ),
            (ComputerErrorCode::ValueNotSettable, "value_not_settable"),
            (ComputerErrorCode::InvalidArgument, "invalid_argument"),
            (ComputerErrorCode::ActionTimeout, "action_timeout"),
            (ComputerErrorCode::ScreenshotFailed, "screenshot_failed"),
            (ComputerErrorCode::AccessibilityError, "accessibility_error"),
        ];
        assert_eq!(
            expected.len(),
            16,
            "COMPUTER_ERROR_CODES in computer-use-runtime-types.ts has 16 entries"
        );
        for (code, expected_str) in expected {
            assert_eq!(code.as_str(), expected_str);
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                Value::String(expected_str.to_string()),
                "serde form must equal as_str"
            );
            let round_trip: ComputerErrorCode =
                serde_json::from_value(Value::String(expected_str.to_string())).unwrap();
            assert_eq!(round_trip, code);
        }
    }

    #[test]
    fn computer_rpc_methods_are_exactly_the_fifteen_camel_case_names() {
        assert_eq!(
            COMPUTER_RPC_METHODS,
            [
                "computer.capabilities",
                "computer.listApps",
                "computer.permissions",
                "computer.permissionsStatus",
                "computer.listWindows",
                "computer.getAppState",
                "computer.click",
                "computer.performSecondaryAction",
                "computer.scroll",
                "computer.drag",
                "computer.typeText",
                "computer.pressKey",
                "computer.hotkey",
                "computer.pasteText",
                "computer.setValue",
            ]
        );
        assert_eq!(COMPUTER_RPC_METHODS.len(), 15);
        assert!(!COMPUTER_RPC_METHODS.contains(&"status.get"));
    }

    #[test]
    fn runtime_file_names_match_contract() {
        assert_eq!(RUNTIME_METADATA_FILE, "runtime.json");
        assert_eq!(RUNTIME_SOCKET_FILE, "runtime.sock");
    }

    #[test]
    fn runtime_metadata_serializes_with_camel_case_fields_and_unix_transport_shape() {
        let metadata = RuntimeMetadata {
            runtime_id: "01JTESTRUNTIME".to_string(),
            pid: 4321,
            transports: vec![RuntimeTransport::Unix {
                endpoint: "/Users/x/Library/Application Support/dev.itsuka.orcinus/runtime.sock"
                    .to_string(),
            }],
            auth_token: "deadbeef".to_string(),
            started_at: "2026-10-11T10:00:00.000Z".to_string(),
        };
        let value = serde_json::to_value(&metadata).unwrap();
        assert_eq!(
            value,
            json!({
                "runtimeId": "01JTESTRUNTIME",
                "pid": 4321,
                "transports": [{
                    "kind": "unix",
                    "endpoint": "/Users/x/Library/Application Support/dev.itsuka.orcinus/runtime.sock",
                }],
                "authToken": "deadbeef",
                "startedAt": "2026-10-11T10:00:00.000Z",
            })
        );
        let round_trip: RuntimeMetadata = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip, metadata);
    }

    #[test]
    fn computer_error_serializes_code_as_canonical_string() {
        let error = ComputerError::new(ComputerErrorCode::ActionTimeout, "click timed out");
        let value = serde_json::to_value(&error).unwrap();
        assert_eq!(
            value,
            json!({"code": "action_timeout", "message": "click timed out"})
        );
        let round_trip: ComputerError = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip.code, ComputerErrorCode::ActionTimeout);
    }
}
