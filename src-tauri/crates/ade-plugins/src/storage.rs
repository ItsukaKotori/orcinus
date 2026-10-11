//! Per-plugin key-value persistence backing both `storage.*` and
//! `settings.*` (settings:own).
//!
//! Ported from `orca:src/main/plugins/plugin-storage-store.ts`: each plugin's
//! data lives in its own 0600 JSON document under
//! `<data_dir>/<qualifiedKey>/<file>`, so one plugin's path can never resolve
//! into another's. Caps mirror `orca:src/shared/plugins/plugin-host-api.ts`:
//! a value serializes to at most 256 KiB, the document to at most 5 MiB, and
//! at most 1024 keys. Reserved names (`__proto__`, `prototype`, `constructor`)
//! are rejected outright, mirroring the host-api key schema.
//!
//! An existing store the process may not read is treated as an error sentinel:
//! `set`/`delete` refuse to rewrite it (a rewrite would silently drop every
//! other key) while reads project as empty, exactly like the oracle.

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::discovery::is_qualified_plugin_key;
use crate::install::atomic_write::write_atomic;
use crate::types::{PluginError, ACTION_FAILED, INVALID_PARAMS};

pub const STORAGE_FILE_NAME: &str = "storage.json";
/// A single key's serialized value may not exceed 256 KiB.
pub const STORAGE_VALUE_MAX_BYTES: usize = 256 * 1024;
/// The whole per-plugin document may not exceed 5 MiB.
pub const STORAGE_TOTAL_MAX_BYTES: usize = 5 * 1024 * 1024;
/// At most 1024 keys per plugin.
pub const STORAGE_KEY_LIMIT: usize = 1024;
/// Host-api key schema: 1..=256 UTF-16 code units.
pub const STORAGE_KEY_MAX_CHARS: usize = 256;
/// Names the host-api key schema refuses outright.
pub const RESERVED_STORAGE_KEYS: [&str; 3] = ["__proto__", "prototype", "constructor"];

const UNREADABLE_STORE_ERROR: &str =
    "storage file exists but could not be read; refusing to overwrite it";

/// Validates an item key against the host-api `storageKeySchema`
/// (non-empty, ≤256 UTF-16 code units, not reserved).
pub(crate) fn validate_storage_key(key: &str) -> Result<(), PluginError> {
    if key.is_empty() {
        return Err(PluginError::new(
            INVALID_PARAMS,
            "storage key must not be empty",
        ));
    }
    if key.encode_utf16().count() > STORAGE_KEY_MAX_CHARS {
        return Err(PluginError::new(
            INVALID_PARAMS,
            format!("storage key must contain at most {STORAGE_KEY_MAX_CHARS} characters"),
        ));
    }
    if RESERVED_STORAGE_KEYS.contains(&key) {
        return Err(PluginError::new(INVALID_PARAMS, "reserved storage key"));
    }
    Ok(())
}

pub(crate) fn validate_plugin_key(plugin_key: &str) -> Result<(), PluginError> {
    if is_qualified_plugin_key(plugin_key) {
        Ok(())
    } else {
        Err(PluginError::new(
            INVALID_PARAMS,
            format!("unsafe plugin key: {plugin_key}"),
        ))
    }
}

/// The oracle's `isUnreadableError`: denied access is not evidence of
/// corruption, so it must never trigger a rewrite.
fn is_unreadable(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EPERM)
            | Some(libc::EACCES)
            | Some(libc::EBUSY)
            | Some(libc::EMFILE)
            | Some(libc::ENFILE)
            | Some(libc::EIO)
    )
}

/// Shared JSON-document implementation: `storage.json` for plugin data and
/// `settings.json` for plugin-owned settings.
#[derive(Debug, Clone)]
pub(crate) struct JsonFileStore {
    root: PathBuf,
    file_name: &'static str,
}

impl JsonFileStore {
    pub(crate) fn new(root: PathBuf, file_name: &'static str) -> Self {
        Self { root, file_name }
    }

    fn file_path(&self, plugin_key: &str) -> Result<PathBuf, PluginError> {
        validate_plugin_key(plugin_key)?;
        Ok(self.root.join(plugin_key).join(self.file_name))
    }

    /// `None` means the file exists and this process may not read it — which
    /// is never the same as an empty store.
    fn read(&self, plugin_key: &str) -> Result<Option<Map<String, Value>>, PluginError> {
        let path = self.file_path(plugin_key)?;
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            // Missing, or the existence check itself was denied: the oracle's
            // `existsSync` reports `false` and the store starts empty.
            Err(_) => return Ok(Some(Map::new())),
        };
        if metadata.len() > STORAGE_TOTAL_MAX_BYTES as u64 {
            // Oversized stores reset to empty rather than wedging the plugin.
            return Ok(Some(Map::new()));
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if is_unreadable(&error) => return Ok(None),
            Err(_) => return Ok(Some(Map::new())),
        };
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => Ok(Some(map)),
            _ => Ok(Some(Map::new())),
        }
    }

    fn write_bytes(&self, plugin_key: &str, bytes: &[u8]) -> Result<(), PluginError> {
        let path = self.file_path(plugin_key)?;
        let directory = path
            .parent()
            .expect("json store paths always have a parent directory");
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(directory).map_err(|error| {
            PluginError::new(
                ACTION_FAILED,
                format!("failed to create the plugin data directory: {error}"),
            )
        })?;
        write_atomic(&path, bytes, Some(0o600))
    }

    pub(crate) fn get(&self, plugin_key: &str, key: &str) -> Result<Option<Value>, PluginError> {
        validate_storage_key(key)?;
        Ok(self.read(plugin_key)?.and_then(|map| map.get(key).cloned()))
    }

    pub(crate) fn get_all(&self, plugin_key: &str) -> Result<Map<String, Value>, PluginError> {
        Ok(self.read(plugin_key)?.unwrap_or_default())
    }

    pub(crate) fn keys(&self, plugin_key: &str) -> Result<Vec<String>, PluginError> {
        Ok(self
            .read(plugin_key)?
            .map(|map| map.into_iter().map(|(key, _)| key).collect())
            .unwrap_or_default())
    }

    pub(crate) fn set(&self, plugin_key: &str, key: &str, value: Value) -> Result<(), PluginError> {
        validate_storage_key(key)?;
        let serialized = serde_json::to_vec(&value).map_err(|error| {
            PluginError::new(
                ACTION_FAILED,
                format!("value is not JSON-serializable: {error}"),
            )
        })?;
        if serialized.len() > STORAGE_VALUE_MAX_BYTES {
            return Err(PluginError::new(
                ACTION_FAILED,
                format!("value exceeds {STORAGE_VALUE_MAX_BYTES} bytes"),
            ));
        }
        let mut map = self.read(plugin_key)?.ok_or_else(unreadable_store_error)?;
        if !map.contains_key(key) && map.len() >= STORAGE_KEY_LIMIT {
            return Err(PluginError::new(
                ACTION_FAILED,
                format!("storage exceeds the {STORAGE_KEY_LIMIT}-key limit"),
            ));
        }
        map.insert(key.to_string(), value);
        let next_file = serialize_document(&map)?;
        if next_file.len() > STORAGE_TOTAL_MAX_BYTES {
            return Err(PluginError::new(
                ACTION_FAILED,
                format!("storage exceeds {STORAGE_TOTAL_MAX_BYTES} bytes"),
            ));
        }
        self.write_bytes(plugin_key, &next_file)
    }

    pub(crate) fn delete(&self, plugin_key: &str, key: &str) -> Result<(), PluginError> {
        validate_storage_key(key)?;
        let Some(mut map) = self.read(plugin_key)? else {
            // Rewriting what we could not read would drop every other key.
            return Ok(());
        };
        if map.remove(key).is_some() {
            let next_file = serialize_document(&map)?;
            self.write_bytes(plugin_key, &next_file)?;
        }
        Ok(())
    }
}

fn serialize_document(map: &Map<String, Value>) -> Result<Vec<u8>, PluginError> {
    serde_json::to_vec_pretty(map).map_err(|error| {
        PluginError::new(
            ACTION_FAILED,
            format!("failed to serialize the plugin data file: {error}"),
        )
    })
}

fn unreadable_store_error() -> PluginError {
    PluginError::new(ACTION_FAILED, UNREADABLE_STORE_ERROR)
}

/// Per-plugin `storage.json` (0600) key-value store. `data_dir` is the scope's
/// `plugin-data` root; every operation keys by the qualified plugin key.
#[derive(Debug, Clone)]
pub struct StorageStore {
    inner: JsonFileStore,
}

impl StorageStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: JsonFileStore::new(data_dir.into(), STORAGE_FILE_NAME),
        }
    }

    pub fn get(&self, plugin_key: &str, key: &str) -> Result<Option<Value>, PluginError> {
        self.inner.get(plugin_key, key)
    }

    pub fn set(&self, plugin_key: &str, key: &str, value: Value) -> Result<(), PluginError> {
        self.inner.set(plugin_key, key, value)
    }

    pub fn delete(&self, plugin_key: &str, key: &str) -> Result<(), PluginError> {
        self.inner.delete(plugin_key, key)
    }

    pub fn keys(&self, plugin_key: &str) -> Result<Vec<String>, PluginError> {
        self.inner.keys(plugin_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    const KEY: &str = "orca-samples.demo";

    fn store(dir: &tempfile::TempDir) -> StorageStore {
        StorageStore::new(dir.path())
    }

    #[test]
    fn set_get_delete_round_trip_persists_with_mode_0600() {
        let dir = tempdir().unwrap();
        let value = json!({"mode": "dark", "size": [1, 2, 3], "enabled": true});
        {
            let store = store(&dir);
            store.set(KEY, "theme", value.clone()).unwrap();
            assert_eq!(store.get(KEY, "theme").unwrap(), Some(value.clone()));
        }

        let store = store(&dir);
        assert_eq!(store.get(KEY, "theme").unwrap(), Some(value));
        assert_eq!(store.keys(KEY).unwrap(), vec!["theme".to_string()]);
        let path = dir.path().join(KEY).join(STORAGE_FILE_NAME);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        store.delete(KEY, "theme").unwrap();
        assert_eq!(store.get(KEY, "theme").unwrap(), None);
        store.delete(KEY, "theme").unwrap();
        assert_eq!(store.keys(KEY).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn isolates_plugin_namespaces() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        store.set("acme.first", "token", json!("a")).unwrap();
        assert_eq!(store.get("acme.first", "token").unwrap(), Some(json!("a")));
        assert_eq!(store.get("acme.second", "token").unwrap(), None);
        assert!(dir
            .path()
            .join("acme.first")
            .join(STORAGE_FILE_NAME)
            .exists());
        assert!(!dir
            .path()
            .join("acme.second")
            .join(STORAGE_FILE_NAME)
            .exists());
    }

    #[test]
    fn rejects_reserved_and_malformed_keys() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        for reserved in RESERVED_STORAGE_KEYS {
            let error = store.set(KEY, reserved, json!(1)).unwrap_err();
            assert_eq!(error.code(), INVALID_PARAMS);
            assert_eq!(error.message(), "reserved storage key");
            assert_eq!(store.get(KEY, reserved).unwrap_err().code(), INVALID_PARAMS);
            assert_eq!(
                store.delete(KEY, reserved).unwrap_err().code(),
                INVALID_PARAMS
            );
        }
        assert_eq!(
            store.set(KEY, "", json!(1)).unwrap_err().code(),
            INVALID_PARAMS
        );
        let too_long = "k".repeat(STORAGE_KEY_MAX_CHARS + 1);
        assert_eq!(
            store.set(KEY, &too_long, json!(1)).unwrap_err().code(),
            INVALID_PARAMS
        );
        let max = "k".repeat(STORAGE_KEY_MAX_CHARS);
        store.set(KEY, &max, json!(1)).unwrap();
        assert_eq!(store.get(KEY, &max).unwrap(), Some(json!(1)));
    }

    #[test]
    fn rejects_unsafe_plugin_keys() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        for unsafe_key in [
            "demo",
            "a.b.c",
            "constructor.demo",
            "acme.constructor",
            "../x",
        ] {
            let error = store.get(unsafe_key, "k").unwrap_err();
            assert_eq!(error.code(), INVALID_PARAMS);
            assert!(
                error.message().contains("unsafe plugin key"),
                "unexpected message: {}",
                error.message()
            );
        }
    }

    #[test]
    fn enforces_value_cap_in_utf8_bytes_without_persisting() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        let value = "😀".repeat(STORAGE_VALUE_MAX_BYTES / 4 + 1);
        let error = store.set(KEY, "large", json!(value)).unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert_eq!(
            error.message(),
            format!("value exceeds {STORAGE_VALUE_MAX_BYTES} bytes")
        );
        assert_eq!(store.get(KEY, "large").unwrap(), None);
    }

    #[test]
    fn enforces_key_limit_but_allows_updates_at_the_limit() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        for index in 0..STORAGE_KEY_LIMIT {
            store.set(KEY, &format!("k{index}"), json!(index)).unwrap();
        }
        let error = store.set(KEY, "overflow", json!(1)).unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert_eq!(
            error.message(),
            format!("storage exceeds the {STORAGE_KEY_LIMIT}-key limit")
        );
        assert_eq!(store.get(KEY, "overflow").unwrap(), None);
        store.set(KEY, "k0", json!(42)).unwrap();
        assert_eq!(store.get(KEY, "k0").unwrap(), Some(json!(42)));
    }

    #[test]
    fn enforces_total_size_cap_without_clobbering_the_store() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        let plugin_dir = dir.path().join(KEY);
        fs::create_dir_all(&plugin_dir).unwrap();
        let existing = format!(r#"{{"big":"{}"}}"#, "a".repeat(4_990_000));
        fs::write(plugin_dir.join(STORAGE_FILE_NAME), &existing).unwrap();

        let error = store
            .set(KEY, "second", json!("b".repeat(262_000)))
            .unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert_eq!(
            error.message(),
            format!("storage exceeds {STORAGE_TOTAL_MAX_BYTES} bytes")
        );
        assert_eq!(
            fs::read_to_string(plugin_dir.join(STORAGE_FILE_NAME)).unwrap(),
            existing
        );
    }

    #[test]
    fn corrupt_or_oversized_files_read_empty_and_writes_recover() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        let path = dir.path().join(KEY).join(STORAGE_FILE_NAME);
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        fs::write(&path, b"{not json").unwrap();
        assert_eq!(store.get(KEY, "k").unwrap(), None);
        assert_eq!(store.keys(KEY).unwrap(), Vec::<String>::new());
        store.set(KEY, "k", json!("v")).unwrap();
        let parsed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(parsed, json!({"k": "v"}));

        fs::write(&path, vec![b'x'; STORAGE_TOTAL_MAX_BYTES + 1]).unwrap();
        assert_eq!(store.get(KEY, "k").unwrap(), None);
        store.set(KEY, "k", json!("v")).unwrap();
        assert_eq!(store.get(KEY, "k").unwrap(), Some(json!("v")));
    }

    #[test]
    fn unreadable_store_refuses_overwrites() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = tempdir().unwrap();
        let store = store(&dir);
        store.set(KEY, "seed", json!(1)).unwrap();
        let path = dir.path().join(KEY).join(STORAGE_FILE_NAME);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

        let error = store.set(KEY, "other", json!(2)).unwrap_err();
        assert_eq!(error.code(), ACTION_FAILED);
        assert_eq!(error.message(), UNREADABLE_STORE_ERROR);
        assert_eq!(store.get(KEY, "seed").unwrap(), None);
        store.delete(KEY, "seed").unwrap();

        let leftovers: Vec<String> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn classifies_unreadable_io_errors_like_the_oracle() {
        for code in [
            libc::EPERM,
            libc::EACCES,
            libc::EBUSY,
            libc::EMFILE,
            libc::ENFILE,
            libc::EIO,
        ] {
            assert!(is_unreadable(&std::io::Error::from_raw_os_error(code)));
        }
        assert!(!is_unreadable(&std::io::Error::from_raw_os_error(
            libc::ENOENT
        )));
        assert!(!is_unreadable(&std::io::Error::from_raw_os_error(
            libc::EISDIR
        )));
    }

    #[test]
    fn missing_store_reads_as_empty_without_creating_files() {
        let dir = tempdir().unwrap();
        let store = store(&dir);
        assert_eq!(store.get(KEY, "k").unwrap(), None);
        assert_eq!(store.keys(KEY).unwrap(), Vec::<String>::new());
        store.delete(KEY, "k").unwrap();
        assert!(!dir.path().join(KEY).exists());
    }
}
