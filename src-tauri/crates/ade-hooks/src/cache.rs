use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LAST_STATUS_FILE_VERSION: u32 = 1;
pub const STATUS_PERSIST_DEBOUNCE: Duration = Duration::from_millis(250);
pub const HYDRATE_MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedHookEvent {
    pub source: String,
    pub payload: Value,
    pub pane_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_token: Option<String>,
    pub received_at: i64,
    /// 仅内存标记：hydrate 后为 true，渲染层据此补 `restoredUnconfirmed`；
    /// 从不落盘（oracle 同语义，规格 §3.6）。
    #[serde(skip_serializing_if = "is_false", default)]
    pub restored: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Serialize, Deserialize)]
struct StatusFile {
    version: u32,
    entries: HashMap<String, CachedHookEvent>,
}

pub struct StatusCache {
    path: PathBuf,
    debounce: Duration,
    inner: Arc<Mutex<CacheInner>>,
    wake: Arc<Condvar>,
    stop: Arc<AtomicBool>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

struct CacheInner {
    entries: HashMap<String, CachedHookEvent>,
    dirty: bool,
    /// 最近一次 record 的时刻；防抖到期才落盘，且等待切成 ≤50ms 片，
    /// 让 shutdown 的 join 不会被长 debounce（测试/未来参数）挂住。
    last_record_at: Option<std::time::Instant>,
}

fn start_writer(cache: &StatusCache) {
    let path = cache.path.clone();
    let debounce = cache.debounce;
    let inner = Arc::clone(&cache.inner);
    let wake = Arc::clone(&cache.wake);
    let stop = Arc::clone(&cache.stop);
    let handle = std::thread::spawn(move || loop {
        let mut guard = inner.lock().unwrap();
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            if guard.dirty {
                write_status_file(&path, &guard.entries);
            }
            return;
        }
        match guard.last_record_at {
            Some(recorded_at) => {
                let elapsed = recorded_at.elapsed();
                if elapsed >= debounce {
                    write_status_file(&path, &guard.entries);
                    guard.dirty = false;
                    guard.last_record_at = None;
                } else {
                    let wait = (debounce - elapsed).min(Duration::from_millis(50));
                    let _ = wake.wait_timeout(guard, wait).unwrap();
                }
            }
            None => {
                let _ = wake.wait_timeout(guard, Duration::from_millis(50)).unwrap();
            }
        }
    });
    *cache.writer.lock().unwrap() = Some(handle);
}

impl StatusCache {
    pub fn load(path: PathBuf) -> Self {
        Self::load_with_debounce(path, STATUS_PERSIST_DEBOUNCE)
    }

    pub fn load_with_debounce(path: PathBuf, debounce: Duration) -> Self {
        let entries = hydrate_entries(&path);
        let cache = Self {
            path,
            debounce,
            inner: Arc::new(Mutex::new(CacheInner {
                entries,
                dirty: false,
                last_record_at: None,
            })),
            wake: Arc::new(Condvar::new()),
            stop: Arc::new(AtomicBool::new(false)),
            writer: Mutex::new(None),
        };
        start_writer(&cache);
        cache
    }

    pub fn record(&self, event: CachedHookEvent) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.entries.insert(event.pane_key.clone(), event);
            inner.dirty = true;
            inner.last_record_at = Some(std::time::Instant::now());
        }
        self.wake.notify_all();
    }

    pub fn snapshot(&self) -> Vec<CachedHookEvent> {
        let inner = self.inner.lock().unwrap();
        let mut entries: Vec<CachedHookEvent> = inner.entries.values().cloned().collect();
        entries.sort_by_key(|entry| entry.received_at);
        entries
    }

    pub fn flush_sync(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.dirty {
            write_status_file(&self.path, &inner.entries);
            inner.dirty = false;
            inner.last_record_at = None;
        }
    }

    pub fn shutdown(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.wake.notify_all();
        let handle = self.writer.lock().unwrap().take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
        self.flush_sync();
    }
}

fn hydrate_entries(path: &PathBuf) -> HashMap<String, CachedHookEvent> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(file) = serde_json::from_str::<StatusFile>(&raw) else {
        return HashMap::new();
    };
    if file.version != LAST_STATUS_FILE_VERSION {
        return HashMap::new();
    }
    let now = now_ms();
    file.entries
        .into_values()
        .filter(|entry| entry.received_at > 0 && now - entry.received_at <= HYDRATE_MAX_AGE_MS)
        .map(|mut entry| {
            entry.restored = true;
            (entry.pane_key.clone(), entry)
        })
        .collect()
}

fn write_status_file(path: &PathBuf, entries: &HashMap<String, CachedHookEvent>) {
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let Ok(serialized) = serde_json::to_string(&StatusFile {
        version: LAST_STATUS_FILE_VERSION,
        entries: entries.clone(),
    }) else {
        return;
    };
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    if std::fs::write(&tmp, serialized.as_bytes()).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        if std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    fn event(pane: &str, received_at: i64) -> CachedHookEvent {
        CachedHookEvent {
            source: "claude".to_string(),
            payload: json!({ "hook_event_name": "Stop" }),
            pane_key: pane.to_string(),
            tab_id: Some("t1".to_string()),
            worktree_id: None,
            launch_token: None,
            received_at,
            restored: false,
        }
    }

    #[test]
    fn flush_sync_writes_versioned_entries_without_restored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last-status.json");
        let cache = StatusCache::load_with_debounce(path.clone(), Duration::from_millis(10));
        cache.record(event("t1:leaf", now_ms()));
        cache.flush_sync();
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["entries"]["t1:leaf"]["source"], "claude");
        assert_eq!(raw["entries"]["t1:leaf"]["paneKey"], "t1:leaf");
        assert!(raw["entries"]["t1:leaf"].get("restored").is_none());
        cache.shutdown();
    }

    #[test]
    fn hydrate_marks_restored_and_drops_stale_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last-status.json");
        let fresh = now_ms() - 1_000;
        let stale = now_ms() - HYDRATE_MAX_AGE_MS - 1_000;
        std::fs::write(
            &path,
            serde_json::to_string(&json!({
                "version": 1,
                "entries": {
                    "t1:fresh": event("t1:fresh", fresh),
                    "t1:stale": event("t1:stale", stale),
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let cache = StatusCache::load(path);
        let snapshot = cache.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].pane_key, "t1:fresh");
        assert!(snapshot[0].restored);
        cache.shutdown();
    }

    #[test]
    fn hydrate_ignores_wrong_version_and_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last-status.json");
        std::fs::write(&path, json!({ "version": 99, "entries": {} }).to_string()).unwrap();
        let cache = StatusCache::load(path.clone());
        assert!(cache.snapshot().is_empty());
        cache.shutdown();
        std::fs::write(&path, "not json").unwrap();
        let cache = StatusCache::load(path);
        assert!(cache.snapshot().is_empty());
        cache.shutdown();
    }

    #[test]
    fn snapshot_sorts_by_received_at_ascending() {
        let dir = tempfile::tempdir().unwrap();
        let cache =
            StatusCache::load_with_debounce(dir.path().join("last-status.json"), Duration::from_secs(60));
        cache.record(event("t1:b", 200));
        cache.record(event("t1:a", 100));
        assert_eq!(
            cache.snapshot().iter().map(|e| e.pane_key.as_str()).collect::<Vec<_>>(),
            vec!["t1:a", "t1:b"]
        );
        cache.shutdown();
    }
}
