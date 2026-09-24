use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::json_file::JsonFile;
use crate::StoreError;

const DEEP_MERGE_KEYS: &[&str] = &["workspaceCleanup"];
const CONTEXTUAL_TOURS_SEEN_IDS: &str = "contextualToursSeenIds";
const FEATURE_INTERACTIONS: &str = "featureInteractions";

pub struct UiStateStore {
    file: JsonFile,
    current: Value,
}

impl UiStateStore {
    pub fn load(path: impl Into<PathBuf>, defaults: Value) -> Self {
        let file = JsonFile::new(path);
        let stored = file.load();
        let current = if stored.is_object() {
            merge_ui_state(&defaults, &stored)
        } else {
            defaults
        };
        Self { file, current }
    }

    pub fn get(&self) -> Value {
        self.current.clone()
    }

    pub fn set(&mut self, updates: Value) -> Result<Value, StoreError> {
        let merged = self.merge(updates)?;
        self.persist()?;
        Ok(merged)
    }

    /// Merge updates into the in-memory snapshot **without persisting**, so the
    /// bridge write scheduler can debounce the disk write (spec §4.1).
    pub fn merge(&mut self, updates: Value) -> Result<Value, StoreError> {
        if !updates.is_object() {
            return Err(StoreError::InvalidInput(
                "ui-state updates must be a JSON object".into(),
            ));
        }
        self.current = merge_ui_state(&self.current, &updates);
        Ok(self.current.clone())
    }

    /// Persist the current in-memory snapshot atomically.
    pub fn persist(&self) -> Result<(), StoreError> {
        self.file.save(&self.current)?;
        Ok(())
    }

    pub fn record_feature_interaction(&mut self, id: &str) -> Result<Value, StoreError> {
        self.record_feature_interaction_at(id, now_millis())
    }

    pub fn record_feature_interaction_at(
        &mut self,
        id: &str,
        now_ms: u64,
    ) -> Result<Value, StoreError> {
        let state = self.record_feature_interaction_in_memory_at(id, now_ms)?;
        self.persist()?;
        Ok(state)
    }

    /// Record an interaction in memory only; the caller persists on its own
    /// schedule. Returns the complete merged state.
    pub fn record_feature_interaction_in_memory(&mut self, id: &str) -> Result<Value, StoreError> {
        self.record_feature_interaction_in_memory_at(id, now_millis())
    }

    pub fn record_feature_interaction_in_memory_at(
        &mut self,
        id: &str,
        now_ms: u64,
    ) -> Result<Value, StoreError> {
        let interaction_count = self
            .current
            .get(FEATURE_INTERACTIONS)
            .and_then(|interactions| interactions.get(id))
            .and_then(|record| record.get("interactionCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;
        let record = serde_json::json!({
            "firstInteractedAt": now_ms,
            "interactionCount": interaction_count
        });
        let mut interactions = Map::new();
        interactions.insert(id.to_string(), record);
        let mut updates = Map::new();
        updates.insert(
            FEATURE_INTERACTIONS.to_string(),
            Value::Object(interactions),
        );
        self.merge(Value::Object(updates))
    }
}

fn merge_ui_state(base: &Value, updates: &Value) -> Value {
    let mut merged = crate::shallow_merge(base, updates, DEEP_MERGE_KEYS);
    let Some(merged_map) = merged.as_object_mut() else {
        return merged;
    };
    if let Some(incoming) = updates.get(CONTEXTUAL_TOURS_SEEN_IDS) {
        let mut seen = Vec::new();
        collect_tour_ids(base.get(CONTEXTUAL_TOURS_SEEN_IDS), &mut seen);
        collect_tour_ids(Some(incoming), &mut seen);
        merged_map.insert(
            CONTEXTUAL_TOURS_SEEN_IDS.to_string(),
            Value::Array(seen.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(incoming) = updates.get(FEATURE_INTERACTIONS) {
        merged_map.insert(
            FEATURE_INTERACTIONS.to_string(),
            merge_feature_interactions(base.get(FEATURE_INTERACTIONS), incoming),
        );
    }
    merged
}

fn collect_tour_ids(value: Option<&Value>, out: &mut Vec<String>) {
    let Some(Value::Array(items)) = value else {
        return;
    };
    for item in items {
        if let Some(id) = item.as_str() {
            if !out.iter().any(|existing| existing == id) {
                out.push(id.to_string());
            }
        }
    }
}

fn merge_feature_interactions(current: Option<&Value>, incoming: &Value) -> Value {
    let Some(incoming_map) = incoming.as_object() else {
        return current
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
    };
    let mut merged = current
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (id, incoming_record) in incoming_map {
        let next = match merged.get(id) {
            Some(existing) if existing.is_object() && incoming_record.is_object() => {
                merge_interaction_record(existing, incoming_record)
            }
            _ => incoming_record.clone(),
        };
        merged.insert(id.clone(), next);
    }
    Value::Object(merged)
}

fn merge_interaction_record(current: &Value, incoming: &Value) -> Value {
    let first = min_u64(
        current.get("firstInteractedAt"),
        incoming.get("firstInteractedAt"),
    );
    let count = max_u64(
        current.get("interactionCount"),
        incoming.get("interactionCount"),
    );
    match (first, count) {
        (Some(first), Some(count)) => serde_json::json!({
            "firstInteractedAt": first,
            "interactionCount": count
        }),
        _ => incoming.clone(),
    }
}

fn min_u64(a: Option<&Value>, b: Option<&Value>) -> Option<u64> {
    match (a.and_then(Value::as_u64), b.and_then(Value::as_u64)) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn max_u64(a: Option<&Value>, b: Option<&Value>) -> Option<u64> {
    match (a.and_then(Value::as_u64), b.and_then(Value::as_u64)) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    fn defaults() -> Value {
        json!({
            "lastActiveRepoId": null,
            "sidebarWidth": 280,
            "filterRepoIds": [],
            "statusBarItems": ["terminal", "tasks"],
            "contextualToursSeenIds": ["intro"],
            "featureInteractions": {
                "tasks": { "firstInteractedAt": 100, "interactionCount": 2 }
            },
            "workspaceCleanup": {
                "dismissals": { "repo-a": 1 },
                "browse": { "path": "/repo/a" }
            }
        })
    }

    fn load_store(dir: &TestDir) -> UiStateStore {
        UiStateStore::load(dir.file("ui-state.json"), defaults())
    }

    #[test]
    fn load_merges_defaults_with_stored() {
        let dir = TestDir::new("ui-load-merge");
        std::fs::write(dir.file("ui-state.json"), r#"{"sidebarWidth":300}"#).unwrap();
        let state = load_store(&dir).get();
        assert_eq!(state["sidebarWidth"], 300);
        assert_eq!(state["statusBarItems"], json!(["terminal", "tasks"]));
    }

    #[test]
    fn set_replaces_arrays_wholesale() {
        let dir = TestDir::new("ui-arrays");
        let mut store = load_store(&dir);
        store.set(json!({ "filterRepoIds": ["a", "b"] })).unwrap();
        assert_eq!(store.get()["filterRepoIds"], json!(["a", "b"]));

        store.set(json!({ "filterRepoIds": ["c"] })).unwrap();
        assert_eq!(store.get()["filterRepoIds"], json!(["c"]));

        store.set(json!({ "statusBarItems": ["tasks"] })).unwrap();
        assert_eq!(store.get()["statusBarItems"], json!(["tasks"]));
    }

    #[test]
    fn set_unions_contextual_tours_seen_ids() {
        let dir = TestDir::new("ui-tours");
        let mut store = load_store(&dir);
        store
            .set(json!({ "contextualToursSeenIds": ["intro", "tour-b"] }))
            .unwrap();
        assert_eq!(
            store.get()["contextualToursSeenIds"],
            json!(["intro", "tour-b"])
        );

        store
            .set(json!({ "contextualToursSeenIds": ["tour-b", "tour-c"] }))
            .unwrap();
        assert_eq!(
            store.get()["contextualToursSeenIds"],
            json!(["intro", "tour-b", "tour-c"])
        );
    }

    #[test]
    fn set_merges_feature_interactions_per_id() {
        let dir = TestDir::new("ui-interactions");
        let mut store = load_store(&dir);
        store
            .set(json!({
                "featureInteractions": {
                    "tasks": { "firstInteractedAt": 50, "interactionCount": 5 },
                    "ports": { "firstInteractedAt": 10, "interactionCount": 1 }
                }
            }))
            .unwrap();
        let interactions = &store.get()["featureInteractions"];
        assert_eq!(
            interactions["tasks"],
            json!({ "firstInteractedAt": 50, "interactionCount": 5 })
        );
        assert_eq!(
            interactions["ports"],
            json!({ "firstInteractedAt": 10, "interactionCount": 1 })
        );
    }

    #[test]
    fn set_merges_feature_interactions_keeps_later_start_and_higher_count() {
        let dir = TestDir::new("ui-interactions-stale");
        let mut store = load_store(&dir);
        store
            .set(json!({
                "featureInteractions": {
                    "tasks": { "firstInteractedAt": 500, "interactionCount": 1 }
                }
            }))
            .unwrap();
        assert_eq!(
            store.get()["featureInteractions"]["tasks"],
            json!({ "firstInteractedAt": 100, "interactionCount": 2 })
        );
    }

    #[test]
    fn set_deep_merges_workspace_cleanup() {
        let dir = TestDir::new("ui-cleanup");
        let mut store = load_store(&dir);
        store
            .set(json!({ "workspaceCleanup": { "dismissals": { "repo-b": 2 } } }))
            .unwrap();
        let cleanup = &store.get()["workspaceCleanup"];
        assert_eq!(cleanup["dismissals"], json!({ "repo-a": 1, "repo-b": 2 }));
        assert_eq!(cleanup["browse"], json!({ "path": "/repo/a" }));
    }

    #[test]
    fn set_returns_complete_object_and_persists() {
        let dir = TestDir::new("ui-persist");
        let mut store = load_store(&dir);
        let returned = store.set(json!({ "sidebarWidth": 320 })).unwrap();
        assert_eq!(returned, store.get());
        assert_eq!(returned["sidebarWidth"], 320);
        assert_eq!(returned["lastActiveRepoId"], Value::Null);
        assert_eq!(load_store(&dir).get()["sidebarWidth"], 320);
    }

    #[test]
    fn set_rejects_non_object_payload() {
        let dir = TestDir::new("ui-invalid");
        let mut store = load_store(&dir);
        assert!(matches!(
            store.set(json!("nope")),
            Err(StoreError::InvalidInput(_))
        ));
    }

    #[test]
    fn record_feature_interaction_increments_existing() {
        let dir = TestDir::new("ui-record-existing");
        let mut store = load_store(&dir);
        let state = store.record_feature_interaction_at("tasks", 500).unwrap();
        assert_eq!(
            state["featureInteractions"]["tasks"],
            json!({ "firstInteractedAt": 100, "interactionCount": 3 })
        );
    }

    #[test]
    fn record_feature_interaction_starts_new_id_at_one() {
        let dir = TestDir::new("ui-record-new");
        let mut store = load_store(&dir);
        let state = store.record_feature_interaction_at("ports", 500).unwrap();
        assert_eq!(
            state["featureInteractions"]["ports"],
            json!({ "firstInteractedAt": 500, "interactionCount": 1 })
        );
    }

    #[test]
    fn record_feature_interaction_keeps_earliest_timestamp() {
        let dir = TestDir::new("ui-record-earliest");
        let mut store = load_store(&dir);
        let state = store.record_feature_interaction_at("tasks", 50).unwrap();
        assert_eq!(
            state["featureInteractions"]["tasks"],
            json!({ "firstInteractedAt": 50, "interactionCount": 3 })
        );
    }

    #[test]
    fn merge_does_not_persist_and_persist_writes_snapshot() {
        let dir = TestDir::new("ui-merge-no-persist");
        let mut store = load_store(&dir);
        store.merge(json!({ "sidebarWidth": 300 })).unwrap();
        assert_eq!(store.get()["sidebarWidth"], 300);
        assert!(!dir.file("ui-state.json").exists());
        store.persist().unwrap();
        assert_eq!(load_store(&dir).get()["sidebarWidth"], 300);
    }

    #[test]
    fn record_feature_interaction_in_memory_defers_persistence() {
        let dir = TestDir::new("ui-record-in-memory");
        let mut store = load_store(&dir);
        let state = store
            .record_feature_interaction_in_memory_at("ports", 500)
            .unwrap();
        assert_eq!(
            state["featureInteractions"]["ports"],
            json!({ "firstInteractedAt": 500, "interactionCount": 1 })
        );
        assert!(!dir.file("ui-state.json").exists());
        store.persist().unwrap();
        assert_eq!(
            load_store(&dir).get()["featureInteractions"]["ports"],
            json!({ "firstInteractedAt": 500, "interactionCount": 1 })
        );
    }

    #[test]
    fn record_feature_interaction_uses_wall_clock_and_persists() {
        let dir = TestDir::new("ui-record-clock");
        let mut store = load_store(&dir);
        let state = store.record_feature_interaction("ports").unwrap();
        let record = &state["featureInteractions"]["ports"];
        assert_eq!(record["interactionCount"], 1);
        assert!(record["firstInteractedAt"].as_u64().unwrap() > 0);
        assert_eq!(
            load_store(&dir).get()["featureInteractions"]["ports"],
            record.clone()
        );
    }
}
