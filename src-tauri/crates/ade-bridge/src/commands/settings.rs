use serde_json::{Map, Value};
use tauri::State;

use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::AppState;

/// Top-level keys whose value differs between two settings snapshots, with the
/// new value attached. Broadcast as the `settings:changed` payload so the
/// renderer merges exactly what changed (oracle semantics).
pub fn changed_settings(before: &Value, after: &Value) -> Value {
    let mut changed = Map::new();
    let Some(after_map) = after.as_object() else {
        return Value::Object(changed);
    };
    for (key, value) in after_map {
        if before.get(key) != Some(value) {
            changed.insert(key.clone(), value.clone());
        }
    }
    Value::Object(changed)
}

/// 从 `settings:changed` 载荷提取 hooks 开关（缺省不动作）。
pub fn hooks_toggle_from_changes(changed: &Value) -> Option<bool> {
    changed.get("agentStatusHooksEnabled").and_then(Value::as_bool)
}

/// Read the full effective settings object (`defaults ∪ stored`).
#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Json, BridgeError> {
    Ok(Json::new(state.settings_store().get()))
}

/// Merge a renderer partial into the in-memory snapshot and debounce the disk
/// write. Returns the complete merged object.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(state: State<'_, AppState>, args: Json) -> Result<Json, BridgeError> {
    let (before, next) = {
        let mut store = state.settings_store();
        let before = store.get();
        let next = store.merge_partial(args.into_inner())?;
        (before, next)
    };
    state.schedule_settings_write();

    let changed = changed_settings(&before, &next);
    if changed.as_object().is_some_and(|map| !map.is_empty()) {
        events::emit_json(&state.app, events::SETTINGS_CHANGED, changed.clone());
    }
    // 显式 toggle（规格 §3.3/§4 裁定）：开 = 安装/更新；关 = 移除托管条目。
    // 启动期关闭只 skip 不删；用户显式关闭才移除（oracle `applyAgentStatusHooksEnabled`）。
    if let Some(enabled) = hooks_toggle_from_changes(&changed) {
        state.hooks.set_hooks_enabled(enabled, &state.home);
    }
    Ok(Json::new(next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reports_only_changed_top_level_keys() {
        let before = json!({
            "theme": "system",
            "notifications": { "enabled": true, "terminalBell": false },
            "workspaceDir": "/tmp/work"
        });
        let after = json!({
            "theme": "dark",
            "notifications": { "enabled": true, "terminalBell": false },
            "workspaceDir": "/tmp/work"
        });
        assert_eq!(
            changed_settings(&before, &after),
            json!({ "theme": "dark" })
        );
    }

    #[test]
    fn reports_deep_merged_groups_as_one_changed_key() {
        let before = json!({ "notifications": { "enabled": true, "terminalBell": false } });
        let after = json!({ "notifications": { "enabled": false, "terminalBell": false } });
        assert_eq!(
            changed_settings(&before, &after),
            json!({ "notifications": { "enabled": false, "terminalBell": false } })
        );
    }

    #[test]
    fn reports_nothing_when_values_match() {
        let value = json!({ "theme": "system", "pluginConsents": {} });
        assert_eq!(changed_settings(&value, &value), json!({}));
    }

    #[test]
    fn extracts_agent_status_hooks_toggle_from_changed_keys() {
        assert_eq!(
            hooks_toggle_from_changes(&json!({ "agentStatusHooksEnabled": false })),
            Some(false)
        );
        assert_eq!(
            hooks_toggle_from_changes(&json!({ "theme": "dark" })),
            None
        );
    }
}
