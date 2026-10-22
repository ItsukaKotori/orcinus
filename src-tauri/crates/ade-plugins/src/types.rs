//! Shared plugin identity rules, the closed capability set, and the error
//! envelope used by every `ade-plugins` surface.
//!
//! The id/path rules and capability canonicalization mirror
//! `orca:src/shared/plugins/plugin-id-format.ts`, `plugin-path-safety.ts`, and
//! `plugin-capabilities.ts` exactly (Zod semantics included).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Install/deployment scope. Serialized as `global` / `project` to match the
/// renderer contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginScope {
    Global,
    Project,
}

impl PluginScope {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginScope::Global => "global",
            PluginScope::Project => "project",
        }
    }
}

pub const PLUGIN_ID_MAX_LENGTH: usize = 64;

const DANGEROUS_PLUGIN_NAMES: [&str; 3] = ["__proto__", "prototype", "constructor"];

/// v1 capability model: a closed set of unscoped kinds plus the fork's
/// `workspace:open-tab` extension, the `worker:call` / `session:push` plugin
/// UI↔worker channel kinds (10 total).
/// Declaration order is the canonical order produced by [`canonical_capability_set`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    EventsSubscribe,
    NotificationsShow,
    Secrets,
    SessionPush,
    SettingsOwn,
    Storage,
    TerminalSend,
    WorkerCall,
    WorkspaceOpenTab,
    WorkspaceRead,
}

impl Capability {
    pub const ALL: [Capability; 10] = [
        Capability::EventsSubscribe,
        Capability::NotificationsShow,
        Capability::Secrets,
        Capability::SessionPush,
        Capability::SettingsOwn,
        Capability::Storage,
        Capability::TerminalSend,
        Capability::WorkerCall,
        Capability::WorkspaceOpenTab,
        Capability::WorkspaceRead,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::EventsSubscribe => "events:subscribe",
            Capability::NotificationsShow => "notifications:show",
            Capability::Secrets => "secrets",
            Capability::SessionPush => "session:push",
            Capability::SettingsOwn => "settings:own",
            Capability::Storage => "storage",
            Capability::TerminalSend => "terminal:send",
            Capability::WorkerCall => "worker:call",
            Capability::WorkspaceOpenTab => "workspace:open-tab",
            Capability::WorkspaceRead => "workspace:read",
        }
    }

    pub fn from_kind(kind: &str) -> Option<Capability> {
        Capability::ALL
            .into_iter()
            .find(|capability| capability.as_str() == kind)
    }
}

impl Serialize for Capability {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("kind", self.as_str())?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCapability {
            kind: String,
        }

        let raw = RawCapability::deserialize(deserializer)?;
        Capability::from_kind(&raw.kind).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown capability kind: {}", raw.kind))
        })
    }
}

/// Canonical serialization of a capability set (order- and duplicate-
/// insensitive, encoded like the oracle's `canonicalizeCapabilitySet`).
pub fn canonical_capability_set(capabilities: &[Capability]) -> String {
    let mut encoded: Vec<String> = capabilities
        .iter()
        .map(|capability| format!("{{\"kind\":\"{}\"}}", capability.as_str()))
        .collect();
    encoded.sort();
    encoded.dedup();
    format!("[{}]", encoded.join(","))
}

/// Error codes surfaced to the UI. Codes are stable identifiers; messages are
/// human-readable and mirror the oracle validation wording where one exists.
pub const INVALID_MANIFEST: &str = "invalid_manifest";
pub const ENGINE_INCOMPATIBLE: &str = "engine_incompatible";
pub const IO_ERROR: &str = "io_error";
pub const UNSAFE_PATH: &str = "unsafe_path";
pub const SYMLINK_NOT_ALLOWED: &str = "symlink_not_allowed";
pub const TOO_MANY_FILES: &str = "too_many_files";
pub const CONTENT_TOO_LARGE: &str = "content_too_large";
pub const UNSUPPORTED_ENTRY: &str = "unsupported_entry";
pub const CONTENT_CHANGED: &str = "content_changed";
pub const MANIFEST_MISMATCH: &str = "manifest_mismatch";
pub const CONSENT_STALE: &str = "consent_stale";
pub const INTEGRITY_MISMATCH: &str = "integrity_mismatch";
pub const INVALID_PARAMS: &str = "invalid_params";
pub const ACTION_FAILED: &str = "action_failed";
pub const UNAVAILABLE: &str = "unavailable";
/// A panel/tab entry the session or request names is no longer declared.
pub const PANEL_NOT_FOUND: &str = "panel_not_found";
/// A main-area tab entry the request names is not declared.
pub const TAB_NOT_FOUND: &str = "tab_not_found";
/// The per-worktree live plugin-tab cap was reached.
pub const TAB_LIMIT: &str = "tab_limit";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PluginError {
    code: &'static str,
    message: String,
}

impl PluginError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// `^[a-z0-9]+(?:-[a-z0-9]+)*$`, max 64 chars, and not a reserved name.
pub fn is_safe_plugin_id(id: &str) -> bool {
    id.len() <= PLUGIN_ID_MAX_LENGTH
        && !id.is_empty()
        && id.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && !DANGEROUS_PLUGIN_NAMES.contains(&id)
}

/// Per-segment path rule from the oracle's `pluginPathSegmentError`.
pub fn plugin_path_segment_error(segment: &str) -> Option<&'static str> {
    if segment.is_empty() || segment == "." || segment == ".." {
        return Some("empty and dot path segments are not allowed");
    }
    if segment.ends_with('.') || segment.ends_with(' ') {
        return Some("path segments may not end with a dot or space");
    }
    if segment.chars().any(|character| {
        matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (character as u32) <= 31
    }) {
        return Some(
            "path segment contains a Windows-forbidden character or alternate-data-stream colon",
        );
    }
    if is_windows_device_name(segment) {
        return Some("path segment is a Windows reserved device name");
    }
    None
}

fn is_windows_device_name(segment: &str) -> bool {
    let lowered = segment.to_ascii_lowercase();
    let base = match lowered.split_once('.') {
        Some((base, _)) => base,
        None => lowered.as_str(),
    };
    if matches!(
        base,
        "con" | "prn" | "aux" | "nul" | "clock$" | "conin$" | "conout$"
    ) {
        return true;
    }
    if let Some(suffix) = base
        .strip_prefix("com")
        .or_else(|| base.strip_prefix("lpt"))
    {
        return matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        );
    }
    false
}

/// Relative-path rule from the oracle's `pluginRelativePathError`.
pub fn plugin_relative_path_error(value: &str) -> Option<&'static str> {
    if value.is_empty() || value.starts_with('/') || value.starts_with('\\') {
        return Some("must be a non-empty relative path");
    }
    for segment in value.split(['/', '\\']) {
        if let Some(error) = plugin_path_segment_error(segment) {
            return Some(error);
        }
    }
    None
}

pub fn is_safe_plugin_relative_path(value: &str) -> bool {
    plugin_relative_path_error(value).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plugin_scope_serializes_lowercase() {
        assert_eq!(
            serde_json::to_value(PluginScope::Global).unwrap(),
            json!("global")
        );
        assert_eq!(
            serde_json::to_value(PluginScope::Project).unwrap(),
            json!("project")
        );
        assert_eq!(
            serde_json::from_value::<PluginScope>(json!("project")).unwrap(),
            PluginScope::Project
        );
        assert!(serde_json::from_value::<PluginScope>(json!("workspace")).is_err());
    }

    #[test]
    fn capability_as_str_covers_closed_set_of_ten() {
        assert_eq!(Capability::ALL.len(), 10);
        assert_eq!(Capability::EventsSubscribe.as_str(), "events:subscribe");
        assert_eq!(Capability::NotificationsShow.as_str(), "notifications:show");
        assert_eq!(Capability::Secrets.as_str(), "secrets");
        assert_eq!(Capability::SessionPush.as_str(), "session:push");
        assert_eq!(Capability::SettingsOwn.as_str(), "settings:own");
        assert_eq!(Capability::Storage.as_str(), "storage");
        assert_eq!(Capability::TerminalSend.as_str(), "terminal:send");
        assert_eq!(Capability::WorkerCall.as_str(), "worker:call");
        assert_eq!(Capability::WorkspaceOpenTab.as_str(), "workspace:open-tab");
        assert_eq!(Capability::WorkspaceRead.as_str(), "workspace:read");
        for capability in Capability::ALL {
            assert_eq!(Capability::from_kind(capability.as_str()), Some(capability));
        }
        assert_eq!(Capability::from_kind("net:fetch"), None);
    }

    #[test]
    fn capability_serializes_as_kind_object() {
        assert_eq!(
            serde_json::to_value(Capability::WorkspaceOpenTab).unwrap(),
            json!({ "kind": "workspace:open-tab" })
        );
        let parsed: Capability = serde_json::from_value(json!({ "kind": "storage" })).unwrap();
        assert_eq!(parsed, Capability::Storage);
    }

    #[test]
    fn capability_deserialization_is_strict() {
        assert!(serde_json::from_value::<Capability>(json!({ "kind": "net:fetch" })).is_err());
        assert!(
            serde_json::from_value::<Capability>(json!({ "kind": "storage", "hosts": [] }))
                .is_err()
        );
        assert!(serde_json::from_value::<Capability>(json!("storage")).is_err());
    }

    #[test]
    fn canonical_capability_set_dedups_and_sorts() {
        let capabilities = [
            Capability::Storage,
            Capability::WorkspaceRead,
            Capability::Storage,
            Capability::Secrets,
        ];
        assert_eq!(
            canonical_capability_set(&capabilities),
            r#"[{"kind":"secrets"},{"kind":"storage"},{"kind":"workspace:read"}]"#
        );
        assert_eq!(canonical_capability_set(&[]), "[]");
    }

    #[test]
    fn plugin_id_rules_match_oracle() {
        assert!(is_safe_plugin_id("demo"));
        assert!(is_safe_plugin_id("my-plugin-2"));
        assert!(is_safe_plugin_id(&"a".repeat(PLUGIN_ID_MAX_LENGTH)));
        assert!(!is_safe_plugin_id(&"a".repeat(PLUGIN_ID_MAX_LENGTH + 1)));
        assert!(!is_safe_plugin_id(""));
        assert!(!is_safe_plugin_id("Demo"));
        assert!(!is_safe_plugin_id("-demo"));
        assert!(!is_safe_plugin_id("demo-"));
        assert!(!is_safe_plugin_id("de--mo"));
        assert!(!is_safe_plugin_id("de mo"));
        assert!(!is_safe_plugin_id("de_mo"));
        for reserved in DANGEROUS_PLUGIN_NAMES {
            assert!(!is_safe_plugin_id(reserved));
        }
    }

    #[test]
    fn path_segment_errors_match_oracle_text() {
        assert_eq!(
            plugin_path_segment_error(".."),
            Some("empty and dot path segments are not allowed")
        );
        assert_eq!(
            plugin_path_segment_error("."),
            Some("empty and dot path segments are not allowed")
        );
        assert_eq!(
            plugin_path_segment_error(""),
            Some("empty and dot path segments are not allowed")
        );
        assert_eq!(
            plugin_path_segment_error("x."),
            Some("path segments may not end with a dot or space")
        );
        assert_eq!(
            plugin_path_segment_error("x "),
            Some("path segments may not end with a dot or space")
        );
        assert_eq!(
            plugin_path_segment_error("a:b"),
            Some("path segment contains a Windows-forbidden character or alternate-data-stream colon")
        );
        assert_eq!(
            plugin_path_segment_error("a\nb"),
            Some("path segment contains a Windows-forbidden character or alternate-data-stream colon")
        );
        assert_eq!(
            plugin_path_segment_error("con.txt"),
            Some("path segment is a Windows reserved device name")
        );
        assert_eq!(
            plugin_path_segment_error("COM1"),
            Some("path segment is a Windows reserved device name")
        );
        assert_eq!(
            plugin_path_segment_error("lpt\u{b9}"),
            Some("path segment is a Windows reserved device name")
        );
        assert_eq!(
            plugin_path_segment_error("clock$"),
            Some("path segment is a Windows reserved device name")
        );
        assert_eq!(plugin_path_segment_error("normal.txt"), None);
    }

    #[test]
    fn relative_path_rules_match_oracle() {
        assert!(is_safe_plugin_relative_path("main.mjs"));
        assert!(is_safe_plugin_relative_path("dist/app.js"));
        assert!(is_safe_plugin_relative_path("panels/dashboard.html"));
        assert!(is_safe_plugin_relative_path("a\\b"));
        assert!(!is_safe_plugin_relative_path(""));
        assert!(!is_safe_plugin_relative_path("/abs/path"));
        assert!(!is_safe_plugin_relative_path("\\abs"));
        assert!(!is_safe_plugin_relative_path("../escape"));
        assert!(!is_safe_plugin_relative_path("a/../b"));
        assert!(!is_safe_plugin_relative_path("./a"));
        assert!(!is_safe_plugin_relative_path("a//b"));
        assert!(!is_safe_plugin_relative_path("a:b"));
        assert!(!is_safe_plugin_relative_path("con.txt"));
        assert!(!is_safe_plugin_relative_path("lpt3"));
        assert!(!is_safe_plugin_relative_path("x."));
        assert!(!is_safe_plugin_relative_path("x "));
    }

    #[test]
    fn plugin_error_exposes_code_and_message() {
        let error = PluginError::new(IO_ERROR, "boom");
        assert_eq!(error.code(), "io_error");
        assert_eq!(error.message(), "boom");
        assert_eq!(error.to_string(), "boom");
    }
}
