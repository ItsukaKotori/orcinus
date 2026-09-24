use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// Out-of-band settings updates (View > Appearance toggles etc.). Payload is a
/// partial `GlobalSettings` containing only the changed top-level keys.
pub const SETTINGS_CHANGED: &str = "settings:changed";
/// Full `PersistedUIState` broadcast after every accepted `ui_set`.
pub const UI_STATE_CHANGED: &str = "ui:stateChanged";
/// Batched filesystem changes for a watched worktree root (`FsChangedPayload`).
pub const FS_CHANGED: &str = "fs:changed";
/// Every repos/projectGroups/folderWorkspaces registry mutation; empty payload.
pub const REPOS_CHANGED: &str = "repos:changed";
/// The repo whose worktree set may have changed (`WorktreeChangedPayload`).
pub const WORKTREES_CHANGED: &str = "worktrees:changed";

/// Payload for [`WORKTREES_CHANGED`] (spec §5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChangedPayload {
    pub repo_id: String,
}

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

/// Broadcast an empty `repos:changed` for one registry mutation.
pub fn emit_repos_changed<R: Runtime>(app: &AppHandle<R>) {
    emit_json(app, REPOS_CHANGED, ());
}

/// Broadcast `worktrees:changed {repoId}` for one repo.
pub fn emit_worktrees_changed<R: Runtime>(app: &AppHandle<R>, repo_id: &str) {
    emit_json(
        app,
        WORKTREES_CHANGED,
        WorktreeChangedPayload {
            repo_id: repo_id.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn worktree_changed_payload_serializes_camel_case() {
        let payload = WorktreeChangedPayload {
            repo_id: "r1".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            json!({ "repoId": "r1" })
        );
    }
}
