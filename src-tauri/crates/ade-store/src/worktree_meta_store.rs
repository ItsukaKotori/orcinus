use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde_json::{Map, Value};

use crate::json_file::JsonFile;
use crate::{StoreError, SCHEMA_VERSION};

pub const WORKTREE_META_FIELDS: &[&str] = &[
    "instanceId",
    "projectId",
    "hostId",
    "projectHostSetupId",
    "ephemeralVmCheckoutMode",
    "creatorProvenance",
    "displayName",
    "displayNameIsPinned",
    "comment",
    "linkedIssue",
    "linkedPR",
    "suppressedGitHubPR",
    "linkedLinearIssue",
    "linkedLinearIssueWorkspaceId",
    "linkedLinearIssueOrganizationUrlKey",
    "linkedGitLabMR",
    "linkedGitLabIssue",
    "linkedBitbucketPR",
    "linkedAzureDevOpsPR",
    "linkedGiteaPR",
    "linkedWorkItem",
    "linkedTaskSourceContext",
    "isArchived",
    "isUnread",
    "isPinned",
    "sortOrder",
    "manualOrder",
    "lastActivityAt",
    "createdAt",
    "createdWithAgent",
    "pendingFirstAgentMessageRename",
    "firstAgentMessageRenameError",
    "sparseDirectories",
    "sparseBaseRef",
    "sparsePresetId",
    "baseRef",
    "preserveBranchOnDelete",
    "pushTarget",
    "orcaCreatedAt",
    "orcaCreationSource",
    "orcaCreationWorkspaceLayout",
    "workspaceStatus",
    "diffComments",
    "priorWorktreeIds",
    "mobileDiffReview",
    "automationProvenance",
    "cliProvenance",
];

const ITEMS: &str = "items";

pub struct WorktreeMetaStore {
    path: PathBuf,
    inner: Mutex<Value>,
}

impl WorktreeMetaStore {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let state = normalize_state(JsonFile::new(&path).load());
        Self {
            path,
            inner: Mutex::new(state),
        }
    }

    pub fn items(&self) -> Map<String, Value> {
        items_of(&self.lock())
    }

    pub fn get(&self, worktree_id: &str) -> Option<Value> {
        self.lock()
            .get(ITEMS)
            .and_then(Value::as_object)
            .and_then(|items| items.get(worktree_id))
            .cloned()
    }

    pub fn merge(&self, worktree_id: &str, updates: &Value) -> Result<Value, StoreError> {
        let updates = updates.as_object().ok_or_else(|| {
            StoreError::InvalidInput("worktree meta updates must be a JSON object".into())
        })?;
        let mut state = self.lock();
        let mut items = items_of(&state);
        let mut entry = items
            .get(worktree_id)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (key, value) in updates {
            if !WORKTREE_META_FIELDS.contains(&key.as_str()) {
                continue;
            }
            if value.is_null() || (key == "displayName" && value.as_str() == Some("")) {
                entry.remove(key);
            } else {
                entry.insert(key.clone(), value.clone());
            }
        }
        let merged = Value::Object(entry);
        items.insert(worktree_id.to_string(), merged.clone());
        let next = state_with_items(&state, items);
        self.persist_state(&next)?;
        *state = next;
        Ok(merged)
    }

    pub fn remove(&self, worktree_id: &str) -> Result<bool, StoreError> {
        let mut state = self.lock();
        let mut items = items_of(&state);
        if items.remove(worktree_id).is_none() {
            return Ok(false);
        }
        let next = state_with_items(&state, items);
        self.persist_state(&next)?;
        *state = next;
        Ok(true)
    }

    pub fn persist_sort_order(&self, ordered_ids: &[String]) -> Result<(), StoreError> {
        let mut state = self.lock();
        let mut items = items_of(&state);
        for (index, id) in ordered_ids.iter().enumerate() {
            let mut entry = items
                .get(id)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            entry.insert("sortOrder".to_string(), Value::from(index as u64));
            items.insert(id.clone(), Value::Object(entry));
        }
        let next = state_with_items(&state, items);
        self.persist_state(&next)?;
        *state = next;
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Value> {
        self.inner
            .lock()
            .expect("worktree meta store lock poisoned")
    }

    fn persist_state(&self, state: &Value) -> Result<(), StoreError> {
        JsonFile::new(&self.path).save(state)?;
        Ok(())
    }
}

fn normalize_state(stored: Value) -> Value {
    let mut state = match stored {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let schema_version = state
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(SCHEMA_VERSION);
    state.insert("schemaVersion".to_string(), Value::from(schema_version));
    if !state.get(ITEMS).is_some_and(Value::is_object) {
        state.insert(ITEMS.to_string(), Value::Object(Map::new()));
    }
    Value::Object(state)
}

fn items_of(state: &Value) -> Map<String, Value> {
    state
        .get(ITEMS)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn state_with_items(state: &Value, items: Map<String, Value>) -> Value {
    let mut map = state.as_object().cloned().unwrap_or_default();
    map.insert(ITEMS.to_string(), Value::Object(items));
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    fn load_store(dir: &TestDir) -> WorktreeMetaStore {
        WorktreeMetaStore::load(dir.file("worktrees.json"))
    }

    #[test]
    fn merge_persists_whitelisted_fields_and_returns_merged_value() {
        let dir = TestDir::new("worktree-meta-merge");
        let store = load_store(&dir);
        let merged = store
            .merge(
                "r::/p",
                &json!({"displayName":"fix","isPinned":true,"bogus":1}),
            )
            .unwrap();
        assert_eq!(merged["displayName"], "fix");
        assert_eq!(merged["isPinned"], true);
        assert!(merged.get("bogus").is_none());
        assert_eq!(load_store(&dir).get("r::/p").unwrap()["displayName"], "fix");
    }

    #[test]
    fn merge_with_null_or_empty_display_name_clears_the_key() {
        let dir = TestDir::new("worktree-meta-clear");
        let store = load_store(&dir);
        store.merge("r::/p", &json!({"displayName":"fix"})).unwrap();
        let merged = store.merge("r::/p", &json!({"displayName":""})).unwrap();
        assert!(merged.get("displayName").is_none());
        assert_eq!(load_store(&dir).get("r::/p").unwrap(), json!({}));

        store.merge("r::/p", &json!({"isUnread": true})).unwrap();
        let merged = store.merge("r::/p", &json!({"isUnread": null})).unwrap();
        assert!(merged.get("isUnread").is_none());
        assert_eq!(load_store(&dir).get("r::/p").unwrap(), json!({}));
    }

    #[test]
    fn merge_ignores_unknown_keys_and_preserves_existing_fields() {
        let dir = TestDir::new("worktree-meta-unknown");
        let store = load_store(&dir);
        store
            .merge("r::/p", &json!({"displayName":"fix","isPinned":true}))
            .unwrap();
        let merged = store
            .merge("r::/p", &json!({"isUnread":true,"nope":"x"}))
            .unwrap();
        assert_eq!(merged["displayName"], "fix");
        assert_eq!(merged["isPinned"], true);
        assert_eq!(merged["isUnread"], true);
        assert!(merged.get("nope").is_none());
    }

    #[test]
    fn merge_rejects_non_object_payload() {
        let dir = TestDir::new("worktree-meta-invalid");
        let store = load_store(&dir);
        assert!(matches!(
            store.merge("r::/p", &json!("nope")),
            Err(StoreError::InvalidInput(_))
        ));
        assert!(matches!(
            store.merge("r::/p", &json!([1, 2])),
            Err(StoreError::InvalidInput(_))
        ));
        assert!(!dir.file("worktrees.json").exists());
    }

    #[test]
    fn get_returns_none_until_merge_creates_the_entry() {
        let dir = TestDir::new("worktree-meta-create");
        let store = load_store(&dir);
        assert!(store.get("w::/x").is_none());
        store.merge("w::/x", &json!({"isArchived":true})).unwrap();
        assert_eq!(store.get("w::/x").unwrap(), json!({"isArchived":true}));
    }

    #[test]
    fn remove_deletes_entry_and_reports_presence() {
        let dir = TestDir::new("worktree-meta-remove");
        let store = load_store(&dir);
        store.merge("r::/p", &json!({"isUnread":true})).unwrap();
        assert!(store.remove("r::/p").unwrap());
        assert!(store.get("r::/p").is_none());
        assert!(load_store(&dir).get("r::/p").is_none());
        assert!(!store.remove("r::/p").unwrap());
    }

    #[test]
    fn persist_sort_order_assigns_indexes_and_persists() {
        let dir = TestDir::new("worktree-meta-sort");
        let store = load_store(&dir);
        store.persist_sort_order(&["a".into(), "b".into()]).unwrap();
        assert_eq!(store.get("a").unwrap()["sortOrder"], 0);
        assert_eq!(store.get("b").unwrap()["sortOrder"], 1);
        assert_eq!(load_store(&dir).get("b").unwrap()["sortOrder"], 1);
    }

    #[test]
    fn items_returns_all_entries() {
        let dir = TestDir::new("worktree-meta-items");
        let store = load_store(&dir);
        assert!(store.items().is_empty());
        store.merge("a", &json!({"isPinned":true})).unwrap();
        store.merge("b", &json!({"isUnread":true})).unwrap();
        let items = store.items();
        assert_eq!(items.len(), 2);
        assert_eq!(items.get("a").unwrap(), &json!({"isPinned":true}));
        assert_eq!(items.get("b").unwrap(), &json!({"isUnread":true}));
    }

    #[test]
    fn load_reads_existing_state_and_persists_the_documented_shape() {
        let dir = TestDir::new("worktree-meta-shape");
        std::fs::write(
            dir.file("worktrees.json"),
            r#"{"schemaVersion":1,"items":{"w1":{"displayName":"one","isPinned":true}}}"#,
        )
        .unwrap();
        let store = load_store(&dir);
        assert_eq!(store.get("w1").unwrap()["displayName"], "one");
        store.merge("w2", &json!({"isUnread":true})).unwrap();
        let saved = JsonFile::new(dir.file("worktrees.json")).load();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(
            saved["items"]["w1"],
            json!({"displayName":"one","isPinned":true})
        );
        assert_eq!(saved["items"]["w2"], json!({"isUnread":true}));
    }

    #[test]
    fn corrupt_file_falls_back_to_empty_state_and_recovers_on_write() {
        let dir = TestDir::new("worktree-meta-corrupt");
        std::fs::write(dir.file("worktrees.json"), "not json").unwrap();
        let store = load_store(&dir);
        assert!(store.items().is_empty());
        store.merge("w1", &json!({"isUnread":true})).unwrap();
        let saved = JsonFile::new(dir.file("worktrees.json")).load();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["items"]["w1"]["isUnread"], true);
    }
}
