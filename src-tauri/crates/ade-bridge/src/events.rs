use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// Out-of-band settings updates (View > Appearance toggles etc.). Payload is a
/// partial `GlobalSettings` containing only the changed top-level keys.
pub const SETTINGS_CHANGED: &str = "settings:changed";
/// Full `PersistedUIState` broadcast after every accepted `ui_set`.
pub const UI_STATE_CHANGED: &str = "ui:stateChanged";
/// Batched filesystem changes for a watched worktree root (`FsChangedPayload`).
pub const FS_CHANGED: &str = "fs:changed";

/// Emit a JSON event without letting a delivery failure abort the command that
/// produced it (spec §6: event payload errors are logged and dropped).
pub fn emit_json<R: Runtime, T>(app: &AppHandle<R>, event: &str, payload: T)
where
    T: Serialize + Clone,
{
    if let Err(error) = app.emit(event, payload) {
        eprintln!("[ade-bridge] failed to emit '{event}': {error}");
    }
}
