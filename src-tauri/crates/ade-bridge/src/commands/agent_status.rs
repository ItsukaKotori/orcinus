use serde::Serialize;
use tauri::State;

use crate::errors::BridgeError;
use crate::json::Json;
use crate::state::AppState;

/// `agent_status_get_snapshot` 元素：与 `AgentHookRawPayload` 同形，
/// `restored` 显式携带（hydrate 回放标记，规格 §3.6/§3.7）。
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookSnapshotEntry {
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
    pub restored: bool,
}

impl From<ade_hooks::CachedHookEvent> for AgentHookSnapshotEntry {
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

/// 重启后 renderer hydration 的原始缓存快照（升序；归一化在 renderer）。
#[tauri::command]
#[specta::specta]
pub async fn agent_status_get_snapshot(
    state: State<'_, AppState>,
) -> Result<Vec<AgentHookSnapshotEntry>, BridgeError> {
    Ok(state
        .hooks
        .snapshot()
        .into_iter()
        .map(AgentHookSnapshotEntry::from)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_entry_serializes_camel_case_with_restored_flag() {
        let entry = AgentHookSnapshotEntry::from(ade_hooks::CachedHookEvent {
            source: "claude".to_string(),
            payload: serde_json::json!({ "hook_event_name": "Stop" }),
            pane_key: "t1:leaf".to_string(),
            tab_id: None,
            worktree_id: None,
            launch_token: None,
            received_at: 7,
            restored: true,
        });
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({
                "source": "claude",
                "payload": { "hook_event_name": "Stop" },
                "paneKey": "t1:leaf",
                "receivedAt": 7,
                "restored": true
            })
        );
    }
}
