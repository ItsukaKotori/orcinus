use std::path::PathBuf;

use serde_json::Value;

use crate::json_file::JsonFile;
use crate::StoreError;

pub struct OnboardingStore {
    file: JsonFile,
    current: Value,
}

impl OnboardingStore {
    pub fn load(path: impl Into<PathBuf>, defaults: Value) -> Self {
        let file = JsonFile::new(path);
        let stored = file.load();
        let current = if stored.is_object() {
            merge_onboarding(&defaults, &stored)
        } else {
            defaults
        };
        Self { file, current }
    }

    pub fn get(&self) -> Value {
        self.current.clone()
    }

    /// Merge a renderer partial into the snapshot and persist it atomically.
    /// Top-level keys are shallow-merged; `checklist` merges field-by-field so
    /// a partial checklist cannot wipe flags the UI already observed.
    pub fn update(&mut self, updates: Value) -> Result<Value, StoreError> {
        if !updates.is_object() {
            return Err(StoreError::InvalidInput(
                "onboarding updates must be a JSON object".into(),
            ));
        }
        self.current = merge_onboarding(&self.current, &updates);
        self.file.save(&self.current)?;
        Ok(self.current.clone())
    }
}

fn merge_onboarding(base: &Value, updates: &Value) -> Value {
    let mut merged = crate::shallow_merge(base, updates, &[]);
    if let (Some(merged_map), Some(incoming)) = (merged.as_object_mut(), updates.get("checklist"))
    {
        let mut checklist = base
            .get("checklist")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        // Why: mirrors the mock's `{...state.checklist, ...updates.checklist}`;
        // a non-object checklist (null) leaves the existing flags untouched.
        if let Some(incoming_map) = incoming.as_object() {
            for (key, value) in incoming_map {
                checklist.insert(key.clone(), value.clone());
            }
        }
        merged_map.insert("checklist".into(), Value::Object(checklist));
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    fn defaults() -> Value {
        json!({
            "flowVersion": 4,
            "closedAt": null,
            "outcome": null,
            "lastCompletedStep": -1,
            "checklist": {
                "addedRepo": false,
                "choseAgent": false,
                "ranFirstAgent": false,
                "ranSecondAgentOnSameTask": false,
                "triedCmdJ": false,
                "shapedSidebar": false,
                "reviewedDiff": false,
                "openedPr": false,
                "addedFolder": false,
                "openedFile": false,
                "ranAgentOnFile": false,
                "dismissed": false
            }
        })
    }

    fn load_store(dir: &TestDir) -> OnboardingStore {
        OnboardingStore::load(dir.file("onboarding.json"), defaults())
    }

    #[test]
    fn get_returns_defaults_when_file_is_missing() {
        let dir = TestDir::new("onboarding-missing");
        assert_eq!(load_store(&dir).get(), defaults());
    }

    #[test]
    fn load_merges_defaults_with_stored() {
        let dir = TestDir::new("onboarding-load-merge");
        std::fs::write(
            dir.file("onboarding.json"),
            r#"{"lastCompletedStep":2,"checklist":{"choseAgent":true}}"#,
        )
        .unwrap();
        let state = load_store(&dir).get();
        assert_eq!(state["lastCompletedStep"], 2);
        assert_eq!(state["flowVersion"], 4);
        assert_eq!(state["checklist"]["choseAgent"], true);
        assert_eq!(state["checklist"]["addedRepo"], false);
    }

    #[test]
    fn load_uses_defaults_when_the_stored_file_is_corrupt() {
        let dir = TestDir::new("onboarding-corrupt");
        std::fs::write(dir.file("onboarding.json"), "{ not json").unwrap();
        assert_eq!(load_store(&dir).get(), defaults());
    }

    #[test]
    fn update_shallow_merges_top_level_keys() {
        let dir = TestDir::new("onboarding-shallow");
        let mut store = load_store(&dir);
        store
            .update(json!({ "lastCompletedStep": 3, "outcome": "completed" }))
            .unwrap();
        let state = store.get();
        assert_eq!(state["lastCompletedStep"], 3);
        assert_eq!(state["outcome"], "completed");
        assert_eq!(state["flowVersion"], 4);
        assert_eq!(state["closedAt"], Value::Null);
    }

    #[test]
    fn update_merges_checklist_field_by_field() {
        let dir = TestDir::new("onboarding-checklist");
        let mut store = load_store(&dir);
        store
            .update(json!({ "checklist": { "choseAgent": true } }))
            .unwrap();
        let state = store.update(json!({ "checklist": { "dismissed": true } })).unwrap();
        assert_eq!(state["checklist"]["choseAgent"], true);
        assert_eq!(state["checklist"]["dismissed"], true);
        assert_eq!(state["checklist"]["addedRepo"], false);
    }

    #[test]
    fn update_preserves_unknown_top_level_keys() {
        let dir = TestDir::new("onboarding-unknown-keys");
        std::fs::write(
            dir.file("onboarding.json"),
            r#"{"lastCompletedStep":1,"futureFlag":true}"#,
        )
        .unwrap();
        let mut store = load_store(&dir);
        store.update(json!({ "lastCompletedStep": 2 })).unwrap();
        let state = store.get();
        assert_eq!(state["futureFlag"], true);
        assert_eq!(state["lastCompletedStep"], 2);
    }

    #[test]
    fn update_returns_complete_object_and_persists_across_reload() {
        let dir = TestDir::new("onboarding-persist");
        let mut store = load_store(&dir);
        let returned = store.update(json!({ "lastCompletedStep": 4 })).unwrap();
        assert_eq!(returned, store.get());
        assert_eq!(returned["lastCompletedStep"], 4);
        assert_eq!(returned["flowVersion"], 4);

        let reloaded = load_store(&dir).get();
        assert_eq!(reloaded["lastCompletedStep"], 4);
        assert_eq!(reloaded["checklist"]["addedRepo"], false);
    }

    #[test]
    fn update_rejects_non_object_payload() {
        let dir = TestDir::new("onboarding-invalid");
        let mut store = load_store(&dir);
        assert!(matches!(
            store.update(json!("nope")),
            Err(StoreError::InvalidInput(_))
        ));
    }
}
