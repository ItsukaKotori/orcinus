//! The one chokepoint every plugin-originated host call runs through.
//!
//! Ported from `orca:src/main/plugins/plugin-host-methods.ts`: gate → spec
//! lookup → param validation → mutation audit (`attempt` before the handler,
//! `ok`/`error` after) → handler → result validation. Handlers delegate to
//! [`PluginHostServices`]; a result that fails its spec schema fails closed as
//! `action_failed` instead of leaking an unvalidated shape.

use serde_json::Value;

use crate::audit::{AuditOutcome, AuditRecord, AUDIT_ACTOR_PREFIX};
use crate::host::api;
use crate::host::gate::{self, GateDecision};
use crate::host::services::{HostServiceError, PluginHostServices, WorktreeContext};
use crate::types::{
    Capability, PluginError, PluginScope, ACTION_FAILED, INVALID_PARAMS, UNAVAILABLE,
};
use crate::worker::manager::UNKNOWN_COMMAND;

/// Mutation audit sink. [`crate::audit::AuditLog`] is the production
/// implementation; tests bind recorders and failing fakes.
pub trait PluginAuditSink: Send + Sync {
    fn record(&self, record: AuditRecord) -> Result<(), PluginError>;
}

impl PluginAuditSink for crate::audit::AuditLog {
    fn record(&self, record: AuditRecord) -> Result<(), PluginError> {
        crate::audit::AuditLog::record(self, record)
    }
}

pub struct HostCallContext<'a> {
    /// Qualified plugin key, bound host-side from authenticated identity.
    pub plugin_key: &'a str,
    /// Consented capabilities; `None` = unknown/disabled/consent-stale.
    pub granted: Option<&'a [Capability]>,
    /// True when the call arrives over the sandboxed panel bridge.
    pub via_panel: bool,
    /// Panel-session token when the call arrives over a UI bridge; `None` for
    /// worker-originated calls.
    pub session_token: Option<&'a str>,
    /// Scope the caller's plugin is loaded under (needed for `openTab`).
    pub scope: PluginScope,
    /// Project id when `scope` is project.
    pub repo_id: Option<&'a str>,
    /// Worktree bound by a tab session; `None` falls back to the active one.
    pub worktree_hint: Option<&'a str>,
    pub services: &'a dyn PluginHostServices,
    pub audit: Option<&'a dyn PluginAuditSink>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Ok(Value),
    Err { code: String, error: String },
}

impl Outcome {
    pub fn ok(value: Value) -> Self {
        Outcome::Ok(value)
    }

    pub fn err(code: impl Into<String>, error: impl Into<String>) -> Self {
        Outcome::Err {
            code: code.into(),
            error: error.into(),
        }
    }
}

pub fn execute(method: &str, params: Value, ctx: &HostCallContext<'_>) -> Outcome {
    match gate::decide(ctx.granted, ctx.via_panel, method) {
        GateDecision::Granted => {}
        GateDecision::Denied { code, error } => return Outcome::err(code, error),
    }
    let Some(spec) = api::spec(method) else {
        return Outcome::err(
            gate::UNKNOWN_METHOD,
            format!("unknown host method: {method}"),
        );
    };
    let parsed = match (spec.params)(&params) {
        Ok(parsed) => parsed,
        Err(error) => return Outcome::err(INVALID_PARAMS, error),
    };
    if spec.mutation {
        let Some(audit) = ctx.audit else {
            return Outcome::err(UNAVAILABLE, "mutation audit log is not available");
        };
        if audit
            .record(audit_record(ctx, method, &parsed, AuditOutcome::Attempt))
            .is_err()
        {
            return Outcome::err(ACTION_FAILED, "mutation audit log could not be written");
        }
    }
    let value = match dispatch(method, &parsed, ctx) {
        Ok(value) => value,
        Err(error) => {
            record_outcome(ctx, method, &parsed, AuditOutcome::Error);
            return Outcome::err(error.code, error.message);
        }
    };
    match (spec.result)(&value) {
        Ok(validated) => {
            record_outcome(ctx, method, &parsed, AuditOutcome::Ok);
            Outcome::ok(validated)
        }
        Err(_) => {
            record_outcome(ctx, method, &parsed, AuditOutcome::Error);
            Outcome::err(
                ACTION_FAILED,
                format!("internal: malformed {method} result"),
            )
        }
    }
}

fn audit_record(
    ctx: &HostCallContext<'_>,
    method: &str,
    params: &Value,
    outcome: AuditOutcome,
) -> AuditRecord {
    AuditRecord {
        ts: now_millis(),
        actor: format!("{AUDIT_ACTOR_PREFIX}{}", ctx.plugin_key),
        method: method.to_string(),
        summary: summarize_params(method, params),
        outcome,
    }
}

fn record_outcome(ctx: &HostCallContext<'_>, method: &str, params: &Value, outcome: AuditOutcome) {
    if !api::spec(method).map(|spec| spec.mutation).unwrap_or(false) {
        return;
    }
    if let Some(audit) = ctx.audit {
        let _ = audit.record(audit_record(ctx, method, params, outcome));
    }
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Bounded, content-free summaries for the audit log.
fn summarize_params(method: &str, params: &Value) -> String {
    match method {
        "terminal.sendText" => {
            let text = field_str(params, "text");
            let enter = params
                .get("enter")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            format!(
                "terminal={} bytes={} enter={enter}",
                field_str(params, "terminalId"),
                text.len()
            )
        }
        "notifications.show" => format!("titleChars={}", utf16_len(field_str(params, "title"))),
        "storage.set" | "storage.delete" | "secrets.set" | "secrets.delete" | "settings.set" => {
            format!("key={}", field_str(params, "key"))
        }
        "workspace.openTab" => format!("tab={}", field_str(params, "tabId")),
        _ => String::new(),
    }
}

struct HandlerError {
    code: &'static str,
    message: String,
}

impl HandlerError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<PluginError> for HandlerError {
    fn from(error: PluginError) -> Self {
        Self {
            code: error.code(),
            message: error.message().to_string(),
        }
    }
}

impl From<HostServiceError> for HandlerError {
    fn from(error: HostServiceError) -> Self {
        Self {
            code: error.code(),
            message: error.message().to_string(),
        }
    }
}

fn dispatch(
    method: &str,
    params: &Value,
    ctx: &HostCallContext<'_>,
) -> Result<Value, HandlerError> {
    match method {
        "workspace.readContext" => read_context(ctx),
        "workspace.openTab" => open_tab(params, ctx),
        "terminal.sendText" => send_text(params, ctx),
        "notifications.show" => show_notification(params, ctx),
        "storage.get" => storage_get(params, ctx),
        "storage.set" => storage_set(params, ctx),
        "storage.delete" => storage_delete(params, ctx),
        "storage.keys" => storage_keys(ctx),
        "secrets.get" => secrets_get(params, ctx),
        "secrets.set" => secrets_set(params, ctx),
        "secrets.delete" => secrets_delete(params, ctx),
        "settings.get" => settings_get(ctx),
        "settings.set" => settings_set(params, ctx),
        "events.subscribe" => events_subscribe(params, ctx),
        "session.post" => session_post(params, ctx),
        "worker.invokeCommand" => invoke_worker_command(params, ctx),
        _ => Err(HandlerError::new(
            gate::UNKNOWN_METHOD,
            format!("unknown host method: {method}"),
        )),
    }
}

/// Worktree the call is scoped to: the tab session's hint, else the focused
/// worktree.
fn resolve_worktree(ctx: &HostCallContext<'_>) -> Option<String> {
    if let Some(hint) = ctx.worktree_hint {
        return Some(hint.to_string());
    }
    ctx.services
        .active_worktree_context()
        .map(|context| context.worktree_id)
}

fn read_context(ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let Some(worktree_id) = resolve_worktree(ctx) else {
        return Ok(Value::Null);
    };
    let terminals = ctx.services.list_worktree_terminals(&worktree_id);
    // Why: worktree ids embed provider paths, so the public projection must
    // select safe fields instead of spreading the internal context.
    let (branch, display_name) = ctx
        .services
        .active_worktree_context()
        .filter(|context| context.worktree_id == worktree_id)
        .map(
            |WorktreeContext {
                 branch,
                 display_name,
                 ..
             }| (branch, display_name),
        )
        .unwrap_or_default();
    let terminals: Vec<Value> = terminals
        .into_iter()
        .filter(|terminal| {
            !terminal.id.is_empty() && utf16_len(&terminal.id) <= api::TERMINAL_ID_MAX_CHARS
        })
        .take(api::WORKSPACE_TERMINAL_LIMIT)
        .map(|terminal| serde_json::json!({ "id": terminal.id }))
        .collect();
    Ok(serde_json::json!({
        "branch": truncate_utf16(&branch, api::WORKSPACE_LABEL_MAX_CHARS),
        "displayName": truncate_utf16(&display_name, api::WORKSPACE_LABEL_MAX_CHARS),
        "terminals": terminals
    }))
}

fn open_tab(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let tab_id = field_str(params, "tabId");
    let declared = ctx
        .services
        .declared_tabs(ctx.plugin_key, ctx.scope, ctx.repo_id);
    if !declared.iter().any(|declared| declared == tab_id) {
        return Err(HandlerError::new(
            INVALID_PARAMS,
            format!("tab is not declared by the plugin: {tab_id}"),
        ));
    }
    let Some(worktree_id) = resolve_worktree(ctx) else {
        return Err(HandlerError::new(
            ACTION_FAILED,
            "no active worktree is available to open a tab",
        ));
    };
    let title = params.get("title").and_then(Value::as_str);
    let tab_params = params
        .get("params")
        .filter(|value| !value.is_null())
        .cloned();
    let result = ctx.services.open_tab(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        tab_id,
        title,
        tab_params,
        Some(&worktree_id),
    )?;
    Ok(serde_json::json!({
        "instanceId": result.instance_id,
        "worktreeId": result.worktree_id
    }))
}

fn send_text(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let terminal_id = field_str(params, "terminalId");
    let Some(worktree_id) = resolve_worktree(ctx) else {
        return Err(HandlerError::new(
            ACTION_FAILED,
            "no active worktree is available for terminal input",
        ));
    };
    // Why: terminal handles are provider-owned and can outlive focus changes;
    // re-list the resolved worktree immediately before routing plugin input.
    let terminals = ctx.services.list_worktree_terminals(&worktree_id);
    if !terminals.iter().any(|terminal| terminal.id == terminal_id) {
        return Err(HandlerError::new(
            ACTION_FAILED,
            "terminal is outside the active worktree",
        ));
    }
    let enter = params
        .get("enter")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let accepted =
        ctx.services
            .send_terminal_text(terminal_id, field_str(params, "text"), enter)?;
    Ok(serde_json::json!({ "accepted": accepted }))
}

fn show_notification(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let body = params.get("body").and_then(Value::as_str);
    let delivered =
        ctx.services
            .dispatch_notification(ctx.plugin_key, field_str(params, "title"), body)?;
    Ok(serde_json::json!({ "delivered": delivered }))
}

fn storage_get(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let value = ctx.services.storage().get(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        field_str(params, "key"),
    )?;
    Ok(serde_json::json!({ "value": value.unwrap_or(Value::Null) }))
}

fn storage_set(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    ctx.services.storage().set(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        field_str(params, "key"),
        field(params, "value").clone(),
    )?;
    Ok(serde_json::json!({ "ok": true }))
}

fn storage_delete(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    ctx.services.storage().delete(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        field_str(params, "key"),
    )?;
    Ok(serde_json::json!({ "ok": true }))
}

fn storage_keys(ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let keys = ctx
        .services
        .storage()
        .keys(ctx.plugin_key, ctx.scope, ctx.repo_id)?;
    Ok(serde_json::json!({ "keys": keys }))
}

fn secrets_get(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let value = ctx.services.secrets().get(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        field_str(params, "key"),
    )?;
    Ok(serde_json::json!({ "value": value }))
}

fn secrets_set(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    ctx.services.secrets().set(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        field_str(params, "key"),
        field_str(params, "value"),
    )?;
    Ok(serde_json::json!({ "ok": true }))
}

fn secrets_delete(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    ctx.services.secrets().delete(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        field_str(params, "key"),
    )?;
    Ok(serde_json::json!({ "ok": true }))
}

fn settings_get(ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let settings = ctx
        .services
        .settings()
        .get_all(ctx.plugin_key, ctx.scope, ctx.repo_id)?;
    Ok(serde_json::json!({ "settings": settings }))
}

fn settings_set(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    ctx.services.settings().set(
        ctx.plugin_key,
        ctx.scope,
        ctx.repo_id,
        field_str(params, "key"),
        field(params, "value").clone(),
    )?;
    Ok(serde_json::json!({ "ok": true }))
}

fn events_subscribe(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let events: Vec<String> = params
        .get("events")
        .and_then(Value::as_array)
        .map(|events| {
            events
                .iter()
                .filter_map(|event| event.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let subscribed = ctx.services.subscribe_events(ctx.plugin_key, &events);
    Ok(serde_json::json!({ "subscribed": subscribed }))
}

fn session_post(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let topic = params.get("topic").and_then(Value::as_str);
    let payload = field(params, "payload").clone();
    ctx.services
        .session_post(ctx.plugin_key, field_str(params, "sessionToken"), topic, payload)?;
    Ok(serde_json::json!({ "ok": true }))
}

fn invoke_worker_command(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let Some(session_token) = ctx.session_token.filter(|_| ctx.via_panel) else {
        return Err(HandlerError::new(
            UNAVAILABLE,
            "worker.invokeCommand requires a UI session context",
        ));
    };
    let command_id = field_str(params, "commandId");
    let args = params.get("args").filter(|value| !value.is_null()).cloned();
    let value = ctx
        .services
        .invoke_worker_command(
            ctx.plugin_key,
            ctx.scope,
            ctx.repo_id,
            command_id,
            args,
            Some(session_token),
        )
        .map_err(|error| {
            if error.code() == UNKNOWN_COMMAND {
                HandlerError::new(INVALID_PARAMS, error.message().to_string())
            } else {
                error.into()
            }
        })?;
    let bytes = serde_json::to_vec(&value)
        .map_err(|error| HandlerError::new(INVALID_PARAMS, error.to_string()))?;
    if bytes.len() > api::WORKER_COMMAND_RESULT_MAX_BYTES {
        return Err(HandlerError::new(
            INVALID_PARAMS,
            format!(
                "command result exceeds {} bytes; use session.post to stream large payloads",
                api::WORKER_COMMAND_RESULT_MAX_BYTES
            ),
        ));
    }
    Ok(value)
}

fn field<'a>(params: &'a Value, name: &str) -> &'a Value {
    params.get(name).unwrap_or(&Value::Null)
}

fn field_str<'a>(params: &'a Value, name: &str) -> &'a str {
    params.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn truncate_utf16(value: &str, max: usize) -> String {
    if utf16_len(value) <= max {
        return value.to_string();
    }
    let mut truncated = String::new();
    let mut used = 0;
    for character in value.chars() {
        let width = character.len_utf16();
        if used + width > max {
            break;
        }
        truncated.push(character);
        used += width;
    }
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::services::{
        OpenTabResult, PluginSettingsAccess, PluginStorageAccess, TerminalRef,
    };
    use crate::secrets::SecretStore;
    use serde_json::{json, Map};
    use std::collections::HashMap;
    use std::sync::Mutex;

    const KEY: &str = "orca-samples.demo";

    static ALL_CAPABILITIES: [Capability; 10] = Capability::ALL;

    #[derive(Default)]
    struct FakeStorage {
        map: Mutex<HashMap<String, Value>>,
        broken: Mutex<bool>,
    }

    impl PluginStorageAccess for FakeStorage {
        fn get(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            key: &str,
        ) -> Result<Option<Value>, PluginError> {
            if *self.broken.lock().unwrap() {
                return Err(PluginError::new(ACTION_FAILED, "storage is broken"));
            }
            Ok(self.map.lock().unwrap().get(key).cloned())
        }

        fn set(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            key: &str,
            value: Value,
        ) -> Result<(), PluginError> {
            if *self.broken.lock().unwrap() {
                return Err(PluginError::new(ACTION_FAILED, "storage is broken"));
            }
            self.map.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }

        fn delete(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            key: &str,
        ) -> Result<(), PluginError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }

        fn keys(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
        ) -> Result<Vec<String>, PluginError> {
            Ok(self.map.lock().unwrap().keys().cloned().collect())
        }
    }

    #[derive(Default)]
    struct FakeSettings {
        map: Mutex<HashMap<String, Value>>,
    }

    impl PluginSettingsAccess for FakeSettings {
        fn get_all(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
        ) -> Result<Map<String, Value>, PluginError> {
            Ok(self.map.lock().unwrap().clone().into_iter().collect())
        }

        fn set(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            key: &str,
            value: Value,
        ) -> Result<(), PluginError> {
            self.map.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSecrets {
        map: Mutex<HashMap<String, String>>,
    }

    impl SecretStore for FakeSecrets {
        fn get(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            key: &str,
        ) -> Result<Option<String>, PluginError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }

        fn set(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            key: &str,
            value: &str,
        ) -> Result<(), PluginError> {
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            key: &str,
        ) -> Result<(), PluginError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeServices {
        context: Option<WorktreeContext>,
        terminals: Vec<TerminalRef>,
        accepted: bool,
        fail_send: bool,
        delivered: bool,
        subscribe_result: Vec<String>,
        declared: Vec<String>,
        open_tab_result: Mutex<Option<Result<OpenTabResult, HostServiceError>>>,
        terminal_list_calls: Mutex<Vec<String>>,
        send_calls: Mutex<Vec<(String, String, bool)>>,
        notification_calls: Mutex<Vec<(String, String, Option<String>)>>,
        open_tab_calls: Mutex<Vec<OpenTabCall>>,
        session_post_calls: Mutex<Vec<(String, Option<String>, Value)>>,
        invoke_result: Mutex<Option<Result<Value, HostServiceError>>>,
        invoke_calls: Mutex<Vec<(String, Option<Value>, Option<String>)>>,
        storage: FakeStorage,
        settings: FakeSettings,
        secrets: FakeSecrets,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct OpenTabCall {
        plugin_key: String,
        scope: PluginScope,
        repo_id: Option<String>,
        tab_id: String,
        title: Option<String>,
        params: Option<Value>,
        worktree_hint: Option<String>,
    }

    impl PluginHostServices for FakeServices {
        fn active_worktree_context(&self) -> Option<WorktreeContext> {
            self.context.clone()
        }

        fn list_worktree_terminals(&self, worktree_id: &str) -> Vec<TerminalRef> {
            self.terminal_list_calls
                .lock()
                .unwrap()
                .push(worktree_id.to_string());
            self.terminals.clone()
        }

        fn send_terminal_text(
            &self,
            terminal_id: &str,
            text: &str,
            enter: bool,
        ) -> Result<bool, HostServiceError> {
            self.send_calls.lock().unwrap().push((
                terminal_id.to_string(),
                text.to_string(),
                enter,
            ));
            if self.fail_send {
                return Err(HostServiceError::new(ACTION_FAILED, "send failed"));
            }
            Ok(self.accepted)
        }

        fn dispatch_notification(
            &self,
            plugin_key: &str,
            title: &str,
            body: Option<&str>,
        ) -> Result<bool, HostServiceError> {
            self.notification_calls.lock().unwrap().push((
                plugin_key.to_string(),
                title.to_string(),
                body.map(str::to_string),
            ));
            Ok(self.delivered)
        }

        fn storage(&self) -> &dyn PluginStorageAccess {
            &self.storage
        }

        fn settings(&self) -> &dyn PluginSettingsAccess {
            &self.settings
        }

        fn secrets(&self) -> &dyn SecretStore {
            &self.secrets
        }

        fn subscribe_events(&self, _plugin_key: &str, events: &[String]) -> Vec<String> {
            if self.subscribe_result.is_empty() {
                events.to_vec()
            } else {
                self.subscribe_result.clone()
            }
        }

        fn open_tab(
            &self,
            plugin_key: &str,
            scope: PluginScope,
            repo_id: Option<&str>,
            tab_id: &str,
            title: Option<&str>,
            params: Option<Value>,
            worktree_hint: Option<&str>,
        ) -> Result<OpenTabResult, HostServiceError> {
            self.open_tab_calls.lock().unwrap().push(OpenTabCall {
                plugin_key: plugin_key.to_string(),
                scope,
                repo_id: repo_id.map(str::to_string),
                tab_id: tab_id.to_string(),
                title: title.map(str::to_string),
                params,
                worktree_hint: worktree_hint.map(str::to_string),
            });
            if let Some(result) = self.open_tab_result.lock().unwrap().clone() {
                return result;
            }
            Ok(OpenTabResult {
                instance_id: "inst-1".to_string(),
                worktree_id: worktree_hint.unwrap_or("wt-1").to_string(),
            })
        }

        fn declared_tabs(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
        ) -> Vec<String> {
            self.declared.clone()
        }

        fn session_post(
            &self,
            _plugin_key: &str,
            session_token: &str,
            topic: Option<&str>,
            payload: Value,
        ) -> Result<(), HostServiceError> {
            self.session_post_calls.lock().unwrap().push((
                session_token.to_string(),
                topic.map(str::to_string),
                payload,
            ));
            Ok(())
        }

        fn invoke_worker_command(
            &self,
            _plugin_key: &str,
            _scope: PluginScope,
            _repo_id: Option<&str>,
            command_id: &str,
            args: Option<Value>,
            session_token: Option<&str>,
        ) -> Result<Value, HostServiceError> {
            self.invoke_calls.lock().unwrap().push((
                command_id.to_string(),
                args,
                session_token.map(str::to_string),
            ));
            if let Some(result) = self.invoke_result.lock().unwrap().clone() {
                return result;
            }
            Ok(json!({ "started": true }))
        }
    }

    #[derive(Default)]
    struct RecordingAudit {
        records: Mutex<Vec<AuditRecord>>,
        fail_attempts: Mutex<bool>,
    }

    impl RecordingAudit {
        fn outcomes(&self) -> Vec<AuditOutcome> {
            self.records
                .lock()
                .unwrap()
                .iter()
                .map(|record| record.outcome)
                .collect()
        }
    }

    impl PluginAuditSink for RecordingAudit {
        fn record(&self, record: AuditRecord) -> Result<(), PluginError> {
            if *self.fail_attempts.lock().unwrap() && record.outcome == AuditOutcome::Attempt {
                return Err(PluginError::new(ACTION_FAILED, "audit disk is broken"));
            }
            self.records.lock().unwrap().push(record);
            Ok(())
        }
    }

    fn services_with_context() -> FakeServices {
        FakeServices {
            context: Some(WorktreeContext {
                worktree_id: "wt-1".to_string(),
                branch: "main".to_string(),
                display_name: "Demo".to_string(),
            }),
            accepted: true,
            delivered: true,
            declared: vec!["settings".to_string(), "dashboard".to_string()],
            ..FakeServices::default()
        }
    }

    fn context<'a>(
        services: &'a dyn PluginHostServices,
        audit: Option<&'a dyn PluginAuditSink>,
    ) -> HostCallContext<'a> {
        HostCallContext {
            plugin_key: KEY,
            granted: Some(&ALL_CAPABILITIES),
            via_panel: false,
            session_token: None,
            scope: PluginScope::Global,
            repo_id: None,
            worktree_hint: None,
            services,
            audit,
        }
    }

    fn expect_ok(outcome: Outcome) -> Value {
        match outcome {
            Outcome::Ok(value) => value,
            Outcome::Err { code, error } => panic!("expected ok, got {code}: {error}"),
        }
    }

    fn expect_err(outcome: Outcome) -> (String, String) {
        match outcome {
            Outcome::Err { code, error } => (code, error),
            Outcome::Ok(value) => panic!("expected error, got {value}"),
        }
    }

    #[test]
    fn gate_is_enforced_before_specs_and_services() {
        let services = services_with_context();
        let audit = RecordingAudit::default();

        let (code, _) = expect_err(execute(
            "workspace.explode",
            json!({}),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, gate::UNKNOWN_METHOD);

        let mut panel_ctx = context(&services, Some(&audit));
        panel_ctx.via_panel = true;
        let (code, _) = expect_err(execute("storage.get", json!({ "key": "k" }), &panel_ctx));
        assert_eq!(code, gate::PANEL_FORBIDDEN);

        let mut consentless = context(&services, Some(&audit));
        consentless.granted = None;
        let (code, _) = expect_err(execute("workspace.readContext", json!({}), &consentless));
        assert_eq!(code, gate::CONSENT_REQUIRED);

        let narrow = [Capability::Storage];
        let mut denied = context(&services, Some(&audit));
        denied.granted = Some(&narrow);
        let (code, _) = expect_err(execute("workspace.readContext", json!({}), &denied));
        assert_eq!(code, gate::CAPABILITY_DENIED);

        assert!(audit.records.lock().unwrap().is_empty());
        assert!(services.send_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn invalid_params_report_the_field_path() {
        let services = services_with_context();
        let (code, error) = expect_err(execute(
            "terminal.sendText",
            json!({ "terminalId": "t1" }),
            &context(&services, None),
        ));
        assert_eq!(code, INVALID_PARAMS);
        assert!(error.starts_with("text: "), "{error}");
    }

    #[test]
    fn read_context_projects_labels_and_whitelists_terminals() {
        let long_id = "x".repeat(api::TERMINAL_ID_MAX_CHARS + 1);
        let mut services = services_with_context();
        services.context = Some(WorktreeContext {
            worktree_id: "wt-1".to_string(),
            branch: "b".repeat(api::WORKSPACE_LABEL_MAX_CHARS + 20),
            display_name: "Demo".to_string(),
        });
        services.terminals = vec![TerminalRef { id: String::new() }];
        services.terminals.push(TerminalRef {
            id: long_id.clone(),
        });
        for index in 0..api::WORKSPACE_TERMINAL_LIMIT + 5 {
            services.terminals.push(TerminalRef {
                id: format!("t{index}"),
            });
        }

        let value = expect_ok(execute(
            "workspace.readContext",
            json!({}),
            &context(&services, None),
        ));
        assert_eq!(
            value.get("branch").and_then(Value::as_str).unwrap().len(),
            api::WORKSPACE_LABEL_MAX_CHARS
        );
        assert_eq!(value.get("displayName"), Some(&json!("Demo")));
        let terminals = value.get("terminals").and_then(Value::as_array).unwrap();
        assert_eq!(terminals.len(), api::WORKSPACE_TERMINAL_LIMIT);
        for terminal in terminals {
            let id = terminal.get("id").and_then(Value::as_str).unwrap();
            assert!(!id.is_empty() && id != long_id);
        }
        assert_eq!(
            services.terminal_list_calls.lock().unwrap().as_slice(),
            ["wt-1"]
        );
    }

    #[test]
    fn read_context_is_null_without_a_worktree() {
        let mut services = services_with_context();
        services.context = None;
        let value = expect_ok(execute(
            "workspace.readContext",
            json!({}),
            &context(&services, None),
        ));
        assert_eq!(value, Value::Null);
    }

    #[test]
    fn worktree_hint_overrides_the_active_context_for_scoping() {
        let mut services = services_with_context();
        let audit = RecordingAudit::default();
        services.context = Some(WorktreeContext {
            worktree_id: "wt-1".to_string(),
            branch: "main".to_string(),
            display_name: "Demo".to_string(),
        });
        services.terminals = vec![TerminalRef {
            id: "hinted".to_string(),
        }];

        let mut hinted = context(&services, Some(&audit));
        hinted.worktree_hint = Some("wt-2");
        let value = expect_ok(execute("workspace.readContext", json!({}), &hinted));
        assert_eq!(value.get("branch"), Some(&json!("")));
        assert_eq!(
            services.terminal_list_calls.lock().unwrap().as_slice(),
            ["wt-2"]
        );

        let value = expect_ok(execute(
            "terminal.sendText",
            json!({ "terminalId": "hinted", "text": "hi" }),
            &hinted,
        ));
        assert_eq!(value, json!({ "accepted": true }));
        assert_eq!(
            services.send_calls.lock().unwrap().as_slice(),
            [("hinted".to_string(), "hi".to_string(), false)]
        );
    }

    #[test]
    fn send_text_rechecks_the_terminal_whitelist() {
        let mut services = services_with_context();
        let audit = RecordingAudit::default();
        services.terminals = vec![TerminalRef {
            id: "t1".to_string(),
        }];

        let (code, error) = expect_err(execute(
            "terminal.sendText",
            json!({ "terminalId": "elsewhere", "text": "hi" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, ACTION_FAILED);
        assert_eq!(error, "terminal is outside the active worktree");
        assert!(services.send_calls.lock().unwrap().is_empty());

        services.context = None;
        let (code, error) = expect_err(execute(
            "terminal.sendText",
            json!({ "terminalId": "t1", "text": "hi", "enter": true }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, ACTION_FAILED);
        assert_eq!(error, "no active worktree is available for terminal input");
    }

    #[test]
    fn open_tab_requires_a_declared_tab_and_delegates_with_the_resolved_worktree() {
        let services = services_with_context();
        let audit = RecordingAudit::default();

        let value = expect_ok(execute(
            "workspace.openTab",
            json!({ "tabId": "settings", "title": "Settings", "params": { "section": "a" } }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(
            value,
            json!({ "instanceId": "inst-1", "worktreeId": "wt-1" })
        );
        let calls = services.open_tab_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].plugin_key, KEY);
        assert_eq!(calls[0].scope, PluginScope::Global);
        assert_eq!(calls[0].repo_id, None);
        assert_eq!(calls[0].tab_id, "settings");
        assert_eq!(calls[0].title.as_deref(), Some("Settings"));
        assert_eq!(calls[0].params, Some(json!({ "section": "a" })));
        assert_eq!(calls[0].worktree_hint.as_deref(), Some("wt-1"));
        drop(calls);

        let (code, error) = expect_err(execute(
            "workspace.openTab",
            json!({ "tabId": "missing" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, INVALID_PARAMS);
        assert!(error.contains("missing"), "{error}");
        assert_eq!(services.open_tab_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn open_tab_propagates_service_errors_and_fails_without_a_worktree() {
        let mut services = services_with_context();
        let audit = RecordingAudit::default();
        *services.open_tab_result.lock().unwrap() = Some(Err(HostServiceError::new(
            gate::RATE_LIMITED,
            "too many tabs",
        )));
        let (code, error) = expect_err(execute(
            "workspace.openTab",
            json!({ "tabId": "settings" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, "rate_limited");
        assert_eq!(error, "too many tabs");

        services.context = None;
        *services.open_tab_result.lock().unwrap() = None;
        let (code, _) = expect_err(execute(
            "workspace.openTab",
            json!({ "tabId": "settings" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, ACTION_FAILED);
    }

    #[test]
    fn mutations_audit_attempt_then_ok_and_non_mutations_do_not_audit() {
        let services = services_with_context();
        let audit = RecordingAudit::default();

        let value = expect_ok(execute(
            "storage.set",
            json!({ "key": "theme", "value": "dark" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(value, json!({ "ok": true }));
        assert_eq!(audit.outcomes(), [AuditOutcome::Attempt, AuditOutcome::Ok]);
        let records = audit.records.lock().unwrap();
        assert_eq!(records[0].actor, format!("{AUDIT_ACTOR_PREFIX}{KEY}"));
        assert_eq!(records[0].method, "storage.set");
        assert_eq!(records[0].summary, "key=theme");
        drop(records);

        expect_ok(execute(
            "workspace.readContext",
            json!({}),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(audit.outcomes().len(), 2);
    }

    #[test]
    fn handler_failures_audit_error_and_result_mismatches_fail_closed() {
        let services = services_with_context();
        let audit = RecordingAudit::default();
        *services.storage.broken.lock().unwrap() = true;
        let (code, error) = expect_err(execute(
            "storage.set",
            json!({ "key": "theme", "value": "dark" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, ACTION_FAILED);
        assert_eq!(error, "storage is broken");
        assert_eq!(
            audit.outcomes(),
            [AuditOutcome::Attempt, AuditOutcome::Error]
        );

        let services = services_with_context();
        let audit = RecordingAudit::default();
        *services.open_tab_result.lock().unwrap() = Some(Ok(OpenTabResult {
            instance_id: String::new(),
            worktree_id: "wt-1".to_string(),
        }));
        let (code, error) = expect_err(execute(
            "workspace.openTab",
            json!({ "tabId": "settings" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, ACTION_FAILED);
        assert_eq!(error, "internal: malformed workspace.openTab result");
        assert_eq!(
            audit.outcomes(),
            [AuditOutcome::Attempt, AuditOutcome::Error]
        );
    }

    #[test]
    fn mutations_require_a_working_audit_sink() {
        let services = services_with_context();
        let (code, error) = expect_err(execute(
            "storage.set",
            json!({ "key": "theme", "value": "dark" }),
            &context(&services, None),
        ));
        assert_eq!(code, UNAVAILABLE);
        assert_eq!(error, "mutation audit log is not available");
        assert!(services.storage.map.lock().unwrap().is_empty());

        let audit = RecordingAudit::default();
        *audit.fail_attempts.lock().unwrap() = true;
        let (code, error) = expect_err(execute(
            "storage.set",
            json!({ "key": "theme", "value": "dark" }),
            &context(&services, Some(&audit)),
        ));
        assert_eq!(code, ACTION_FAILED);
        assert_eq!(error, "mutation audit log could not be written");
        assert!(services.storage.map.lock().unwrap().is_empty());
    }

    #[test]
    fn storage_secrets_settings_notifications_and_events_round_trip() {
        let services = services_with_context();
        let audit = RecordingAudit::default();
        let none = context(&services, Some(&audit));

        expect_ok(execute(
            "notifications.show",
            json!({ "title": "Done", "body": "ok" }),
            &none,
        ));
        assert_eq!(
            services.notification_calls.lock().unwrap().as_slice(),
            [(KEY.to_string(), "Done".to_string(), Some("ok".to_string()))]
        );

        let value = expect_ok(execute("secrets.get", json!({ "key": "token" }), &none));
        assert_eq!(value, json!({ "value": null }));
        expect_ok(execute(
            "secrets.set",
            json!({ "key": "token", "value": "s3cret" }),
            &none,
        ));
        let value = expect_ok(execute("secrets.get", json!({ "key": "token" }), &none));
        assert_eq!(value, json!({ "value": "s3cret" }));

        let value = expect_ok(execute("storage.keys", json!({}), &none));
        assert_eq!(value, json!({ "keys": [] }));
        expect_ok(execute(
            "storage.delete",
            json!({ "key": "missing" }),
            &none,
        ));

        expect_ok(execute(
            "settings.set",
            json!({ "key": "editor", "value": { "fontSize": 13 } }),
            &none,
        ));
        let value = expect_ok(execute("settings.get", json!({}), &none));
        assert_eq!(
            value,
            json!({ "settings": { "editor": { "fontSize": 13 } } })
        );

        let value = expect_ok(execute(
            "events.subscribe",
            json!({ "events": ["worktree.created", "agent.status.changed"] }),
            &none,
        ));
        assert_eq!(
            value,
            json!({ "subscribed": ["worktree.created", "agent.status.changed"] })
        );
    }

    #[test]
    fn malformed_handler_results_fail_closed() {
        let mut services = services_with_context();
        services.subscribe_result = vec!["net.fetch".to_string()];
        let (code, error) = expect_err(execute(
            "events.subscribe",
            json!({ "events": ["worktree.created"] }),
            &context(&services, None),
        ));
        assert_eq!(code, ACTION_FAILED);
        assert_eq!(error, "internal: malformed events.subscribe result");
    }

    #[test]
    fn session_post_reaches_services_only_from_workers() {
        let services = services_with_context();
        let value = expect_ok(execute(
            "session.post",
            json!({ "sessionToken": "t".repeat(32), "topic": "log", "payload": { "line": "x" } }),
            &context(&services, None),
        ));
        assert_eq!(value, json!({ "ok": true }));
        assert_eq!(
            services.session_post_calls.lock().unwrap().as_slice(),
            [("t".repeat(32), Some("log".to_string()), json!({ "line": "x" }))]
        );

        let session_token = "t".repeat(32);
        let mut panel_ctx = context(&services, None);
        panel_ctx.via_panel = true;
        panel_ctx.session_token = Some(session_token.as_str());
        let (code, _) = expect_err(execute(
            "session.post",
            json!({ "sessionToken": "t".repeat(32), "payload": 1 }),
            &panel_ctx,
        ));
        assert_eq!(code, gate::PANEL_FORBIDDEN);
    }

    #[test]
    fn worker_invoke_command_requires_a_panel_session_and_sanitizes_errors() {
        let services = services_with_context();
        // 面板会话上下文成功路径
        let token_owned = "t".repeat(32);
        let mut panel = context(&services, None);
        panel.via_panel = true;
        panel.session_token = Some(token_owned.as_str());
        *services.invoke_result.lock().unwrap() = Some(Ok(json!({ "started": true })));
        let value = expect_ok(execute(
            "worker.invokeCommand",
            json!({ "commandId": "check" }),
            &panel,
        ));
        assert_eq!(value, json!({ "started": true }));
        assert_eq!(services.invoke_calls.lock().unwrap().len(), 1);

        // worker 自调（无会话上下文）→ unavailable
        let (code, _) = expect_err(execute(
            "worker.invokeCommand",
            json!({ "commandId": "check" }),
            &context(&services, None),
        ));
        assert_eq!(code, UNAVAILABLE);

        // 未声明命令 → 面板层 invalid_params
        *services.invoke_result.lock().unwrap() =
            Some(Err(HostServiceError::new("unknown_command", "plugin does not declare command x")));
        let (code, error) = expect_err(execute(
            "worker.invokeCommand",
            json!({ "commandId": "missing" }),
            &panel,
        ));
        assert_eq!(code, INVALID_PARAMS);
        assert!(error.contains("does not declare"), "{error}");

        // 超限结果 → invalid_params
        *services.invoke_result.lock().unwrap() = Some(Ok(json!({ "blob": "x".repeat(64 * 1024 + 1) })));
        let (code, _) = expect_err(execute(
            "worker.invokeCommand",
            json!({ "commandId": "check" }),
            &panel,
        ));
        assert_eq!(code, INVALID_PARAMS);

        // 面板桥外（palette/worker）无 session → unavailable（同 worker 自调分支）
        let mut no_session = context(&services, None);
        no_session.via_panel = true;
        let (code, _) = expect_err(execute(
            "worker.invokeCommand",
            json!({ "commandId": "check" }),
            &no_session,
        ));
        assert_eq!(code, UNAVAILABLE);
    }
}
