//! Host API v0 method-spec table and parameter/result validation.
//!
//! Ported from `orca:src/shared/plugins/plugin-host-api.ts` (13 methods) plus
//! the fork's `workspace.openTab` (14 total). Every spec carries capability,
//! mutation, and panel metadata; the gate, the panel bridge action set, and
//! the worker SDK all derive from this single table. Params and results are
//! validated with the oracle's exact limits (JSON values in/out).

use serde_json::{Map, Value};

use crate::manifest::{PLUGIN_EVENT_NAMES, PLUGIN_EVENT_SUBSCRIPTION_LIMIT};
use crate::secrets::SECRET_VALUE_MAX_BYTES;
use crate::storage::{validate_storage_key, STORAGE_KEY_LIMIT};
use crate::types::Capability;

pub const PANEL_ACTION_TEXT_MAX: usize = 4096;
pub const WORKSPACE_TERMINAL_LIMIT: usize = 50;
pub const WORKSPACE_LABEL_MAX_CHARS: usize = 512;
pub const TERMINAL_ID_MAX_CHARS: usize = 1024;
pub const NOTIFICATION_TITLE_MAX_CHARS: usize = 120;
pub const NOTIFICATION_BODY_MAX_CHARS: usize = 1000;
pub const OPEN_TAB_TITLE_MAX_CHARS: usize = 128;
pub const OPEN_TAB_ID_MAX_CHARS: usize = 256;
pub const OPEN_TAB_PARAMS_MAX_BYTES: usize = 64 * 1024;
pub const OPEN_TAB_PARAMS_MAX_DEPTH: usize = 64;

pub type Validator = fn(&Value) -> Result<Value, String>;

pub struct MethodSpec {
    pub name: &'static str,
    pub capability: Capability,
    pub mutation: bool,
    pub panel_callable: bool,
    pub params: Validator,
    pub result: Validator,
}

static SPECS: [MethodSpec; 14] = [
    MethodSpec {
        name: "workspace.readContext",
        capability: Capability::WorkspaceRead,
        mutation: false,
        panel_callable: true,
        params: empty_object_params,
        result: read_context_result,
    },
    MethodSpec {
        name: "workspace.openTab",
        capability: Capability::WorkspaceOpenTab,
        mutation: true,
        panel_callable: true,
        params: open_tab_params,
        result: open_tab_result,
    },
    MethodSpec {
        name: "terminal.sendText",
        capability: Capability::TerminalSend,
        mutation: true,
        panel_callable: true,
        params: send_text_params,
        result: send_text_result,
    },
    MethodSpec {
        name: "notifications.show",
        capability: Capability::NotificationsShow,
        mutation: true,
        panel_callable: true,
        params: notification_params,
        result: notification_result,
    },
    MethodSpec {
        name: "storage.get",
        capability: Capability::Storage,
        mutation: false,
        panel_callable: false,
        params: key_only_params,
        result: storage_get_result,
    },
    MethodSpec {
        name: "storage.set",
        capability: Capability::Storage,
        mutation: true,
        panel_callable: false,
        params: key_value_params,
        result: ok_result,
    },
    MethodSpec {
        name: "storage.delete",
        capability: Capability::Storage,
        mutation: true,
        panel_callable: false,
        params: key_only_params,
        result: ok_result,
    },
    MethodSpec {
        name: "storage.keys",
        capability: Capability::Storage,
        mutation: false,
        panel_callable: false,
        params: empty_object_params,
        result: storage_keys_result,
    },
    MethodSpec {
        name: "secrets.get",
        capability: Capability::Secrets,
        mutation: false,
        panel_callable: false,
        params: key_only_params,
        result: secrets_get_result,
    },
    MethodSpec {
        name: "secrets.set",
        capability: Capability::Secrets,
        mutation: true,
        panel_callable: false,
        params: secret_set_params,
        result: ok_result,
    },
    MethodSpec {
        name: "secrets.delete",
        capability: Capability::Secrets,
        mutation: true,
        panel_callable: false,
        params: key_only_params,
        result: ok_result,
    },
    MethodSpec {
        name: "settings.get",
        capability: Capability::SettingsOwn,
        mutation: false,
        panel_callable: false,
        params: empty_object_params,
        result: settings_get_result,
    },
    MethodSpec {
        name: "settings.set",
        capability: Capability::SettingsOwn,
        mutation: true,
        panel_callable: false,
        params: key_value_params,
        result: ok_result,
    },
    MethodSpec {
        name: "events.subscribe",
        capability: Capability::EventsSubscribe,
        mutation: false,
        panel_callable: false,
        params: events_subscribe_params,
        result: events_subscribe_result,
    },
];

pub fn specs() -> &'static [MethodSpec; 14] {
    &SPECS
}

pub fn spec(name: &str) -> Option<&'static MethodSpec> {
    SPECS.iter().find(|entry| entry.name == name)
}

fn invalid(path: &str, message: impl std::fmt::Display) -> String {
    format!("{path}: {message}")
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn max_utf16(value: &str, max: usize, path: &str) -> Result<(), String> {
    if utf16_len(value) > max {
        return Err(invalid(
            path,
            format!("must contain at most {max} characters"),
        ));
    }
    Ok(())
}

fn non_empty(value: &str, path: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(invalid(path, "must contain at least 1 character"));
    }
    Ok(())
}

fn require_object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| invalid(path, "expected an object"))
}

fn reject_extra_keys(obj: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), String> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid(path, format!("unknown field: {key}")));
        }
    }
    Ok(())
}

fn require_string<'a>(obj: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    obj.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field, "expected a string"))
}

fn optional_string<'a>(
    obj: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, String> {
    match obj.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(invalid(field, "expected a string")),
    }
}

fn require_storage_key<'a>(obj: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    let key = require_string(obj, field)?;
    validate_storage_key(key).map_err(|error| invalid(field, error.message()))?;
    Ok(key)
}

fn empty_object_params(value: &Value) -> Result<Value, String> {
    match value {
        Value::Null => Ok(serde_json::json!({})),
        Value::Object(map) if map.is_empty() => Ok(serde_json::json!({})),
        Value::Object(map) => {
            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
            Err(invalid(
                "(root)",
                format!("unknown fields are not allowed: {}", keys.join(", ")),
            ))
        }
        _ => Err(invalid("(root)", "expected an object")),
    }
}

fn key_only_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let key = require_storage_key(obj, "key")?;
    Ok(serde_json::json!({ "key": key }))
}

fn key_value_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let key = require_storage_key(obj, "key")?;
    let stored = obj
        .get("value")
        .ok_or_else(|| invalid("value", "required"))?
        .clone();
    Ok(serde_json::json!({ "key": key, "value": stored }))
}

fn secret_set_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let key = require_storage_key(obj, "key")?;
    let secret = require_string(obj, "value")?;
    if secret.len() > SECRET_VALUE_MAX_BYTES {
        return Err(invalid(
            "value",
            format!("must contain at most {SECRET_VALUE_MAX_BYTES} bytes"),
        ));
    }
    Ok(serde_json::json!({ "key": key, "value": secret }))
}

fn send_text_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let terminal_id = require_string(obj, "terminalId")?;
    non_empty(terminal_id, "terminalId")?;
    max_utf16(terminal_id, TERMINAL_ID_MAX_CHARS, "terminalId")?;
    let text = require_string(obj, "text")?;
    non_empty(text, "text")?;
    max_utf16(text, PANEL_ACTION_TEXT_MAX, "text")?;
    let enter = match obj.get("enter") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(invalid("enter", "expected a boolean")),
    };
    Ok(serde_json::json!({ "terminalId": terminal_id, "text": text, "enter": enter }))
}

fn notification_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let title = require_string(obj, "title")?;
    non_empty(title, "title")?;
    max_utf16(title, NOTIFICATION_TITLE_MAX_CHARS, "title")?;
    let body = optional_string(obj, "body")?;
    if let Some(body) = body {
        max_utf16(body, NOTIFICATION_BODY_MAX_CHARS, "body")?;
    }
    let mut normalized = Map::new();
    normalized.insert("title".to_string(), Value::String(title.to_string()));
    if let Some(body) = body {
        normalized.insert("body".to_string(), Value::String(body.to_string()));
    }
    Ok(Value::Object(normalized))
}

fn events_subscribe_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let events = obj
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("events", "expected an array of event names"))?;
    if events.is_empty() {
        return Err(invalid("events", "must contain at least 1 event"));
    }
    if events.len() > PLUGIN_EVENT_SUBSCRIPTION_LIMIT {
        return Err(invalid(
            "events",
            format!("must contain at most {PLUGIN_EVENT_SUBSCRIPTION_LIMIT} events"),
        ));
    }
    for (index, event) in events.iter().enumerate() {
        let name = event
            .as_str()
            .ok_or_else(|| invalid(&format!("events.{index}"), "expected a string"))?;
        if !PLUGIN_EVENT_NAMES.contains(&name) {
            return Err(invalid(
                &format!("events.{index}"),
                format!("unknown event: {name}"),
            ));
        }
    }
    Ok(serde_json::json!({ "events": events }))
}

fn open_tab_params(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    let tab_id = require_string(obj, "tabId")?;
    non_empty(tab_id, "tabId")?;
    max_utf16(tab_id, OPEN_TAB_ID_MAX_CHARS, "tabId")?;
    let title = optional_string(obj, "title")?;
    if let Some(title) = title {
        max_utf16(title, OPEN_TAB_TITLE_MAX_CHARS, "title")?;
    }
    let params = match obj.get("params") {
        None | Some(Value::Null) => None,
        Some(params) => {
            check_json_bounds(params, "params")?;
            Some(params.clone())
        }
    };
    let mut normalized = Map::new();
    normalized.insert("tabId".to_string(), Value::String(tab_id.to_string()));
    if let Some(title) = title {
        normalized.insert("title".to_string(), Value::String(title.to_string()));
    }
    if let Some(params) = params {
        normalized.insert("params".to_string(), params);
    }
    Ok(Value::Object(normalized))
}

fn check_json_bounds(value: &Value, path: &str) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| invalid(path, error))?;
    if bytes.len() > OPEN_TAB_PARAMS_MAX_BYTES {
        return Err(invalid(
            path,
            format!("must serialize to at most {OPEN_TAB_PARAMS_MAX_BYTES} bytes"),
        ));
    }
    if json_depth(value) > OPEN_TAB_PARAMS_MAX_DEPTH {
        return Err(invalid(
            path,
            format!("must not be nested deeper than {OPEN_TAB_PARAMS_MAX_DEPTH} levels"),
        ));
    }
    Ok(())
}

fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(json_depth).max().unwrap_or(0),
        _ => 1,
    }
}

fn read_context_result(value: &Value) -> Result<Value, String> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["branch", "displayName", "terminals"], "(root)")?;
    let branch = require_string(obj, "branch")?;
    max_utf16(branch, WORKSPACE_LABEL_MAX_CHARS, "branch")?;
    let display_name = require_string(obj, "displayName")?;
    max_utf16(display_name, WORKSPACE_LABEL_MAX_CHARS, "displayName")?;
    let terminals = obj
        .get("terminals")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("terminals", "expected an array"))?;
    if terminals.len() > WORKSPACE_TERMINAL_LIMIT {
        return Err(invalid(
            "terminals",
            format!("must contain at most {WORKSPACE_TERMINAL_LIMIT} terminals"),
        ));
    }
    let mut projected = Vec::with_capacity(terminals.len());
    for (index, terminal) in terminals.iter().enumerate() {
        let path = format!("terminals.{index}");
        let terminal = require_object(terminal, &path)?;
        reject_extra_keys(terminal, &["id"], &path)?;
        let id = require_string(terminal, "id")?;
        non_empty(id, &format!("{path}.id"))?;
        max_utf16(id, TERMINAL_ID_MAX_CHARS, &format!("{path}.id"))?;
        projected.push(serde_json::json!({ "id": id }));
    }
    Ok(serde_json::json!({
        "branch": branch,
        "displayName": display_name,
        "terminals": projected
    }))
}

fn send_text_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["accepted"], "(root)")?;
    let accepted = obj
        .get("accepted")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("accepted", "expected a boolean"))?;
    Ok(serde_json::json!({ "accepted": accepted }))
}

fn notification_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["delivered"], "(root)")?;
    let delivered = obj
        .get("delivered")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("delivered", "expected a boolean"))?;
    Ok(serde_json::json!({ "delivered": delivered }))
}

fn storage_get_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["value"], "(root)")?;
    let stored = obj
        .get("value")
        .cloned()
        .ok_or_else(|| invalid("value", "required"))?;
    Ok(serde_json::json!({ "value": stored }))
}

fn ok_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["ok"], "(root)")?;
    match obj.get("ok") {
        Some(Value::Bool(true)) => Ok(serde_json::json!({ "ok": true })),
        _ => Err(invalid("ok", "expected true")),
    }
}

fn storage_keys_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["keys"], "(root)")?;
    let keys = obj
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("keys", "expected an array"))?;
    if keys.len() > STORAGE_KEY_LIMIT {
        return Err(invalid(
            "keys",
            format!("must contain at most {STORAGE_KEY_LIMIT} keys"),
        ));
    }
    for (index, key) in keys.iter().enumerate() {
        if !key.is_string() {
            return Err(invalid(&format!("keys.{index}"), "expected a string"));
        }
    }
    Ok(serde_json::json!({ "keys": keys }))
}

fn secrets_get_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["value"], "(root)")?;
    match obj.get("value") {
        Some(Value::Null) => Ok(serde_json::json!({ "value": null })),
        Some(Value::String(secret)) => Ok(serde_json::json!({ "value": secret })),
        _ => Err(invalid("value", "expected a string or null")),
    }
}

fn settings_get_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["settings"], "(root)")?;
    let settings = obj
        .get("settings")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("settings", "expected an object"))?;
    Ok(serde_json::json!({ "settings": settings }))
}

fn events_subscribe_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["subscribed"], "(root)")?;
    let subscribed = obj
        .get("subscribed")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("subscribed", "expected an array"))?;
    for (index, event) in subscribed.iter().enumerate() {
        let name = event
            .as_str()
            .ok_or_else(|| invalid(&format!("subscribed.{index}"), "expected a string"))?;
        if !PLUGIN_EVENT_NAMES.contains(&name) {
            return Err(invalid(
                &format!("subscribed.{index}"),
                format!("unknown event: {name}"),
            ));
        }
    }
    Ok(serde_json::json!({ "subscribed": subscribed }))
}

fn open_tab_result(value: &Value) -> Result<Value, String> {
    let obj = require_object(value, "(root)")?;
    reject_extra_keys(obj, &["instanceId", "worktreeId"], "(root)")?;
    let instance_id = require_string(obj, "instanceId")?;
    non_empty(instance_id, "instanceId")?;
    let worktree_id = require_string(obj, "worktreeId")?;
    non_empty(worktree_id, "worktreeId")?;
    Ok(serde_json::json!({ "instanceId": instance_id, "worktreeId": worktree_id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn validate_params(name: &str, params: Value) -> Result<Value, String> {
        (spec(name).expect("known method").params)(&params)
    }

    fn validate_result(name: &str, result: Value) -> Result<Value, String> {
        (spec(name).expect("known method").result)(&result)
    }

    #[test]
    fn spec_table_has_fourteen_methods_with_expected_metadata() {
        assert_eq!(specs().len(), 14);
        let names: Vec<&str> = specs().iter().map(|entry| entry.name).collect();
        assert_eq!(
            names,
            vec![
                "workspace.readContext",
                "workspace.openTab",
                "terminal.sendText",
                "notifications.show",
                "storage.get",
                "storage.set",
                "storage.delete",
                "storage.keys",
                "secrets.get",
                "secrets.set",
                "secrets.delete",
                "settings.get",
                "settings.set",
                "events.subscribe",
            ]
        );
        let panel: Vec<&str> = specs()
            .iter()
            .filter(|entry| entry.panel_callable)
            .map(|entry| entry.name)
            .collect();
        assert_eq!(
            panel,
            vec![
                "workspace.readContext",
                "workspace.openTab",
                "terminal.sendText",
                "notifications.show",
            ]
        );
        for entry in specs() {
            assert!(spec(entry.name).is_some());
        }
        assert!(spec("workspace.explode").is_none());
        assert_eq!(
            spec("workspace.openTab").unwrap().capability,
            Capability::WorkspaceOpenTab
        );
        assert_eq!(
            spec("storage.keys").unwrap().capability,
            Capability::Storage
        );
        assert!(!spec("storage.get").unwrap().mutation);
        assert!(spec("storage.set").unwrap().mutation);
        assert!(spec("workspace.openTab").unwrap().mutation);
        assert!(!spec("workspace.readContext").unwrap().mutation);
    }

    #[test]
    fn empty_object_params_accept_missing_null_and_empty_but_reject_extras() {
        for method in ["workspace.readContext", "storage.keys", "settings.get"] {
            assert!(validate_params(method, Value::Null).is_ok(), "{method}");
            assert!(validate_params(method, json!({})).is_ok(), "{method}");
            let error = validate_params(method, json!({ "extra": 1 })).unwrap_err();
            assert!(error.contains("extra"), "{method}: {error}");
            assert!(validate_params(method, json!([])).is_err(), "{method}");
        }
    }

    #[test]
    fn send_text_params_enforce_oracle_limits_and_default_enter() {
        let normalized = validate_params(
            "terminal.sendText",
            json!({ "terminalId": "t1", "text": "hello" }),
        )
        .unwrap();
        assert_eq!(
            normalized,
            json!({ "terminalId": "t1", "text": "hello", "enter": false })
        );

        assert!(validate_params("terminal.sendText", json!({ "terminalId": "t1" })).is_err());
        assert!(validate_params(
            "terminal.sendText",
            json!({ "terminalId": "", "text": "x" })
        )
        .is_err());
        assert!(validate_params(
            "terminal.sendText",
            json!({ "terminalId": "t1", "text": "" })
        )
        .is_err());
        assert!(validate_params(
            "terminal.sendText",
            json!({ "terminalId": "t1", "text": "x".repeat(PANEL_ACTION_TEXT_MAX + 1) })
        )
        .is_err());
        assert!(validate_params(
            "terminal.sendText",
            json!({ "terminalId": "t", "text": "x", "enter": "yes" })
        )
        .is_err());
        assert!(validate_params(
            "terminal.sendText",
            json!({ "terminalId": "t".repeat(TERMINAL_ID_MAX_CHARS + 1), "text": "x" })
        )
        .is_err());
    }

    #[test]
    fn notification_params_enforce_title_and_body_caps() {
        assert!(validate_params("notifications.show", json!({ "title": "Build done" })).is_ok());
        assert!(validate_params(
            "notifications.show",
            json!({ "title": "x".repeat(NOTIFICATION_TITLE_MAX_CHARS + 1) })
        )
        .is_err());
        assert!(validate_params("notifications.show", json!({ "title": "" })).is_err());
        assert!(validate_params("notifications.show", json!({ "body": "b" })).is_err());
        assert!(validate_params(
            "notifications.show",
            json!({ "title": "t", "body": "x".repeat(NOTIFICATION_BODY_MAX_CHARS + 1) })
        )
        .is_err());
        assert!(validate_params(
            "notifications.show",
            json!({ "title": "t", "body": "x".repeat(NOTIFICATION_BODY_MAX_CHARS) })
        )
        .is_ok());
    }

    #[test]
    fn storage_and_settings_keys_use_the_reserved_key_schema() {
        for method in [
            "storage.get",
            "storage.set",
            "storage.delete",
            "secrets.get",
            "secrets.set",
            "secrets.delete",
            "settings.set",
        ] {
            for reserved in ["__proto__", "prototype", "constructor"] {
                let params = if method.ends_with("set") {
                    json!({ "key": reserved, "value": "v" })
                } else {
                    json!({ "key": reserved })
                };
                let error = validate_params(method, params).unwrap_err();
                assert_eq!(error, "key: reserved storage key", "{method}");
            }
        }
        assert!(validate_params("storage.get", json!({ "key": "" })).is_err());
        assert!(validate_params("storage.get", json!({ "key": "x".repeat(257) })).is_err());
        assert!(validate_params("storage.get", json!({ "key": "x".repeat(256) })).is_ok());
    }

    #[test]
    fn storage_values_accept_any_json_and_secrets_are_strings_with_a_64kib_cap() {
        assert!(validate_params(
            "storage.set",
            json!({ "key": "k", "value": {"deep": [1, 2]} })
        )
        .is_ok());
        assert!(validate_params("storage.set", json!({ "key": "k" })).is_err());
        assert!(validate_params("secrets.set", json!({ "key": "k", "value": 1 })).is_err());
        assert!(validate_params(
            "secrets.set",
            json!({ "key": "k", "value": "x".repeat(SECRET_VALUE_MAX_BYTES + 1) })
        )
        .is_err());
        assert!(validate_params(
            "secrets.set",
            json!({ "key": "k", "value": "x".repeat(SECRET_VALUE_MAX_BYTES) })
        )
        .is_ok());
    }

    #[test]
    fn events_subscribe_accepts_one_to_three_known_events() {
        let normalized = validate_params(
            "events.subscribe",
            json!({ "events": ["worktree.created"] }),
        )
        .unwrap();
        assert_eq!(normalized, json!({ "events": ["worktree.created"] }));
        assert!(validate_params("events.subscribe", json!({ "events": [] })).is_err());
        assert!(validate_params(
            "events.subscribe",
            json!({ "events": ["worktree.created", "worktree.removed", "agent.status.changed"] })
        )
        .is_ok());
        assert!(validate_params("events.subscribe", json!({ "events": ["net.fetch"] })).is_err());
        assert!(validate_params(
            "events.subscribe",
            json!({ "events": ["worktree.created", "worktree.created", "worktree.removed", "agent.status.changed"] })
        )
        .is_err());
        assert_eq!(PLUGIN_EVENT_SUBSCRIPTION_LIMIT, 3);
        assert_eq!(PLUGIN_EVENT_NAMES.len(), 3);
    }

    #[test]
    fn open_tab_params_validate_tab_id_title_and_json_bounds() {
        assert_eq!(
            validate_params("workspace.openTab", json!({ "tabId": "settings" })).unwrap(),
            json!({ "tabId": "settings" })
        );
        assert!(validate_params("workspace.openTab", json!({})).is_err());
        assert!(validate_params("workspace.openTab", json!({ "tabId": "" })).is_err());
        assert!(validate_params(
            "workspace.openTab",
            json!({ "tabId": "x".repeat(OPEN_TAB_ID_MAX_CHARS + 1) })
        )
        .is_err());
        assert!(validate_params(
            "workspace.openTab",
            json!({ "tabId": "t", "title": "x".repeat(OPEN_TAB_TITLE_MAX_CHARS + 1) })
        )
        .is_err());
        assert!(validate_params(
            "workspace.openTab",
            json!({ "tabId": "t", "title": "x".repeat(OPEN_TAB_TITLE_MAX_CHARS) })
        )
        .is_ok());

        let big = "x".repeat(OPEN_TAB_PARAMS_MAX_BYTES);
        assert!(
            validate_params("workspace.openTab", json!({ "tabId": "t", "params": big })).is_err()
        );
        let ok = "x".repeat(OPEN_TAB_PARAMS_MAX_BYTES - 64);
        assert!(
            validate_params("workspace.openTab", json!({ "tabId": "t", "params": ok })).is_ok()
        );
    }

    #[test]
    fn result_validators_are_strict() {
        assert!(validate_result(
            "workspace.readContext",
            json!({
                "branch": "main",
                "displayName": "demo",
                "terminals": [{ "id": "t1" }]
            })
        )
        .is_ok());
        assert!(validate_result("workspace.readContext", Value::Null).is_ok());
        assert!(validate_result(
            "workspace.readContext",
            json!({ "branch": "main", "displayName": "demo", "terminals": [{"id": ""}] })
        )
        .is_err());
        assert!(validate_result(
            "workspace.readContext",
            json!({ "branch": "main", "displayName": "demo", "terminals": [], "extra": 1 })
        )
        .is_err());

        assert!(validate_result("terminal.sendText", json!({ "accepted": true })).is_ok());
        assert!(validate_result("terminal.sendText", json!({ "accepted": 1 })).is_err());
        assert!(validate_result("notifications.show", json!({ "delivered": false })).is_ok());
        assert!(validate_result("storage.get", json!({ "value": null })).is_ok());
        assert!(validate_result("storage.set", json!({ "ok": true })).is_ok());
        assert!(validate_result("storage.set", json!({ "ok": false })).is_err());
        assert!(validate_result("storage.delete", json!({ "ok": true })).is_ok());
        assert!(validate_result("storage.keys", json!({ "keys": ["a", "b"] })).is_ok());
        let overflow: Vec<String> = (0..STORAGE_KEY_LIMIT + 1)
            .map(|i| format!("k{i}"))
            .collect();
        assert!(validate_result("storage.keys", json!({ "keys": overflow })).is_err());
        assert!(validate_result("secrets.get", json!({ "value": "v" })).is_ok());
        assert!(validate_result("secrets.get", json!({ "value": null })).is_ok());
        assert!(validate_result("secrets.get", json!({ "value": 1 })).is_err());
        assert!(validate_result("secrets.set", json!({ "ok": true })).is_ok());
        assert!(validate_result("settings.get", json!({ "settings": { "a": 1 } })).is_ok());
        assert!(validate_result("settings.get", json!({ "settings": [] })).is_err());
        assert!(validate_result("settings.set", json!({ "ok": true })).is_ok());
        assert!(validate_result(
            "events.subscribe",
            json!({ "subscribed": ["worktree.created"] })
        )
        .is_ok());
        assert!(
            validate_result("events.subscribe", json!({ "subscribed": ["net.fetch"] })).is_err()
        );
        assert!(validate_result(
            "workspace.openTab",
            json!({ "instanceId": "i1", "worktreeId": "w1" })
        )
        .is_ok());
        assert!(validate_result("workspace.openTab", json!({ "instanceId": "" })).is_err());
    }
}
