use std::path::PathBuf;

use serde_json::Value;

use crate::json_file::JsonFile;
use crate::StoreError;

pub const DEEP_MERGE_KEYS: &[&str] = &["notifications", "telemetry", "worktreeVisibilityDefaults"];
pub const RENDERER_READONLY_KEYS: &[&str] = &[
    "pluginConsents",
    "disabledPlugins",
    "activeRuntimeEnvironmentId",
    "floatingTerminalTrustedCwds",
];

pub struct SettingsStore {
    file: JsonFile,
    current: Value,
}

impl SettingsStore {
    pub fn load(path: impl Into<PathBuf>, defaults: Value) -> Self {
        let file = JsonFile::new(path);
        let stored = file.load();
        let current = if stored.is_object() {
            crate::shallow_merge(&defaults, &stored, DEEP_MERGE_KEYS)
        } else {
            defaults
        };
        Self { file, current }
    }

    pub fn get(&self) -> Value {
        self.current.clone()
    }

    pub fn snapshot(&self) -> Value {
        self.current.clone()
    }

    pub fn set_partial(&mut self, updates: Value) -> Result<Value, StoreError> {
        let merged = self.merge_partial(updates)?;
        self.persist()?;
        Ok(merged)
    }

    /// Merge a renderer partial into the in-memory snapshot **without persisting**,
    /// so the bridge write scheduler can debounce the disk write (spec §4.1).
    /// Renderer-readonly keys are stripped exactly like [`SettingsStore::set_partial`].
    pub fn merge_partial(&mut self, updates: Value) -> Result<Value, StoreError> {
        if !updates.is_object() {
            return Err(StoreError::InvalidInput(
                "settings updates must be a JSON object".into(),
            ));
        }
        let sanitized = strip_renderer_readonly_keys(&updates);
        self.current = crate::shallow_merge(&self.current, &sanitized, DEEP_MERGE_KEYS);
        Ok(self.current.clone())
    }

    /// Main-owned write path (pluginConsents, disabledPlugins, …): merges the
    /// updates **without** the renderer-readonly stripping and persists
    /// synchronously. Not reachable from the renderer command surface.
    pub fn set_main_owned(&mut self, updates: Value) -> Result<Value, StoreError> {
        if !updates.is_object() {
            return Err(StoreError::InvalidInput(
                "settings updates must be a JSON object".into(),
            ));
        }
        self.current = crate::shallow_merge(&self.current, &updates, DEEP_MERGE_KEYS);
        self.persist()?;
        Ok(self.current.clone())
    }

    /// Persist the current in-memory snapshot atomically.
    pub fn persist(&self) -> Result<(), StoreError> {
        self.file.save(&self.current)?;
        Ok(())
    }
}

pub fn strip_renderer_readonly_keys(updates: &Value) -> Value {
    let mut sanitized = updates.clone();
    if let Some(map) = sanitized.as_object_mut() {
        for key in RENDERER_READONLY_KEYS {
            map.remove(*key);
        }
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    fn defaults() -> Value {
        json!({
            "workspaceDir": "/home/tester/orca/workspaces",
            "theme": "system",
            "notifications": {
                "enabled": true,
                "terminalBell": false,
                "customSoundVolume": 100
            },
            "telemetry": {
                "optedIn": true,
                "installId": "install-1"
            },
            "worktreeVisibilityDefaults": {
                "external": "hide",
                "nested": { "keep": true }
            },
            "pluginConsents": {},
            "disabledPlugins": [],
            "activeRuntimeEnvironmentId": null,
            "floatingTerminalTrustedCwds": []
        })
    }

    fn load_store(dir: &TestDir) -> SettingsStore {
        SettingsStore::load(dir.file("settings.json"), defaults())
    }

    #[test]
    fn get_returns_defaults_when_file_is_missing() {
        let dir = TestDir::new("settings-missing");
        assert_eq!(load_store(&dir).get(), defaults());
    }

    #[test]
    fn load_merges_defaults_with_stored() {
        let dir = TestDir::new("settings-load-merge");
        std::fs::write(dir.file("settings.json"), r#"{"theme":"dark"}"#).unwrap();
        let loaded = load_store(&dir).get();
        assert_eq!(loaded["theme"], "dark");
        assert_eq!(loaded["workspaceDir"], defaults()["workspaceDir"]);
        assert_eq!(loaded["notifications"]["enabled"], true);
    }

    #[test]
    fn set_partial_shallow_merges_top_level_keys() {
        let dir = TestDir::new("settings-shallow");
        let mut store = load_store(&dir);
        store.set_partial(json!({ "theme": "dark" })).unwrap();
        let settings = store.get();
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["workspaceDir"], defaults()["workspaceDir"]);
    }

    #[test]
    fn set_partial_deep_merges_notifications() {
        let dir = TestDir::new("settings-deep-notifications");
        let mut store = load_store(&dir);
        store
            .set_partial(json!({ "notifications": { "terminalBell": true } }))
            .unwrap();
        let notifications = &store.get()["notifications"];
        assert_eq!(notifications["terminalBell"], true);
        assert_eq!(notifications["enabled"], true);
        assert_eq!(notifications["customSoundVolume"], 100);
    }

    #[test]
    fn set_partial_deep_merges_telemetry() {
        let dir = TestDir::new("settings-deep-telemetry");
        let mut store = load_store(&dir);
        store
            .set_partial(json!({ "telemetry": { "optedIn": false } }))
            .unwrap();
        let telemetry = &store.get()["telemetry"];
        assert_eq!(telemetry["optedIn"], false);
        assert_eq!(telemetry["installId"], "install-1");
    }

    #[test]
    fn set_partial_deep_merges_worktree_visibility_defaults() {
        let dir = TestDir::new("settings-deep-visibility");
        let mut store = load_store(&dir);
        store
            .set_partial(json!({ "worktreeVisibilityDefaults": { "external": "show" } }))
            .unwrap();
        let visibility = &store.get()["worktreeVisibilityDefaults"];
        assert_eq!(visibility["external"], "show");
        assert_eq!(visibility["nested"]["keep"], true);
    }

    #[test]
    fn set_partial_ignores_renderer_readonly_keys() {
        let dir = TestDir::new("settings-readonly");
        let mut store = load_store(&dir);
        store
            .set_partial(json!({
                "theme": "dark",
                "pluginConsents": { "orca-samples.demo": "forged" },
                "disabledPlugins": ["demo"],
                "activeRuntimeEnvironmentId": "forged-env",
                "floatingTerminalTrustedCwds": ["/forged"]
            }))
            .unwrap();
        let settings = store.get();
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["pluginConsents"], json!({}));
        assert_eq!(settings["disabledPlugins"], json!([]));
        assert_eq!(settings["activeRuntimeEnvironmentId"], Value::Null);
        assert_eq!(settings["floatingTerminalTrustedCwds"], json!([]));
    }

    #[test]
    fn set_partial_returns_complete_object() {
        let dir = TestDir::new("settings-return-complete");
        let mut store = load_store(&dir);
        let returned = store.set_partial(json!({ "theme": "dark" })).unwrap();
        assert_eq!(returned, store.get());
        assert_eq!(returned["theme"], "dark");
        assert_eq!(returned["workspaceDir"], defaults()["workspaceDir"]);
        assert_eq!(returned["notifications"]["enabled"], true);
    }

    #[test]
    fn set_partial_persists_across_reload() {
        let dir = TestDir::new("settings-persist");
        let mut store = load_store(&dir);
        store.set_partial(json!({ "theme": "dark" })).unwrap();
        let reloaded = load_store(&dir);
        assert_eq!(reloaded.get()["theme"], "dark");
    }

    #[test]
    fn snapshot_tracks_latest_set() {
        let dir = TestDir::new("settings-snapshot");
        let mut store = load_store(&dir);
        store.set_partial(json!({ "theme": "dark" })).unwrap();
        assert_eq!(store.snapshot(), store.get());
    }

    #[test]
    fn set_partial_rejects_non_object_payload() {
        let dir = TestDir::new("settings-invalid");
        let mut store = load_store(&dir);
        assert!(matches!(
            store.set_partial(json!(["nope"])),
            Err(StoreError::InvalidInput(_))
        ));
    }

    #[test]
    fn merge_partial_does_not_persist() {
        let dir = TestDir::new("settings-merge-no-persist");
        let mut store = load_store(&dir);
        store
            .merge_partial(json!({ "theme": "dark", "pluginConsents": { "forged": true } }))
            .unwrap();
        assert_eq!(store.get()["theme"], "dark");
        assert!(!dir.file("settings.json").exists());
        assert_eq!(load_store(&dir).get()["theme"], "system");
    }

    #[test]
    fn set_main_owned_keeps_readonly_keys_and_persists() {
        let dir = TestDir::new("settings-main-owned");
        let mut store = load_store(&dir);
        store
            .set_main_owned(json!({
                "pluginConsents": { "orca-samples.demo": "granted" },
                "disabledPlugins": ["demo"],
                "activeRuntimeEnvironmentId": "env-1",
                "theme": "dark"
            }))
            .unwrap();
        let settings = store.get();
        assert_eq!(
            settings["pluginConsents"],
            json!({ "orca-samples.demo": "granted" })
        );
        assert_eq!(settings["disabledPlugins"], json!(["demo"]));
        assert_eq!(settings["activeRuntimeEnvironmentId"], "env-1");
        assert_eq!(settings["theme"], "dark");

        let reloaded = load_store(&dir).get();
        assert_eq!(
            reloaded["pluginConsents"],
            json!({ "orca-samples.demo": "granted" })
        );
        assert_eq!(reloaded["disabledPlugins"], json!(["demo"]));
        assert_eq!(reloaded["activeRuntimeEnvironmentId"], "env-1");
    }

    #[test]
    fn set_main_owned_rejects_non_object_payload() {
        let dir = TestDir::new("settings-main-owned-invalid");
        let mut store = load_store(&dir);
        assert!(matches!(
            store.set_main_owned(json!("nope")),
            Err(StoreError::InvalidInput(_))
        ));
    }
}
