//! Plugin event-source projection: the v0 event-name set (worktree lifecycle
//! plus agent status) and bounded payload projections matching
//! `orca:src/shared/plugins/plugin-events.ts`. Payloads are bounded
//! projections (`worktreeId` ≤2048 chars, `path` ≤32KiB chars, `branch`
//! ≤1024, `paneKey` ≤2048, `state` ≤256, `receivedAt` finite positive) so no
//! raw runtime object leaks through the event stream.

use serde::{Deserialize, Serialize};

/// v0 plugin event names, mirroring the oracle `PluginEventName` union.
pub const PLUGIN_EVENT_NAMES: [&str; 3] = [
    "worktree.created",
    "worktree.removed",
    "agent.status.changed",
];

const WORKTREE_ID_MAX_CHARS: usize = 2048;
const PATH_MAX_CHARS: usize = 32 * 1024;
const BRANCH_MAX_CHARS: usize = 1024;
const PANE_KEY_MAX_CHARS: usize = 2048;
const STATE_MAX_CHARS: usize = 256;

/// Payload of `worktree.created`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreatedPayload {
    pub worktree_id: String,
    pub path: String,
    pub branch: String,
}

/// Payload of `worktree.removed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRemovedPayload {
    pub worktree_id: String,
    pub path: String,
}

/// Payload of `agent.status.changed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusChangedPayload {
    pub worktree_id: Option<String>,
    pub pane_key: String,
    pub state: String,
    pub received_at: f64,
}

/// Whether `name` is one of the v0 plugin event names.
pub fn is_plugin_event_name(name: &str) -> bool {
    PLUGIN_EVENT_NAMES.contains(&name)
}

/// Project a raw event payload onto the bounded oracle schema for `name`.
///
/// Known names get a camelCase projection with oversized string fields
/// truncated to their oracle bounds; unknown names and invalid shapes
/// (missing keys, wrong types, empty strings where the oracle requires
/// `min(1)`, non-positive/non-finite `receivedAt`) return `None`.
pub fn project_payload(name: &str, raw: &serde_json::Value) -> Option<serde_json::Value> {
    let object = raw.as_object()?;
    let projected = match name {
        "worktree.created" => serde_json::to_value(WorktreeCreatedPayload {
            worktree_id: required_string(object, "worktreeId", WORKTREE_ID_MAX_CHARS)?,
            path: required_string(object, "path", PATH_MAX_CHARS)?,
            branch: bounded_string(object, "branch", BRANCH_MAX_CHARS)?,
        }),
        "worktree.removed" => serde_json::to_value(WorktreeRemovedPayload {
            worktree_id: required_string(object, "worktreeId", WORKTREE_ID_MAX_CHARS)?,
            path: required_string(object, "path", PATH_MAX_CHARS)?,
        }),
        "agent.status.changed" => {
            let worktree_id = match object.get("worktreeId")? {
                serde_json::Value::Null => None,
                value => Some(required_string_value(value, WORKTREE_ID_MAX_CHARS)?),
            };
            let received_at = object.get("receivedAt")?.as_f64()?;
            if !received_at.is_finite() || received_at <= 0.0 {
                return None;
            }
            serde_json::to_value(AgentStatusChangedPayload {
                worktree_id,
                pane_key: required_string(object, "paneKey", PANE_KEY_MAX_CHARS)?,
                state: required_string(object, "state", STATE_MAX_CHARS)?,
                received_at,
            })
        }
        _ => return None,
    };
    projected.ok()
}

fn required_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    max_chars: usize,
) -> Option<String> {
    required_string_value(object.get(key)?, max_chars)
}

fn required_string_value(value: &serde_json::Value, max_chars: usize) -> Option<String> {
    let value = value.as_str()?;
    if value.is_empty() {
        return None;
    }
    Some(truncate_chars(value, max_chars))
}

fn bounded_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    max_chars: usize,
) -> Option<String> {
    Some(truncate_chars(object.get(key)?.as_str()?, max_chars))
}

/// Truncate to at most `max_chars` characters, never splitting a UTF-8
/// character. Mirrors the oracle `.max()` bound for over-long fields.
fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn created(id: &str, path: &str, branch: &str) -> serde_json::Value {
        json!({ "worktreeId": id, "path": path, "branch": branch })
    }

    fn status(
        id: serde_json::Value,
        pane: &str,
        state: &str,
        received_at: impl Into<serde_json::Value>,
    ) -> serde_json::Value {
        json!({ "worktreeId": id, "paneKey": pane, "state": state, "receivedAt": received_at.into() })
    }

    #[test]
    fn event_names_match_oracle_contract() {
        assert_eq!(
            PLUGIN_EVENT_NAMES,
            [
                "worktree.created",
                "worktree.removed",
                "agent.status.changed"
            ]
        );
    }

    #[test]
    fn is_plugin_event_name_matches_closed_set() {
        for name in PLUGIN_EVENT_NAMES {
            assert!(is_plugin_event_name(name), "{name} must be recognized");
        }
        assert!(!is_plugin_event_name("worktree.updated"));
        assert!(!is_plugin_event_name(""));
        assert!(!is_plugin_event_name("WORKTREE.CREATED"));
        assert!(!is_plugin_event_name("agent.status"));
    }

    #[test]
    fn payload_structs_serialize_camel_case() {
        let created = WorktreeCreatedPayload {
            worktree_id: "w1".into(),
            path: "/tmp/wt".into(),
            branch: "main".into(),
        };
        assert_eq!(
            serde_json::to_value(&created).unwrap(),
            json!({ "worktreeId": "w1", "path": "/tmp/wt", "branch": "main" })
        );

        let removed = WorktreeRemovedPayload {
            worktree_id: "w1".into(),
            path: "/tmp/wt".into(),
        };
        assert_eq!(
            serde_json::to_value(&removed).unwrap(),
            json!({ "worktreeId": "w1", "path": "/tmp/wt" })
        );

        let status = AgentStatusChangedPayload {
            worktree_id: None,
            pane_key: "pane-1".into(),
            state: "working".into(),
            received_at: 123.5,
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({ "worktreeId": null, "paneKey": "pane-1", "state": "working", "receivedAt": 123.5 })
        );
    }

    #[test]
    fn project_created_accepts_valid_and_drops_extra_fields() {
        let projected = project_payload(
            "worktree.created",
            &json!({
                "worktreeId": "w1",
                "path": "/tmp/wt",
                "branch": "feature/x",
                "extra": "dropped",
                "remotes": ["git@github.com:o/r.git"]
            }),
        )
        .expect("valid payload projects");
        assert_eq!(
            projected,
            json!({ "worktreeId": "w1", "path": "/tmp/wt", "branch": "feature/x" })
        );
    }

    #[test]
    fn project_created_rejects_invalid_shapes() {
        assert_eq!(
            project_payload(
                "worktree.created",
                &json!({ "worktreeId": "w1", "path": "/tmp/wt" })
            ),
            None,
            "missing branch is invalid"
        );
        assert_eq!(
            project_payload("worktree.created", &created("", "/tmp/wt", "main")),
            None,
            "empty worktreeId is invalid"
        );
        assert_eq!(
            project_payload("worktree.created", &created("w1", "", "main")),
            None,
            "empty path is invalid"
        );
        assert_eq!(
            project_payload(
                "worktree.created",
                &json!({ "worktreeId": 7, "path": "/tmp/wt", "branch": "" })
            ),
            None,
            "non-string worktreeId is invalid"
        );
        assert_eq!(
            project_payload("worktree.created", &json!(["not", "an", "object"])),
            None
        );
        assert_eq!(project_payload("worktree.created", &json!(null)), None);
    }

    #[test]
    fn project_created_truncates_fields_to_oracle_bounds() {
        let projected = project_payload(
            "worktree.created",
            &created(&"i".repeat(3000), &"p".repeat(40_000), &"b".repeat(2000)),
        )
        .expect("oversized fields truncate instead of failing");
        assert_eq!(
            projected["worktreeId"].as_str().unwrap().chars().count(),
            2048
        );
        assert_eq!(
            projected["path"].as_str().unwrap().chars().count(),
            32 * 1024
        );
        assert_eq!(projected["branch"].as_str().unwrap().chars().count(), 1024);
    }

    #[test]
    fn project_removed_accepts_valid_and_rejects_invalid() {
        let projected = project_payload(
            "worktree.removed",
            &json!({ "worktreeId": "w1", "path": "/tmp/wt", "branch": "main" }),
        )
        .expect("valid payload projects");
        assert_eq!(projected, json!({ "worktreeId": "w1", "path": "/tmp/wt" }));

        assert_eq!(
            project_payload("worktree.removed", &json!({ "worktreeId": "w1" })),
            None,
            "missing path is invalid"
        );
        assert_eq!(
            project_payload(
                "worktree.removed",
                &json!({ "worktreeId": "", "path": "/tmp/wt" })
            ),
            None,
            "empty worktreeId is invalid"
        );
        assert_eq!(
            project_payload(
                "worktree.removed",
                &json!({ "worktreeId": "w1", "path": 3 })
            ),
            None,
            "non-string path is invalid"
        );
    }

    #[test]
    fn project_removed_truncates_fields() {
        let projected = project_payload(
            "worktree.removed",
            &json!({ "worktreeId": "i".repeat(3000), "path": "p".repeat(40_000) }),
        )
        .expect("oversized fields truncate");
        assert_eq!(
            projected["worktreeId"].as_str().unwrap().chars().count(),
            2048
        );
        assert_eq!(
            projected["path"].as_str().unwrap().chars().count(),
            32 * 1024
        );
    }

    #[test]
    fn project_agent_status_accepts_null_and_string_worktree_id() {
        let unbound = project_payload(
            "agent.status.changed",
            &status(serde_json::Value::Null, "pane-1", "working", 123.5),
        )
        .expect("null worktreeId is valid");
        assert_eq!(
            unbound,
            json!({ "worktreeId": null, "paneKey": "pane-1", "state": "working", "receivedAt": 123.5 })
        );

        let bound = project_payload(
            "agent.status.changed",
            &status(json!("w1"), "pane-1", "idle", 1.0),
        )
        .expect("string worktreeId is valid");
        assert_eq!(bound["worktreeId"], json!("w1"));
    }

    #[test]
    fn project_agent_status_rejects_invalid_shapes() {
        assert_eq!(
            project_payload(
                "agent.status.changed",
                &json!({ "paneKey": "p", "state": "s", "receivedAt": 1.0 })
            ),
            None,
            "missing worktreeId key is invalid"
        );
        assert_eq!(
            project_payload("agent.status.changed", &status(json!(""), "p", "s", 1.0)),
            None,
            "empty worktreeId string is invalid"
        );
        assert_eq!(
            project_payload("agent.status.changed", &status(json!("w1"), "", "s", 1.0)),
            None,
            "empty paneKey is invalid"
        );
        assert_eq!(
            project_payload("agent.status.changed", &status(json!("w1"), "p", "", 1.0)),
            None,
            "empty state is invalid"
        );
        assert_eq!(
            project_payload(
                "agent.status.changed",
                &status(json!("w1"), "p", "s", "now")
            ),
            None,
            "non-number receivedAt is invalid"
        );
        assert_eq!(
            project_payload("agent.status.changed", &status(json!("w1"), "p", "s", 0.0)),
            None,
            "zero receivedAt is invalid"
        );
        assert_eq!(
            project_payload("agent.status.changed", &status(json!("w1"), "p", "s", -5.0)),
            None,
            "negative receivedAt is invalid"
        );
    }

    #[test]
    fn project_agent_status_truncates_fields() {
        let projected = project_payload(
            "agent.status.changed",
            &status(
                json!("i".repeat(3000)),
                &"k".repeat(3000),
                &"s".repeat(500),
                2.5,
            ),
        )
        .expect("oversized fields truncate");
        assert_eq!(
            projected["worktreeId"].as_str().unwrap().chars().count(),
            2048
        );
        assert_eq!(projected["paneKey"].as_str().unwrap().chars().count(), 2048);
        assert_eq!(projected["state"].as_str().unwrap().chars().count(), 256);
        assert_eq!(projected["receivedAt"], json!(2.5));
    }

    #[test]
    fn project_unknown_event_name_is_none() {
        assert_eq!(
            project_payload("worktree.updated", &created("w1", "/tmp/wt", "main")),
            None
        );
        assert_eq!(project_payload("", &json!({})), None);
        assert_eq!(project_payload("plugin.log", &json!({})), None);
    }
}
