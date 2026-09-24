use std::sync::Mutex;

use ade_store::ui_state_store::UiStateStore;
use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::{lock, AppState, WriteScheduler};

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UiRecordFeatureInteractionArgs {
    pub id: String,
}

/// Read the full effective UI state (`defaults ∪ stored`).
#[tauri::command]
#[specta::specta]
pub async fn ui_get(state: State<'_, AppState>) -> Result<Json, BridgeError> {
    Ok(Json::new(state.ui_store().get()))
}

/// Merge a renderer partial into the in-memory snapshot, debounce the disk
/// write, and broadcast the complete object to every window.
#[tauri::command]
#[specta::specta]
pub async fn ui_set(state: State<'_, AppState>, args: Json) -> Result<(), BridgeError> {
    let next = state.ui_store().merge(args.into_inner())?;
    state.schedule_ui_write();
    events::emit_json(&state.app, events::UI_STATE_CHANGED, next);
    Ok(())
}

/// Record one feature interaction (`interactionCount + 1`, earliest timestamp
/// kept) and return the complete object.
#[tauri::command]
#[specta::specta]
pub async fn ui_record_feature_interaction(
    state: State<'_, AppState>,
    args: UiRecordFeatureInteractionArgs,
) -> Result<Json, BridgeError> {
    let next = state
        .ui_store()
        .record_feature_interaction_in_memory(&args.id)?;
    state.schedule_ui_write();
    events::emit_json(&state.app, events::UI_STATE_CHANGED, next.clone());
    Ok(Json::new(next))
}

/// `ui_set_with_ack` core: merge the partial into the in-memory snapshot, then
/// force the debounced write and surface a persist failure. Free of Tauri types
/// so the failure contract is testable without a runtime.
pub(crate) fn merge_ui_state_and_flush(
    store: &Mutex<UiStateStore>,
    writer: &WriteScheduler,
    partial: Value,
) -> Result<Value, BridgeError> {
    let next = lock(store).merge(partial)?;
    writer.schedule();
    writer.flush().map_err(BridgeError::message)?;
    Ok(next)
}

/// `setWithAck`: same in-memory merge as `ui_set`, but the write is forced
/// before returning and a persist failure rejects instead of being logged
/// (spec §5.4).
#[tauri::command]
#[specta::specta]
pub async fn ui_set_with_ack(state: State<'_, AppState>, args: Json) -> Result<(), BridgeError> {
    let next = state.merge_ui_state_with_ack(args.into_inner())?;
    events::emit_json(&state.app, events::UI_STATE_CHANGED, next);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ade_core::defaults::ui_state_defaults;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    fn store_in(path: std::path::PathBuf) -> Arc<Mutex<UiStateStore>> {
        Arc::new(Mutex::new(UiStateStore::load(path, ui_state_defaults())))
    }

    fn scheduler_for(store: &Arc<Mutex<UiStateStore>>) -> WriteScheduler {
        let store = Arc::clone(store);
        WriteScheduler::new(
            Duration::from_millis(10),
            Duration::from_millis(10),
            move || lock(&store).persist(),
        )
    }

    #[test]
    fn set_with_ack_persists_before_returning() {
        let dir = tempfile::tempdir().unwrap();
        let ui_path = dir.path().join("ui-state.json");
        let store = store_in(ui_path.clone());
        let writer = scheduler_for(&store);

        let next = merge_ui_state_and_flush(&store, &writer, json!({ "sidebarWidth": 321 }))
            .expect("persist succeeds");

        assert_eq!(next["sidebarWidth"], 321);
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&ui_path).unwrap()).unwrap();
        assert_eq!(on_disk["sidebarWidth"], 321);
    }

    #[test]
    fn set_with_ack_rejects_when_persist_fails_and_keeps_the_in_memory_merge() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the store's parent directory should be makes every save
        // fail (`create_dir_all` hits ENOTDIR).
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, "").unwrap();
        let store = store_in(blocked.join("ui-state.json"));
        let writer = scheduler_for(&store);

        let error = merge_ui_state_and_flush(&store, &writer, json!({ "sidebarWidth": 321 }))
            .expect_err("a failed persist must reject");

        assert!(!error.to_string().is_empty());
        // Why: `ui_get` reads this same in-memory store, so the merge that
        // succeeded before the failed write must still be visible there.
        assert_eq!(lock(&store).get()["sidebarWidth"], 321);
    }
}
