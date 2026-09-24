use serde::Deserialize;
use tauri::State;

use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::AppState;

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
