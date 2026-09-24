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
        events::emit_json(&state.app, events::SETTINGS_CHANGED, changed);
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
}
