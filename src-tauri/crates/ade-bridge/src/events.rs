use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::json::Json;

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
/// PtyHost spawn 成功（`ade_pty::PtyEvent::Spawned` 转发，载荷 `{id}`）。
pub const PTY_SPAWNED: &str = "pty:spawned";
/// 会话退出（`ade_pty::PtyEvent::Exit` 转发；载荷 `{id, code}`，WS close 之
/// 后发）。即时退出会话的 Exit 可能先于 spawn 返回到达（Task 9 交接）。
pub const PTY_EXIT: &str = "pty:exit";
/// Window-close quit → one renderer flush window (spec §3.4).
pub const SESSION_FLUSH_REQUESTED: &str = "session:flush-requested";
/// hook server 原始事件（归一化在 renderer，规格 §3.2）。
pub const AGENT_HOOK_RAW: &str = "agent-hook:raw";

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

/// Payload for [`PTY_SPAWNED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct PtySpawnedPayload {
    pub id: String,
}

/// Payload for [`PTY_EXIT`]（`ade_pty::ExitInfo` 的 bridge 侧同形——host crate
/// 不依赖 specta，投影在此定形）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyExitPayload {
    pub id: String,
    pub code: i32,
}

/// Forward one PtyHost event to the Tauri event bus (spec §2.1). Spawned 在
/// `PtyHost::spawn` 成功处发、Exit 在会话退出处发——命令层不再重复 emit。
/// Delivery failure is logged and dropped (`emit_json` 惯例)。
pub fn forward_pty_event<R: Runtime>(app: &AppHandle<R>, event: ade_pty::PtyEvent) {
    match event {
        ade_pty::PtyEvent::Spawned { id } => {
            emit_json(app, PTY_SPAWNED, PtySpawnedPayload { id });
        }
        ade_pty::PtyEvent::Exit(info) => emit_json(
            app,
            PTY_EXIT,
            PtyExitPayload {
                id: info.id,
                code: info.code,
            },
        ),
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

/// Payload for [`AGENT_HOOK_RAW`]（`CachedHookEvent` 的 bridge 侧同形；
/// `restored` false 时省略——实时事件不需要回放标记，规格 §3.2）。
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookRawPayload {
    pub source: String,
    pub payload: Json,
    pub pane_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_token: Option<String>,
    pub received_at: i64,
    #[serde(skip_serializing_if = "restored_is_false")]
    pub restored: bool,
}

fn restored_is_false(value: &bool) -> bool {
    !*value
}

impl From<ade_hooks::CachedHookEvent> for AgentHookRawPayload {
    fn from(event: ade_hooks::CachedHookEvent) -> Self {
        Self {
            source: event.source,
            payload: Json::new(event.payload),
            pane_key: event.pane_key,
            tab_id: event.tab_id,
            worktree_id: event.worktree_id,
            launch_token: event.launch_token,
            received_at: event.received_at,
            restored: event.restored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pty_event_payloads_serialize_verbatim_and_names_are_locked() {
        assert_eq!(
            serde_json::to_value(PtySpawnedPayload {
                id: "p1".to_string()
            })
            .unwrap(),
            json!({ "id": "p1" })
        );
        assert_eq!(
            serde_json::to_value(PtyExitPayload {
                id: "p1".to_string(),
                code: 0,
            })
            .unwrap(),
            json!({ "id": "p1", "code": 0 })
        );
        assert_eq!(PTY_SPAWNED, "pty:spawned");
        assert_eq!(PTY_EXIT, "pty:exit");
    }

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
    fn agent_hook_raw_payload_serializes_camel_case_and_omits_false_restored() {
        let payload = AgentHookRawPayload::from(ade_hooks::CachedHookEvent {
            source: "claude".to_string(),
            payload: json!({ "hook_event_name": "Stop" }),
            pane_key: "t1:leaf".to_string(),
            tab_id: Some("t1".to_string()),
            worktree_id: None,
            launch_token: None,
            received_at: 42,
            restored: false,
        });
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            json!({
                "source": "claude",
                "payload": { "hook_event_name": "Stop" },
                "paneKey": "t1:leaf",
                "tabId": "t1",
                "receivedAt": 42
            })
        );
        assert_eq!(AGENT_HOOK_RAW, "agent-hook:raw");
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
