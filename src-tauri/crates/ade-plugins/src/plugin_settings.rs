//! Per-plugin settings document (`settings:own`), persisted as
//! `<data_dir>/<qualifiedKey>/settings.json` (0600).
//!
//! Settings share the storage key-value implementation and its host-api
//! limits; the oracle's settings binding is the same `PluginKvStore` with the
//! `settings.json` file name. `get_all`/`set` are the only surfaces the host
//! API exposes.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::storage::JsonFileStore;
use crate::types::PluginError;

pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// Per-plugin `settings.json` (0600) store. `data_dir` is the scope's
/// `plugin-data` root; every operation keys by the qualified plugin key.
#[derive(Debug, Clone)]
pub struct PluginSettingsStore {
    inner: JsonFileStore,
}

impl PluginSettingsStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: JsonFileStore::new(data_dir.into(), SETTINGS_FILE_NAME),
        }
    }

    pub fn get_all(&self, plugin_key: &str) -> Result<Map<String, Value>, PluginError> {
        self.inner.get_all(plugin_key)
    }

    pub fn set(&self, plugin_key: &str, key: &str, value: Value) -> Result<(), PluginError> {
        self.inner.set(plugin_key, key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{STORAGE_FILE_NAME, STORAGE_VALUE_MAX_BYTES};
    use crate::types::{ACTION_FAILED, INVALID_PARAMS};
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    const KEY: &str = "orca-samples.demo";

    #[test]
    fn settings_round_trip_and_persist_across_instances() {
        let dir = tempdir().unwrap();
        {
            let store = PluginSettingsStore::new(dir.path());
            store
                .set(KEY, "editor", json!({"fontSize": 13, "tabSize": 2}))
                .unwrap();
            store.set(KEY, "flags", json!(["beta"])).unwrap();
            assert_eq!(store.get_all(KEY).unwrap().len(), 2);
        }

        let store = PluginSettingsStore::new(dir.path());
        let all = store.get_all(KEY).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            all.get("editor"),
            Some(&json!({"fontSize": 13, "tabSize": 2}))
        );
        assert_eq!(all.get("flags"), Some(&json!(["beta"])));
    }

    #[test]
    fn writes_settings_json_with_mode_0600() {
        let dir = tempdir().unwrap();
        let store = PluginSettingsStore::new(dir.path());
        store.set(KEY, "theme", json!("dark")).unwrap();

        let path = dir.path().join(KEY).join(SETTINGS_FILE_NAME);
        assert!(path.exists());
        assert!(!dir.path().join(KEY).join(STORAGE_FILE_NAME).exists());
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn isolates_plugin_namespaces_and_missing_settings_are_empty() {
        let dir = tempdir().unwrap();
        let store = PluginSettingsStore::new(dir.path());
        store.set("acme.first", "token", json!("a")).unwrap();

        assert_eq!(store.get_all("acme.second").unwrap().len(), 0);
        assert_eq!(
            store.get_all("acme.first").unwrap().get("token"),
            Some(&json!("a"))
        );
        assert_eq!(store.get_all("acme.missing").unwrap().len(), 0);
    }

    #[test]
    fn rejects_reserved_keys_and_oversized_values() {
        let dir = tempdir().unwrap();
        let store = PluginSettingsStore::new(dir.path());

        let error = store.set(KEY, "constructor", json!(1)).unwrap_err();
        assert_eq!(error.code(), INVALID_PARAMS);
        assert_eq!(error.message(), "reserved storage key");

        let oversized = "x".repeat(STORAGE_VALUE_MAX_BYTES + 1);
        let error = store.set(KEY, "big", json!(oversized)).unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert_eq!(
            error.message(),
            format!("value exceeds {STORAGE_VALUE_MAX_BYTES} bytes")
        );
        assert_eq!(store.get_all(KEY).unwrap().len(), 0);
    }

    #[test]
    fn rejects_unsafe_plugin_keys() {
        let dir = tempdir().unwrap();
        let store = PluginSettingsStore::new(dir.path());
        let error = store.get_all("demo").unwrap_err();
        assert_eq!(error.code(), INVALID_PARAMS);
        assert!(error.message().contains("unsafe plugin key"));
    }
}
