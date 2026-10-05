//! SQLite-backed key/value store for durable renderer session state (spec §3.1).
//!
//! Single connection behind a Mutex; the session domain is the only writer, so
//! there is no contention. Schema evolution is `PRAGMA user_version` plus an
//! ordered migration list — 2B/2D append to `MIGRATIONS`, never rewrite history.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::StoreError;

type MigrationFn = fn(&Connection) -> rusqlite::Result<()>;

/// Ordered schema history. `IF NOT EXISTS` keeps re-runs idempotent so a
/// user_version that fell behind can never wedge startup.
const MIGRATIONS: &[(i64, MigrationFn)] = &[(1, migrate_v1)];

fn migrate_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspace_sessions (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );",
    )
}

fn apply_migrations(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for &(version, migrate) in MIGRATIONS {
        if version <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        migrate(conn)?;
        conn.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

fn open_fresh(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(5_000))?;
    // journal_mode answers with the new mode; query_row consumes the row.
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    apply_migrations(&conn)?;
    Ok(conn)
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn sidecar_paths(path: &Path) -> [PathBuf; 2] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shm = path.as_os_str().to_os_string();
    shm.push("-shm");
    [PathBuf::from(wal), PathBuf::from(shm)]
}

/// Quarantine target `<original>.corrupt-<stamp>`. The suffix is appended to
/// the full original name (never `with_extension`, which would replace the
/// last extension and collide the main DB with its WAL/SHM sidecars onto one
/// path), so all three quarantine to distinct files (spec §6).
fn quarantined_path(path: &Path, stamp: i64) -> PathBuf {
    let mut quarantined = path.as_os_str().to_os_string();
    quarantined.push(format!(".corrupt-{stamp}"));
    PathBuf::from(quarantined)
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (or rebuild after corruption) the store at `path` and apply pending
    /// migrations. A corrupt database is renamed aside (`<name>.corrupt-<ms>`)
    /// and recreated empty so one bad file cannot wedge startup (spec §6).
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match open_fresh(path) {
            Ok(conn) => Ok(Self {
                conn: Mutex::new(conn),
            }),
            Err(open_error) => {
                let stamp = unix_ms();
                for sidecar in sidecar_paths(path) {
                    let _ = std::fs::rename(&sidecar, quarantined_path(&sidecar, stamp));
                }
                let quarantined = quarantined_path(path, stamp);
                let _ = std::fs::rename(path, &quarantined);
                eprintln!(
                    "[ade-store] sqlite open failed ({open_error}); quarantined to {} and rebuilt",
                    quarantined.display()
                );
                let conn = open_fresh(path).map_err(|retry| {
                    StoreError::InvalidInput(format!(
                        "sqlite rebuild failed: open={open_error} retry={retry}"
                    ))
                })?;
                Ok(Self {
                    conn: Mutex::new(conn),
                })
            }
        }
    }

    pub fn put(&self, key: &str, value: &str) -> Result<(), StoreError> {
        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO workspace_sessions(key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![key, value, unix_ms()],
        )?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare("SELECT value FROM workspace_sessions WHERE key = ?1")?;
        let mut rows = stmt.query(rusqlite::params![key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// All `(key, value)` rows ordered by key, so `session_get` assembly is
    /// deterministic.
    pub fn all(&self) -> Result<Vec<(String, String)>, StoreError> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare("SELECT key, value FROM workspace_sessions ORDER BY key")?;
        let rows =
            stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Whole-state replace (contract `set`): upsert every entry, delete keys
    /// absent from `entries`.
    pub fn replace_all(&self, entries: &[(String, String)]) -> Result<(), StoreError> {
        let conn = self.lock_conn();
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM workspace_sessions", [])?;
        {
            let mut stmt = tx
                .prepare("INSERT INTO workspace_sessions(key, value, updated_at) VALUES (?1, ?2, ?3)")?;
            let now = unix_ms();
            for (key, value) in entries {
                stmt.execute(rusqlite::params![key, value, now])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Fold the WAL back into the main file without blocking readers (contract
    /// `flush`); durability already holds via WAL commits.
    pub fn checkpoint_passive(&self) -> Result<(), StoreError> {
        let conn = self.lock_conn();
        let _busy: i64 = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| row.get(0))?;
        Ok(())
    }

    /// Full WAL fold on app exit (`RunEvent::Exit`); blocks until readers drain.
    pub fn checkpoint_truncate(&self) -> Result<(), StoreError> {
        let conn = self.lock_conn();
        let _busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
        Ok(())
    }

    fn lock_conn(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_applies_v1_and_persists_across_reopen() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("ade.sqlite");
        {
            let store = Store::open(&db).unwrap();
            store.put("tabsByWorktree", r#"{"w1":[]}"#).unwrap();
        }
        let store = Store::open(&db).unwrap();
        assert_eq!(store.get("tabsByWorktree").unwrap().as_deref(), Some(r#"{"w1":[]}"#));
        let conn = Connection::open(&db).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn put_upserts_by_key() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("ade.sqlite")).unwrap();
        store.put("k", r#"{"v":1}"#).unwrap();
        store.put("k", r#"{"v":2}"#).unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some(r#"{"v":2}"#));
    }

    #[test]
    fn get_missing_key_returns_none() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("ade.sqlite")).unwrap();
        assert_eq!(store.get("nope").unwrap(), None);
    }

    #[test]
    fn all_returns_rows_ordered_by_key() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("ade.sqlite")).unwrap();
        store.put("terminalLayoutsByTabId", r#"{}"#).unwrap();
        store.put("tabsByWorktree", r#"{}"#).unwrap();
        store.put("activeTabId", r#"null"#).unwrap();
        let keys: Vec<String> = store.all().unwrap().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["activeTabId", "tabsByWorktree", "terminalLayoutsByTabId"]);
    }

    #[test]
    fn replace_all_deletes_keys_absent_from_payload() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("ade.sqlite")).unwrap();
        store.put("keep", r#"1"#).unwrap();
        store.put("drop", r#"2"#).unwrap();
        store
            .replace_all(&[("fresh".to_string(), r#"true"#.to_string()), ("keep".to_string(), r#"9"#.to_string())])
            .unwrap();
        let entries = store.all().unwrap();
        assert_eq!(
            entries,
            vec![
                ("fresh".to_string(), "true".to_string()),
                ("keep".to_string(), "9".to_string())
            ]
        );
    }

    #[test]
    fn v0_database_with_existing_v1_table_migrates_idempotently() {
        // 升级演练：手工构造 user_version=0 但表已存在的库（模拟历史半程状态），
        // open 必须安全重放 v1（IF NOT EXISTS）且数据完好。
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("ade.sqlite");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE workspace_sessions (
                    key        TEXT PRIMARY KEY,
                    value      TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                INSERT INTO workspace_sessions VALUES ('legacy', '{}', 1);
                PRAGMA user_version = 0;",
            )
            .unwrap();
        }
        let store = Store::open(&db).unwrap();
        assert_eq!(store.get("legacy").unwrap().as_deref(), Some("{}"));
    }

    #[test]
    fn corrupt_database_is_quarantined_and_rebuilt() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("ade.sqlite");
        std::fs::write(&db, "this is not sqlite").unwrap();
        let store = Store::open(&db).unwrap();
        store.put("after", r#"rebuild"#).unwrap();
        assert_eq!(store.get("after").unwrap().as_deref(), Some("rebuild"));
        let quarantined: Vec<PathBuf> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("corrupt-"))
            })
            .collect();
        assert_eq!(quarantined.len(), 1, "exactly one quarantined file expected");
        let quarantined = &quarantined[0];
        assert!(
            quarantined
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ade.sqlite.corrupt-"),
            "quarantine must append the suffix to the full name: {}",
            quarantined.display()
        );
        let preserved = std::fs::read_to_string(quarantined).unwrap();
        assert_eq!(
            preserved, "this is not sqlite",
            "corrupt bytes must be preserved aside, not overwritten"
        );
        let rebuilt = Store::open(&db).unwrap();
        assert_eq!(rebuilt.get("after").unwrap().as_deref(), Some("rebuild"));
    }

    #[test]
    fn checkpoints_return_ok() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("ade.sqlite")).unwrap();
        store.put("k", "v").unwrap();
        store.checkpoint_passive().unwrap();
        store.checkpoint_truncate().unwrap();
    }
}
