pub mod json_file;
pub mod onboarding_store;
pub mod projects_store;
pub mod settings_store;
pub mod ui_state_store;
pub mod worktree_meta_store;

pub use worktree_meta_store::{WorktreeMetaStore, WORKTREE_META_FIELDS};

use serde_json::Value;
use thiserror::Error;

pub const SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Invalid store payload: {0}")]
    InvalidInput(String),
}

pub(crate) fn deep_merge(base: &Value, updates: &Value) -> Value {
    let (Some(base_map), Some(updates_map)) = (base.as_object(), updates.as_object()) else {
        return updates.clone();
    };
    let mut merged = base_map.clone();
    for (key, value) in updates_map {
        let next = match merged.get(key) {
            Some(existing) if existing.is_object() && value.is_object() => {
                deep_merge(existing, value)
            }
            _ => value.clone(),
        };
        merged.insert(key.clone(), next);
    }
    Value::Object(merged)
}

pub(crate) fn shallow_merge(base: &Value, updates: &Value, deep_keys: &[&str]) -> Value {
    let (Some(base_map), Some(updates_map)) = (base.as_object(), updates.as_object()) else {
        return updates.clone();
    };
    let mut merged = base_map.clone();
    for (key, value) in updates_map {
        let is_deep_key = deep_keys.contains(&key.as_str());
        let next = if is_deep_key {
            match merged.get(key) {
                Some(existing) => deep_merge(existing, value),
                None => value.clone(),
            }
        } else {
            value.clone()
        };
        merged.insert(key.clone(), next);
    }
    Value::Object(merged)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        pub fn new(name: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("ade-store-{name}-{}-{unique}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        pub fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
