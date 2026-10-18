use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};

use ade_plugins::service::PanelPushRequest;
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
/// 插件/内容包集合可能已变（payload `PluginsChangedPayload`，规格 §3.9）。
pub const PLUGINS_CHANGED: &str = "plugins:changed";
/// 插件请求打开一个主区 tab（payload `PluginOpenTabPayload`，规格 §3.6/§3.9）。
/// 去重/限额已在 Rust 侧判定；渲染层按 `instanceId` 建立或聚焦 tab。
pub const PLUGINS_OPEN_TAB: &str = "plugins:open-tab";
/// 插件推送一条消息到其 UI 会话（payload `PluginPanelPushPayload`，规格 §3.5）。
pub const PLUGINS_PANEL_PUSH: &str = "plugins:panel-push";

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

/// Payload for [`PLUGINS_CHANGED`] (`{contentPacksChanged, pluginsChanged}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsChangedPayload {
    pub content_packs_changed: bool,
    pub plugins_changed: bool,
}

/// Payload for [`PLUGINS_OPEN_TAB`]（TS `PluginOpenTabRequest` 同形；`scope`
/// 为 `'global' | 'project'` 字符串，渲染层据此定位 tab）。
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginOpenTabPayload {
    pub plugin_key: String,
    pub tab_id: String,
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub params: Json,
    pub worktree_id: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

impl From<ade_plugins::service::OpenTabRequest> for PluginOpenTabPayload {
    fn from(request: ade_plugins::service::OpenTabRequest) -> Self {
        Self {
            plugin_key: request.plugin_key,
            tab_id: request.tab_id,
            instance_id: request.instance_id,
            title: request.title,
            params: Json::new(request.params),
            worktree_id: request.worktree_id,
            scope: request.scope.as_str().to_string(),
            repo_id: request.repo_id,
        }
    }
}

/// Payload of [`PLUGINS_PANEL_PUSH`]（TS `PluginPanelPushRequest` 同形）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelPushPayload {
    pub session_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub payload: Value,
}

impl From<PanelPushRequest> for PluginPanelPushPayload {
    fn from(request: PanelPushRequest) -> Self {
        Self {
            session_token: request.session_token,
            topic: request.topic,
            payload: request.payload,
        }
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
    fn plugin_event_names_and_payload_shapes_are_locked() {
        assert_eq!(PLUGINS_CHANGED, "plugins:changed");
        assert_eq!(PLUGINS_OPEN_TAB, "plugins:open-tab");
        assert_eq!(
            serde_json::to_value(PluginsChangedPayload {
                content_packs_changed: false,
                plugins_changed: true,
            })
            .unwrap(),
            json!({ "contentPacksChanged": false, "pluginsChanged": true })
        );
        let payload = PluginOpenTabPayload::from(ade_plugins::service::OpenTabRequest {
            plugin_key: "orca-samples.demo".to_string(),
            tab_id: "board".to_string(),
            instance_id: "inst-1".to_string(),
            title: Some("Board".to_string()),
            params: json!({ "filter": "open" }),
            worktree_id: "r1::/wt".to_string(),
            scope: ade_plugins::types::PluginScope::Project,
            repo_id: Some("r1".to_string()),
        });
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            json!({
                "pluginKey": "orca-samples.demo",
                "tabId": "board",
                "instanceId": "inst-1",
                "title": "Board",
                "params": { "filter": "open" },
                "worktreeId": "r1::/wt",
                "scope": "project",
                "repoId": "r1"
            })
        );
        let bare = PluginOpenTabPayload::from(ade_plugins::service::OpenTabRequest {
            plugin_key: "orca-samples.demo".to_string(),
            tab_id: "board".to_string(),
            instance_id: "inst-2".to_string(),
            title: None,
            params: serde_json::Value::Null,
            worktree_id: "wt-1".to_string(),
            scope: ade_plugins::types::PluginScope::Global,
            repo_id: None,
        });
        let value = serde_json::to_value(&bare).unwrap();
        assert_eq!(value["scope"], "global");
        assert!(value.get("title").is_none());
        assert!(value.get("repoId").is_none());

        assert_eq!(PLUGINS_PANEL_PUSH, "plugins:panel-push");
        let push = PluginPanelPushPayload::from(PanelPushRequest {
            session_token: "t".repeat(32),
            topic: Some("state".to_string()),
            payload: json!({ "n": 1 }),
        });
        assert_eq!(
            serde_json::to_value(&push).unwrap(),
            json!({ "sessionToken": "t".repeat(32), "topic": "state", "payload": { "n": 1 } })
        );
        let bare_push = PluginPanelPushPayload::from(PanelPushRequest {
            session_token: "t".repeat(32),
            topic: None,
            payload: Value::Null,
        });
        let bare_value = serde_json::to_value(&bare_push).unwrap();
        assert!(bare_value.get("topic").is_none());
        assert_eq!(bare_value["payload"], Value::Null);
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
