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
/// Incremental `project_groups_scan_nested` progress
/// (`ScanNestedProgressPayload`).
pub const PROJECT_GROUPS_SCAN_NESTED_PROGRESS: &str = "project-groups:scan-nested-progress";

/// Payload for [`WORKTREES_CHANGED`] (spec §5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChangedPayload {
    pub repo_id: String,
}

/// Payload for [`PROJECT_GROUPS_SCAN_NESTED_PROGRESS`]: the task's
/// `{scanId, scanned, found}` fields plus the full snapshot, which the TS
/// contract's `onNestedScanProgress` callback consumes as `{scanId, scan}`.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanNestedProgressPayload {
    pub scan_id: String,
    pub scanned: u64,
    pub found: u64,
    pub scan: ade_core::models::project_group::NestedRepoScanResult,
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

    #[test]
    fn scan_nested_progress_payload_carries_scan_id_counts_and_snapshot() {
        let payload = ScanNestedProgressPayload {
            scan_id: "scan-1".to_string(),
            scanned: 4,
            found: 2,
            scan: ade_core::models::project_group::NestedRepoScanResult {
                selected_path: "/workspace".to_string(),
                selected_path_kind:
                    ade_core::models::project_group::NestedRepoSelectedPathKind::NonGitFolder,
                repos: vec![ade_core::models::project_group::NestedRepoCandidate {
                    path: "/workspace/api".to_string(),
                    display_name: "api".to_string(),
                    depth: 1,
                }],
                truncated: false,
                timed_out: false,
                stopped: false,
                duration_ms: 3,
                max_depth: 3,
                max_repos: 100,
                timeout_ms: None,
            },
        };
        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["scanId"], "scan-1");
        assert_eq!(value["scanned"], 4);
        assert_eq!(value["found"], 2);
        assert_eq!(value["scan"]["repos"][0]["path"], "/workspace/api");
        assert_eq!(
            PROJECT_GROUPS_SCAN_NESTED_PROGRESS,
            "project-groups:scan-nested-progress"
        );
    }
}
