# Phase 2A：持久化基座 + 终端会话恢复 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 应用重启/reload 后恢复终端 tabs/split/scrollback/标题，休眠 agent 会话自动 `--resume` 重生；`ade-store` 增 SQLite 基座承接工作区会话态。

**Architecture:** 渲染层 serializeAddon 产物内联进 `TerminalLayoutSnapshot.buffersByLeafId`，随 `WorkspaceSessionState` 经 session 域命令落 SQLite（Rust 侧为不透明 JSON 文档存储，顶层字段一行）；退出走 `RunEvent::ExitRequested` → `session:flush-requested` → 渲染层捕获+flush+ack → 宿主 2s 超时放行；providerSession 由 transcript 扫描命令捕获进 `sleepingAgentSessionsByPaneKey`，消费侧复用 fork 冷恢复机械（零改动）。

**Tech Stack:** Rust（rusqlite bundled / tauri-specta / serde_json）、TS（React + zustand + vitest）。

**Spec:** `docs/superpowers/specs/2026-10-01-phase2a-persistence-session-restore-design.md`（含 §0 修订记录 R1/R2/R3——本计划按修订后规格执行）

## Global Constraints

- 门禁：`cargo test --workspace` 全绿；`pnpm test` 全绿；`pnpm typecheck && pnpm build:web` exit 0
- Rust 快测：`cargo test -p ade-store -p ade-bridge`
- 新增 Rust 依赖仅 `rusqlite = { version = "0.32", features = ["bundled"] }`（ade-store 主依赖）；ade-store dev-deps 增 `tempfile = "3"` 与 `filetime = "0.2"`（后者加在 ade-bridge dev-deps）；**不新增任何 npm 依赖**
- bindings 单一登记点：新命令必须同时进 `src-tauri/crates/ade-bridge/src/specta_export.rs` 的 `collect_commands!` **和** `export_lists_every_command` 测试清单，然后 `cargo run -p ade-bridge --bin export-bindings` 再生成 `src/bridge/real/generated/tauri-bindings.ts`（`bindings_are_fresh` 测试防漂移）
- 契约文件 `src/shared/preload-api/api/*.ts` 为 fork 形状：**只做加法**，不删除/重命名既有方法；`readTerminalScrollback` 保持返回 null
- xterm 系依赖已 pnpm patch 钉死（`pnpm-workspace.yaml:16-24`），禁止升级
- 命令载荷信封：TS 侧 `invokeCommand(cmd, { args })` ↔ Rust 侧参数名 `args`（见 `src/bridge/real/invoke.ts:23-37` 与 `commands/ui.rs` 先例）
- 提交信息：中文 conventional commits（`feat(store): …`），结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 执行前用 superpowers:using-git-worktrees 建隔离工作区

---

### Task 1: SQLite 基座（ade-store/sqlite.rs）

**Files:**
- Modify: `src-tauri/crates/ade-store/Cargo.toml`
- Modify: `src-tauri/crates/ade-store/src/lib.rs`
- Create: `src-tauri/crates/ade-store/src/sqlite.rs`

**Interfaces:**
- Consumes: 无（新基座）
- Produces: `ade_store::sqlite::Store`，方法签名（后续 Task 2 依赖）：
  - `Store::open(path: &Path) -> Result<Self, StoreError>`
  - `put(&self, key: &str, value: &str) -> Result<(), StoreError>`
  - `get(&self, key: &str) -> Result<Option<String>, StoreError>`
  - `all(&self) -> Result<Vec<(String, String)>, StoreError>`
  - `replace_all(&self, entries: &[(String, String)]) -> Result<(), StoreError>`
  - `checkpoint_passive(&self) -> Result<(), StoreError>` / `checkpoint_truncate(&self) -> Result<(), StoreError>`
  - `StoreError::Rusqlite(rusqlite::Error)` 变体

- [ ] **Step 1: 加依赖**

`src-tauri/crates/ade-store/Cargo.toml` 改为：

```toml
[package]
name = "ade-store"
version = "0.0.1"
edition = "2021"

[dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }
serde_json = "1"
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```

`src-tauri/crates/ade-store/src/lib.rs` 的 `StoreError` 增变体，并注册模块（第 1-21 行区域）：

```rust
pub mod json_file;
pub mod onboarding_store;
pub mod projects_store;
pub mod settings_store;
pub mod sqlite;
pub mod ui_state_store;
pub mod worktree_meta_store;
```

```rust
#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Rusqlite(#[from] rusqlite::Error),
    #[error("Invalid store payload: {0}")]
    InvalidInput(String),
}
```

- [ ] **Step 2: 写失败测试**

创建 `src-tauri/crates/ade-store/src/sqlite.rs`，先只写模块头与测试（实现暂以 `todo!()` 占位编译不过即视为红）：

```rust
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
        let quarantined = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt-"));
        assert!(quarantined, "old corrupt file must be renamed aside");
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
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p ade-store sqlite`
Expected: 编译失败（`Store` 未定义）

- [ ] **Step 4: 最小实现**

在同文件测试上方补实现：

```rust
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
                    let _ = std::fs::rename(
                        &sidecar,
                        sidecar.with_extension(format!("corrupt-{stamp}")),
                    );
                }
                let quarantined = path.with_extension(format!("corrupt-{stamp}"));
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
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p ade-store`
Expected: 全部 PASS（含既有 json_file/store 测试）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/crates/ade-store
git commit -m "feat(store): SQLite 基座——user_version 迁移框架 + workspace_sessions 单表 + 损坏隔离重建"
```

---

### Task 2: session 域命令 + AppState 集成

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（AppState 字段/初始化/退出收尾）
- Create: `src-tauri/crates/ade-bridge/src/commands/session.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Modify: `src-tauri/src/lib.rs`（Exit 分支 checkpoint）
- Modify（再生成）: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: Task 1 的 `ade_store::sqlite::Store` 全部方法
- Produces: 命令 `session_get -> string`、`session_set(args: string)`、`session_patch(args: string)`、`session_flush()`；载荷均为 JSON **文本**（不透明文档存储，spec §3.2）

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/crates/ade-bridge/src/commands/session.rs`，测试先行（核心逻辑提取为自由函数，脱离 Tauri runtime 可测）：

```rust
//! Workspace session state persistence (spec §3.2): an opaque JSON document
//! store. The renderer is the only writer; Rust never mirrors the
//! `WorkspaceSessionState` type — each top-level field is one row.

use ade_store::sqlite::Store;
use serde_json::{Map, Value};

use crate::errors::BridgeError;

fn parse_object(text: &str) -> Result<Vec<(String, Value)>, BridgeError> {
    let parsed = serde_json::from_str::<Value>(text)
        .map_err(|error| BridgeError::message(format!("invalid session payload: {error}")))?;
    match parsed {
        Value::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(BridgeError::message("session payload must be a JSON object")),
    }
}

/// `session_get` core: assemble every row into one JSON object text.
pub(crate) fn assemble_state_text(store: &Store) -> Result<String, BridgeError> {
    let mut object = Map::new();
    for (key, value) in store.all()? {
        // Values are written by us as valid JSON; a stray unreadable row degrades
        // to null instead of failing the whole restore.
        let parsed = serde_json::from_str::<Value>(&value).unwrap_or(Value::Null);
        object.insert(key, parsed);
    }
    Ok(Value::Object(object).to_string())
}

/// `session_patch` core: replace each present top-level key wholesale
/// (`WorkspaceSessionPatch = Partial<WorkspaceSessionState>` semantics).
pub(crate) fn apply_patch(store: &Store, payload: &str) -> Result<(), BridgeError> {
    for (key, value) in parse_object(payload)? {
        store.put(&key, &value.to_string())?;
    }
    Ok(())
}

/// `session_set` core: whole-state replace, deleting keys absent from the payload.
pub(crate) fn apply_set(store: &Store, payload: &str) -> Result<(), BridgeError> {
    let entries = parse_object(payload)?
        .into_iter()
        .map(|(key, value)| (key, value.to_string()))
        .collect::<Vec<_>>();
    store.replace_all(&entries)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &tempfile::TempDir) -> Store {
        Store::open(&dir.path().join("ade.sqlite")).unwrap()
    }

    #[test]
    fn patch_round_trips_through_assemble() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        apply_patch(&store, r#"{"tabsByWorktree":{"w1":[]},"activeTabId":null}"#).unwrap();
        let assembled = assemble_state_text(&store).unwrap();
        let value: Value = serde_json::from_str(&assembled).unwrap();
        assert_eq!(value["tabsByWorktree"]["w1"], Value::Array(vec![]));
        assert_eq!(value["activeTabId"], Value::Null);
    }

    #[test]
    fn patch_replaces_top_level_keys_wholesale() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        apply_patch(&store, r#"{"tabsByWorktree":{"w1":["a"]}}"#).unwrap();
        apply_patch(&store, r#"{"tabsByWorktree":{"w2":["b"]}}"#).unwrap();
        let assembled = assemble_state_text(&store).unwrap();
        let value: Value = serde_json::from_str(&assembled).unwrap();
        // Whole-key replacement: w1 must be gone, not deep-merged.
        assert_eq!(value["tabsByWorktree"]["w2"], serde_json::json!(["b"]));
        assert!(value["tabsByWorktree"].get("w1").is_none());
    }

    #[test]
    fn set_deletes_keys_absent_from_payload_and_tolerates_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        apply_patch(&store, r#"{"keep":1,"drop":2,"futureKey":{}}"#).unwrap();
        apply_set(&store, r#"{"keep":3}"#).unwrap();
        let value: Value = serde_json::from_str(&assemble_state_text(&store).unwrap()).unwrap();
        assert_eq!(value["keep"], 3);
        assert!(value.get("drop").is_none());
        assert!(value.get("futureKey").is_none());
    }

    #[test]
    fn non_object_payloads_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        assert!(apply_patch(&store, "[]").is_err());
        assert!(apply_patch(&store, "not json").is_err());
        assert!(apply_set(&store, "42").is_err());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p ade-bridge commands::session`
Expected: 编译失败（模块未注册到 `commands/mod.rs` 之前）——先在 `commands/mod.rs` 的 `pub mod` 列表（按字母序）加一行使测试可达：

```rust
pub mod session;
```

再次运行，Expected: 编译通过、4 个测试 PASS（此步实现已在 Step 1 给出；若坚持先红后绿，可先删掉三个 `pub(crate) fn` 的函数体以 `todo!()` 代替确认测试失败，再还原）。

- [ ] **Step 3: AppState 集成 + 命令导出**

`state.rs`：
1. import 区加 `use ade_store::sqlite::Store;`
2. `PersistedState` 不动；`AppState` 增字段（`pub pty_worktree_ids: PtyWorktreeIds,` 之后）：

```rust
    /// SQLite 会话态存储（规格 §3.1；Task 2）。损坏由 `Store::open` 隔离重建。
    pub session: Arc<Store>,
```

3. `AppState::initialize` 中 `std::fs::create_dir_all(&data_dir)?;` 之后加：

```rust
        let session = Arc::new(Store::open(&data_dir.join("ade.sqlite"))?);
```

并在 `Ok(Self { ... })` 构造里加 `session,`。

4. accessor（`ui_store()` 附近）：

```rust
    /// Shared handle to the SQLite session store (spec §3.1).
    pub fn session_store(&self) -> &Store {
        &self.session
    }
```

同文件追加命令包装（供 tauri command 使用，含可测的核心已抽出）：

```rust
// commands/session.rs 追加（文件末尾）：
use tauri::State;

use crate::state::AppState;

/// Read the full workspace session state as one JSON object text.
#[tauri::command]
#[specta::specta]
pub async fn session_get(state: State<'_, AppState>) -> Result<String, BridgeError> {
    assemble_state_text(state.session_store())
}

/// Replace each present top-level key wholesale (opaque JSON payload).
#[tauri::command]
#[specta::specta]
pub async fn session_patch(state: State<'_, AppState>, args: String) -> Result<(), BridgeError> {
    apply_patch(state.session_store(), &args)
}

/// Whole-state replace; keys absent from the payload are deleted.
#[tauri::command]
#[specta::specta]
pub async fn session_set(state: State<'_, AppState>, args: String) -> Result<(), BridgeError> {
    apply_set(state.session_store(), &args)
}

/// Explicit flush point; WAL commits already hold durability, so this only
/// folds the WAL (spec §3.2).
#[tauri::command]
#[specta::specta]
pub async fn session_flush(state: State<'_, AppState>) -> Result<(), BridgeError> {
    state.session_store().checkpoint_passive()
}
```

注意：上述四个命令放 `commands/session.rs` 内（`parse_object`/`assemble_state_text` 等同文件）；`state.rs` 只加字段与 accessor。

`specta_export.rs`：
1. `collect_commands!` 里 `commands::settings::settings_set,` 之后加：

```rust
                commands::session::session_get,
                commands::session::session_set,
                commands::session::session_patch,
                commands::session::session_flush,
```

2. `export_lists_every_command` 测试清单在 `"settings_set",` 之后加：

```rust
            "session_get",
            "session_set",
            "session_patch",
            "session_flush",
```

`src-tauri/src/lib.rs` 的 `RunEvent::Exit` 分支，`state.flush_pending_writes();` 之后加：

```rust
                if let Err(error) = state.session_store().checkpoint_truncate() {
                    eprintln!("[ade] failed to checkpoint session store on exit: {error}");
                }
```

- [ ] **Step 4: 再生成 bindings 并跑全量 Rust 测试**

Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge -p ade-store`
Expected: bindings 再生成成功；`bindings_are_fresh`、`export_lists_every_command`、session 模块 4 测试全 PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): session 域命令——不透明 JSON 文档存储（get/set/patch/flush）+ SQLite 接入 AppState"
```

---

### Task 3: transcript 扫描命令（agent_sessions_resolve_capture）

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/agent_sessions.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`、`src-tauri/crates/ade-bridge/src/specta_export.rs`、`src-tauri/crates/ade-bridge/Cargo.toml`（dev-dep filetime）
- Modify（再生成）: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: `AppState.home: String`、`crate::run_blocking`
- Produces: 命令 `agent_sessions_resolve_capture(args: {cwd, agentKind, windowFromMs, windowToMs}) -> AgentProviderSessionMetadata | null`，specta 类型 `AgentProviderSessionMetadata { key: 'session_id' | 'conversation_id', id: string, transcriptPath?: string }`（Task 8 的 TS 侧经 `preflight.resolveAgentProviderSession` 消费）

- [ ] **Step 1: 加 dev-dep**

`src-tauri/crates/ade-bridge/Cargo.toml` 的 `[dev-dependencies]` 加：

```toml
filetime = "0.2"
```

- [ ] **Step 2: 写失败测试**

创建 `commands/agent_sessions.rs`：

```rust
//! providerSession capture via transcript-directory scan (spec §3.3).
//!
//! 2A supports the two agents the PATH probe knows (`claude`, `codex`); any
//! other `agentKind` returns null. A miss degrades to a plain shell restore —
//! never an error.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::errors::BridgeError;
use crate::state::AppState;

const SCAN_TIME_BUDGET: Duration = Duration::from_millis(500);
const CODEX_WALK_MAX_DEPTH: u8 = 4;
const CODEX_HEADER_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderSessionKey {
    SessionId,
    ConversationId,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderSessionMetadata {
    pub key: AgentProviderSessionKey,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionsResolveCaptureArgs {
    pub cwd: String,
    pub agent_kind: String,
    pub window_from_ms: i64,
    pub window_to_ms: i64,
}

pub(crate) type CaptureWindow = (i64, i64);

/// Claude Code stores transcripts in `~/.claude/projects/<munged-cwd>/`, the
/// munge replacing every path separator with `-` (`/Users/a/b` → `-Users-a-b`).
pub(crate) fn munge_claude_project_dir_name(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect()
}

fn modified_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis() as i64,
    )
}

fn in_window(mtime: i64, window: CaptureWindow) -> bool {
    mtime >= window.0 && mtime <= window.1
}

fn latest_jsonl_in_window(
    dir: &Path,
    window: CaptureWindow,
    started: &Instant,
) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(i64, PathBuf)> = None;
    for entry in entries.flatten() {
        if started.elapsed() >= SCAN_TIME_BUDGET {
            break;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(mtime) = modified_ms(&path) else { continue };
        if !in_window(mtime, window) {
            continue;
        }
        if best.as_ref().map_or(true, |(best_mtime, _)| mtime >= *best_mtime) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, path)| path)
}

fn codex_rollout_mentions_cwd(path: &Path, cwd: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut head = vec![0u8; CODEX_HEADER_BYTES];
    let Ok(read) = file.read(&mut head) else {
        return false;
    };
    head.truncate(read);
    String::from_utf8_lossy(&head).contains(cwd)
}

fn latest_codex_rollout_in_window(
    root: &Path,
    cwd: &str,
    window: CaptureWindow,
    started: &Instant,
) -> Option<PathBuf> {
    let mut best: Option<(i64, PathBuf)> = None;
    let mut stack = vec![(root.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if started.elapsed() >= SCAN_TIME_BUDGET {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if started.elapsed() >= SCAN_TIME_BUDGET {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                if depth < CODEX_WALK_MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let is_rollout = path
                .file_name()
                .and_then(|name| name.to_str())
                .map_or(false, |name| {
                    name.starts_with("rollout-") && name.ends_with(".jsonl")
                });
            if !is_rollout {
                continue;
            }
            let Some(mtime) = modified_ms(&path) else { continue };
            if !in_window(mtime, window) || !codex_rollout_mentions_cwd(&path, cwd) {
                continue;
            }
            if best.as_ref().map_or(true, |(best_mtime, _)| mtime >= *best_mtime) {
                best = Some((mtime, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// Pure capture core (testable without a home directory): newest transcript in
/// the window whose location matches `agentKind`'s on-disk convention.
pub(crate) fn resolve_capture(
    home: &Path,
    cwd: &str,
    agent_kind: &str,
    window: CaptureWindow,
) -> Option<AgentProviderSessionMetadata> {
    if window.1 < window.0 || cwd.trim().is_empty() {
        return None;
    }
    let started = Instant::now();
    match agent_kind {
        "claude" => {
            let dir = home
                .join(".claude")
                .join("projects")
                .join(munge_claude_project_dir_name(cwd));
            let transcript = latest_jsonl_in_window(&dir, window, &started)?;
            let id = transcript.file_stem()?.to_str()?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            Some(AgentProviderSessionMetadata {
                key: AgentProviderSessionKey::SessionId,
                id,
                transcript_path: Some(transcript.to_string_lossy().into_owned()),
            })
        }
        "codex" => {
            // Observed layout: ~/.codex/sessions/YYYY/MM/DD/rollout-<local-ts>-<uuid>.jsonl.
            // The CLI resume id is the trailing uuid segment (loose by design —
            // spec §8.3; a miss degrades to shell-only restore).
            let sessions = home.join(".codex").join("sessions");
            let transcript = latest_codex_rollout_in_window(&sessions, cwd, window, &started)?;
            let stem = transcript.file_stem()?.to_str()?;
            let id = stem.rsplit('-').next()?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            Some(AgentProviderSessionMetadata {
                key: AgentProviderSessionKey::SessionId,
                id,
                transcript_path: Some(transcript.to_string_lossy().into_owned()),
            })
        }
        _ => None,
    }
}

/// Transcript scan runs off the async runtime (directory walks can stall on
/// network volumes); the 500ms budget bounds the worst case.
#[tauri::command]
#[specta::specta]
pub async fn agent_sessions_resolve_capture(
    state: State<'_, AppState>,
    args: AgentSessionsResolveCaptureArgs,
) -> Result<Option<AgentProviderSessionMetadata>, BridgeError> {
    let home = state.home.clone();
    crate::run_blocking(move || {
        Ok(resolve_capture(
            Path::new(&home),
            &args.cwd,
            &args.agent_kind,
            (args.window_from_ms, args.window_to_ms),
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    fn write_with_mtime(path: &Path, contents: &str, mtime_unix: i64) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
        let time = filetime::FileTime::from_unix_time(mtime_unix, 0);
        filetime::set_file_mtime(path, time).unwrap();
    }

    const WINDOW: CaptureWindow = (1_000, 2_000);

    #[test]
    fn munge_replaces_separators_with_dashes() {
        assert_eq!(munge_claude_project_dir_name("/Users/a/b"), "-Users-a-b");
        assert_eq!(munge_claude_project_dir_name("C:\\Users\\x"), "C-Users-x");
    }

    #[test]
    fn claude_scan_picks_newest_transcript_in_window() {
        let home = tempfile::tempdir().unwrap();
        let dir = home
            .path()
            .join(".claude/projects/-Users-a-b");
        write_with_mtime(&dir.join("old.jsonl"), "{}", 1_100);
        write_with_mtime(&dir.join("new.jsonl"), "{}", 1_500);
        write_with_mtime(&dir.join("too-new.jsonl"), "{}", 9_999);
        write_with_mtime(&dir.join("too-old.jsonl"), "{}", 5);

        let found = resolve_capture(home.path(), "/Users/a/b", "claude", WINDOW).unwrap();
        assert_eq!(found.key, AgentProviderSessionKey::SessionId);
        assert_eq!(found.id, "new");
        assert!(found.transcript_path.unwrap().ends_with("new.jsonl"));
    }

    #[test]
    fn claude_scan_returns_none_when_dir_or_window_misses() {
        let home = tempfile::tempdir().unwrap();
        assert!(resolve_capture(home.path(), "/Users/other", "claude", WINDOW).is_none());
        let dir = home.path().join(".claude/projects/-Users-a-b");
        write_with_mtime(&dir.join("a.jsonl"), "{}", 9_999);
        assert!(resolve_capture(home.path(), "/Users/a/b", "claude", WINDOW).is_none());
    }

    #[test]
    fn codex_scan_walks_date_dirs_and_requires_cwd_match() {
        let home = tempfile::tempdir().unwrap();
        let base = home.path().join(".codex/sessions/2026/10/01");
        write_with_mtime(
            &base.join("rollout-2026-10-01T10-00-00-aaaaaaaa-1111-2222-3333-444444444444.jsonl"),
            r#"{"cwd":"/repo/one"}"#,
            1_200,
        );
        // Newer rollout for a different cwd must not win.
        write_with_mtime(
            &base.join("rollout-2026-10-01T11-00-00-bbbbbbbb-1111-2222-3333-444444444444.jsonl"),
            r#"{"cwd":"/repo/two"}"#,
            1_800,
        );

        let found = resolve_capture(home.path(), "/repo/one", "codex", WINDOW).unwrap();
        assert_eq!(found.id, "444444444444");
        assert!(found.transcript_path.unwrap().contains("aaaaaaaa"));
    }

    #[test]
    fn unknown_agent_kind_and_degenerate_windows_return_none() {
        let home = tempfile::tempdir().unwrap();
        assert!(resolve_capture(home.path(), "/repo", "gemini", WINDOW).is_none());
        assert!(resolve_capture(home.path(), "/repo", "claude", (2_000, 1_000)).is_none());
        assert!(resolve_capture(home.path(), "  ", "claude", WINDOW).is_none());
    }

    #[test]
    fn scan_budget_bounds_the_walk() {
        let home = tempfile::tempdir().unwrap();
        // A fifo-style unreadable path would hang a naive walk; here we only
        // assert a normal empty scan returns quickly and misses.
        let started = Instant::now();
        assert!(resolve_capture(home.path(), "/repo", "codex", WINDOW).is_none());
        assert!(started.elapsed() < StdDuration::from_secs(2));
    }
}
```

- [ ] **Step 3: 注册模块与命令**

`commands/mod.rs` 加 `pub mod agent_sessions;`（`pub mod app;` 之前，保持字母序）。

`specta_export.rs`：
1. `collect_commands!` 里 `commands::preflight::preflight_refresh_agents,` 之后加：

```rust
                commands::agent_sessions::agent_sessions_resolve_capture,
```

2. `export_lists_every_command` 清单 `"preflight_refresh_agents",` 之后加：

```rust
            "agent_sessions_resolve_capture",
```

- [ ] **Step 4: 跑测试、再生成 bindings**

Run: `cargo test -p ade-bridge commands::agent_sessions && cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`
Expected: agent_sessions 6 测试 PASS；bindings 新鲜度 PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): agent_sessions_resolve_capture——claude/codex transcript 扫描捕获 providerSession"
```

---

### Task 4: 退出 flush 协议（Rust 侧）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/events.rs`
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（flush 信号原语 + 编排方法）
- Modify: `src-tauri/crates/ade-bridge/src/commands/session.rs`（ack 命令）
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Modify: `src-tauri/src/lib.rs`（ExitRequested 分支）
- Modify（再生成）: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: Task 2 的 `AppState.session`
- Produces: 事件 `session:flush-requested`（Task 5 订阅）；命令 `session_flush_ack()`（Task 5 调用）；`AppState::flush_session_then_exit()`

- [ ] **Step 1: 写失败测试**

`state.rs` 测试模块追加（信号原语提取为自由函数以便无 AppHandle 测试）：

```rust
    #[test]
    fn session_flush_signal_arms_resets_and_acks() {
        let slot: Mutex<Option<SessionFlushSignal>> = Mutex::new(None);
        let signal = arm_session_flush_signal(&slot);
        {
            let (lock, _) = &*signal;
            assert!(!*lock.lock().unwrap());
        }
        signal_session_flush_ack(&slot);
        {
            let (lock, _) = &*signal;
            assert!(*lock.lock().unwrap());
        }
        // Re-arm resets the flag (a second window-close quit must wait afresh).
        let rearmed = arm_session_flush_signal(&slot);
        {
            let (lock, _) = &*rearmed;
            assert!(!*lock.lock().unwrap());
        }
    }

    #[test]
    fn session_flush_ack_wakes_a_waiter_before_timeout() {
        let slot: std::sync::Arc<Mutex<Option<SessionFlushSignal>>> =
            std::sync::Arc::new(Mutex::new(None));
        let signal = arm_session_flush_signal(&slot);
        let waiter_slot = std::sync::Arc::clone(&slot);
        let handle = thread::spawn(move || {
            let slot_ref = &*waiter_slot;
            let signal = lock(slot_ref).clone().expect("armed");
            let (lock, cvar) = &*signal;
            let guard = lock.lock().unwrap();
            let (_, acked) = cvar
                .wait_timeout_for(guard, Duration::from_millis(2_000))
                .unwrap();
            acked
        });
        thread::sleep(Duration::from_millis(50));
        signal_session_flush_ack(&slot);
        assert!(handle.join().unwrap(), "ack must arrive well before the 2s timeout");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p ade-bridge state::tests::session_flush`
Expected: 编译失败（`SessionFlushSignal`/两个自由函数未定义）

- [ ] **Step 3: 实现信号原语与编排**

`events.rs` 常量区加：

```rust
/// Window-close quit → one renderer flush window (spec §3.4).
pub const SESSION_FLUSH_REQUESTED: &str = "session:flush-requested";
```

`state.rs`：

1. import/类型区加：

```rust
/// Renderer flush handshake state: `false` until `session_flush_ack` lands.
pub type SessionFlushSignal = Arc<(Mutex<bool>, Condvar)>;
```

2. 自由函数（`PtyWorktreeIds` 定义之前）：

```rust
pub(crate) fn arm_session_flush_signal(slot: &Mutex<Option<SessionFlushSignal>>) -> SessionFlushSignal {
    let signal: SessionFlushSignal = Arc::new((Mutex::new(false), Condvar::new()));
    *lock(slot) = Some(Arc::clone(&signal));
    signal
}

pub(crate) fn signal_session_flush_ack(slot: &Mutex<Option<SessionFlushSignal>>) {
    let current = lock(slot).clone();
    if let Some(signal) = current {
        let (flag, cvar) = &*signal;
        let mut acked = flag.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *acked = true;
        cvar.notify_all();
    }
}
```

3. `AppState` 字段（`session` 之后）：

```rust
    /// `session:flush-requested` 握手槽（spec §3.4；None = 无等待中的退出编排）。
    session_flush_slot: Mutex<Option<SessionFlushSignal>>,
```

构造器 `Ok(Self { ... })` 里加 `session_flush_slot: Mutex::new(None),`。

4. `impl AppState` 加编排方法（`flush_pending_writes` 附近）：

```rust
    /// Window-close quit (spec §3.4): request one renderer flush window, wait
    /// on a background thread (never the event-loop thread — macOS WKWebView
    /// IPC is main-thread), then exit regardless of the outcome.
    pub fn flush_session_then_exit(&self) {
        let signal = arm_session_flush_signal(&self.session_flush_slot);
        events::emit_json(&self.app, events::SESSION_FLUSH_REQUESTED, serde_json::json!({}));
        let app_for_thread = self.app.clone();
        let wait = std::thread::Builder::new()
            .name("session-flush-exit".to_string())
            .spawn(move || {
                let (flag, cvar) = &*signal;
                let guard = flag.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let (guard, acked) = cvar
                    .wait_timeout_for(guard, FLUSH_ACK_TIMEOUT)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                drop(guard);
                if !acked {
                    eprintln!("[ade-bridge] session flush ack not received in time; exiting");
                }
                app_for_thread.exit(0);
            });
        if wait.is_err() {
            // No waiter thread → no flush window; still must exit.
            self.app.exit(0);
        }
    }
```

5. 常量（`WRITE_MAX_WAIT` 之后）：

```rust
/// Renderer flush window on window-close quit (spec §3.4).
const FLUSH_ACK_TIMEOUT: Duration = Duration::from_secs(2);
```

`commands/session.rs` 追加 ack 命令：

```rust
/// Renderer acknowledges the `session:flush-requested` window (spec §3.4).
#[tauri::command]
#[specta::specta]
pub async fn session_flush_ack(state: State<'_, AppState>) -> Result<(), BridgeError> {
    crate::state::signal_session_flush_ack(&state.session_flush_slot);
    Ok(())
}
```

（`session_flush_slot` 字段可见性：同 crate 内 commands 访问需 `pub(crate)`——把字段声明改为 `pub(crate) session_flush_slot: Mutex<Option<SessionFlushSignal>>,`。）

`specta_export.rs`：`collect_commands!` 的 session 组末尾（`commands::session::session_flush,` 后）加 `commands::session::session_flush_ack,`；`export_lists_every_command` 清单 `"session_flush",` 后加 `"session_flush_ack",`。

`src-tauri/src/lib.rs`：`app.run` 闭包改为 match（RunEvent 非 exhaustive）：

```rust
    app.run(|app_handle, event| {
        match event {
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                // code: None = window-close-initiated quit (spec §3.4): give the
                // renderer one flush window, then exit from the waiter thread.
                // Some(_) = explicit exit() from that waiter — pass through.
                if code.is_some() {
                    return;
                }
                if let Some(state) = app_handle.try_state::<AppState>() {
                    state.flush_session_then_exit();
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    // Debounced settings/ui writes may still be pending; flush them so a
                    // quick quit after a change cannot lose the update (spec §4.1).
                    state.flush_pending_writes();
                    if let Err(error) = state.session_store().checkpoint_truncate() {
                        eprintln!("[ade] failed to checkpoint session store on exit: {error}");
                    }
                    // 逐会话 kill（带 2s+2s 升级时限）——订阅流随会话退出自然终止
                    // （规格 §3.1：app 退出全量收尾）。
                    state.pty_host.shutdown_all();
                }
            }
            _ => {}
        }
    });
```

- [ ] **Step 4: 跑测试、再生成 bindings**

Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`
Expected: 全 PASS（含新 2 个信号测试）

- [ ] **Step 5: Commit**

```bash
git add src-tauri src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): 退出 flush 协议——session:flush-requested 事件 + ack 握手 + ExitRequested 编排（2s 超时兜底）"
```

---

### Task 5: 渲染层 bridge——session real 域 + preflight 捕获契约

**Files:**
- Create: `src/bridge/real/session.ts`
- Modify: `src/bridge/mock/workspace-session-api.ts`（补桩）
- Modify: `src/shared/preload-api/api/preflight-api.ts`（加 `resolveAgentProviderSession`）
- Modify: `src/bridge/real/preflight.ts`、`src/bridge/mock/preflight-api.ts`
- Modify: `src/bridge/create-api.ts`（RealDomains 增 `session`）
- Modify: `src/bridge/real/parity.test.ts`
- Test: `src/bridge/real/session.test.ts`

**Interfaces:**
- Consumes: Task 2/3/4 的命令（`session_get/set/patch/flush/flush_ack`、`agent_sessions_resolve_capture`）与事件 `session:flush-requested`
- Produces:
  - `createSessionRealApi(): Pick<PreloadApi, 'session'>`（session/cache/remoteWorkspace 三子域；cache/remoteWorkspace 复用 mock）
  - `registerSessionFlushHandler(handler: () => Promise<void>): () => void`（Task 9 注入）
  - `PreloadApi['preflight']['resolveAgentProviderSession']`（Task 8 消费，返回 `AgentProviderSessionMetadata | null`）

- [ ] **Step 1: 写失败测试**

创建 `src/bridge/real/session.test.ts`：

```ts
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  createSessionRealApi,
  registerSessionFlushHandler
} from './session'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type FlushListener = (payload: unknown) => void
let flushListener: FlushListener | null = null

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
  flushListener = null
  listenMock.mockImplementation((_event: string, handler: FlushListener) => {
    flushListener = handler
    return Promise.resolve(() => {})
  })
})

describe('session real domain', () => {
  it('get parses the JSON text payload', async () => {
    invokeMock.mockResolvedValue(JSON.stringify({ activeTabId: 't1' }))
    const api = createSessionRealApi()
    const state = await api.session.get()
    expect(invokeMock).toHaveBeenCalledWith('session_get')
    expect(state.activeTabId).toBe('t1')
  })

  it('patch stringifies the payload into the args envelope', async () => {
    invokeMock.mockResolvedValue(undefined)
    const api = createSessionRealApi()
    await api.session.patch({ activeTabId: 't2' })
    expect(invokeMock).toHaveBeenCalledWith('session_patch', {
      args: JSON.stringify({ activeTabId: 't2' })
    })
  })

  it('set stringifies the full state into the args envelope', async () => {
    invokeMock.mockResolvedValue(undefined)
    const api = createSessionRealApi()
    await api.session.set({ activeTabId: null })
    expect(invokeMock).toHaveBeenCalledWith('session_set', {
      args: JSON.stringify({ activeTabId: null })
    })
  })

  it('flush invokes session_flush', async () => {
    invokeMock.mockResolvedValue(undefined)
    const api = createSessionRealApi()
    await api.session.flush()
    expect(invokeMock).toHaveBeenCalledWith('session_flush')
  })

  it('setSync fires without awaiting and swallows errors', async () => {
    invokeMock.mockRejectedValue(new Error('disk full'))
    const api = createSessionRealApi()
    expect(() => api.session.setSync({ activeTabId: null })).not.toThrow()
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalled())
  })

  it('readTerminalScrollback stays null (deleted domain G refs)', () => {
    expect(createSessionRealApi().session.readTerminalScrollback({ ref: 'v1-x' })).toBeNull()
  })
})

describe('session flush handshake', () => {
  it('runs the registered handler then acks', async () => {
    const handler = vi.fn().mockResolvedValue(undefined)
    const unregister = registerSessionFlushHandler(handler)
    expect(typeof flushListener).toBe('function')
    flushListener!({})
    await vi.waitFor(() => {
      expect(handler).toHaveBeenCalledTimes(1)
      expect(invokeMock).toHaveBeenCalledWith('session_flush_ack')
    })
    unregister()
  })

  it('acks even when the handler rejects', async () => {
    const handler = vi.fn().mockRejectedValue(new Error('capture failed'))
    registerSessionFlushHandler(handler)
    flushListener!({})
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('session_flush_ack'))
  })

  it('unregister drops the handler but acks still flow', async () => {
    const handler = vi.fn().mockResolvedValue(undefined)
    const unregister = registerSessionFlushHandler(handler)
    unregister()
    flushListener!({})
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('session_flush_ack'))
    expect(handler).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/bridge/real/session.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现 real/session.ts**

```ts
import type { PreloadApi } from '../../preload/api-types'
import type {
  WorkspaceSessionPatch,
  WorkspaceSessionState
} from '../../shared/workspace-session-state-types'
import { createCacheApi } from '../mock/cache-api'
import { createRemoteWorkspaceApi } from '../mock/workspace-session-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Workspace session state (spec §3.2): opaque JSON documents over opaque
 * `String` payloads. `readTerminalScrollback` stays null — the refs lane was
 * the deleted remote-mirror domain's; inline `buffersByLeafId` is the restore
 * source (spec R1).
 */
export function createSessionRealApi(): Pick<PreloadApi, 'session'> {
  return {
    session: withMethodFallback<PreloadApi['session']['session']>('session', {
      get: async () => JSON.parse(await invokeCommand<string>('session_get')) as WorkspaceSessionState,
      set: async (args: WorkspaceSessionState) => {
        await invokeCommand('session_set', { args: JSON.stringify(args) })
      },
      patch: async (args: WorkspaceSessionPatch) => {
        await invokeCommand('session_patch', { args: JSON.stringify(args) })
      },
      flush: async () => {
        await invokeCommand('session_flush')
      },
      readTerminalScrollback: () => null,
      // Fire-and-forget: the void contract has no error lane (same precedent
      // as `pty.write`); persistence failures surface on the next `get`.
      setSync: (args: WorkspaceSessionState) => {
        void invokeCommand('session_set', { args: JSON.stringify(args) }).catch(() => {})
      }
    }),
    // cache/remoteWorkspace stay mock in 2A (spec §2.2).
    cache: createCacheApi(),
    remoteWorkspace: createRemoteWorkspaceApi()
  }
}

export type SessionFlushHandler = () => Promise<void>

let flushHandler: SessionFlushHandler | null = null
let flushSubscriptionStarted = false

/**
 * Quit handshake (spec §3.4): the host prevents exit, emits
 * `session:flush-requested`, and waits for `session_flush_ack` (2s timeout).
 * The ack flows regardless of handler outcome — persistence failures must not
 * wedge the quit.
 */
export function registerSessionFlushHandler(handler: SessionFlushHandler): () => void {
  flushHandler = handler
  if (!flushSubscriptionStarted) {
    flushSubscriptionStarted = true
    subscribeToEvent('session:flush-requested', () => {
      const current = flushHandler
      const run = current ? current() : Promise.resolve()
      void run
        .catch(() => {})
        .finally(() => {
          void invokeCommand('session_flush_ack').catch(() => {})
        })
    })
  }
  return () => {
    if (flushHandler === handler) {
      flushHandler = null
    }
  }
}
```

- [ ] **Step 4: mock 补桩 + preflight 契约 + create-api 接线**

`src/bridge/mock/workspace-session-api.ts` 的 `createSessionApi` 补齐六个方法：

```ts
export function createSessionApi(): PreloadApi['session']['session'] {
  return withMethodFallback<PreloadApi['session']['session']>('session', {
    get: async () => getDefaultWorkspaceSession(),
    set: async () => {},
    patch: async () => {},
    flush: async () => {},
    readTerminalScrollback: () => null,
    setSync: () => {}
  })
}
```

`src/shared/preload-api/api/preflight-api.ts` 的 `PreflightApi` 类型加（import `AgentProviderSessionMetadata` from `'../../agent-session-resume'`，按该文件既有相对路径深度调整）：

```ts
  /** Transcript-scan capture of a resumable provider session (spec §3.3). */
  resolveAgentProviderSession: (args: {
    cwd: string
    agentKind: string
    windowFromMs: number
    windowToMs: number
  }) => Promise<AgentProviderSessionMetadata | null>
```

`src/bridge/real/preflight.ts` 的工厂对象加（沿用该文件 `invokeCommand` 用法）：

```ts
    resolveAgentProviderSession: (args) =>
      invokeCommand('agent_sessions_resolve_capture', { args }),
```

`src/bridge/mock/preflight-api.ts` 加桩：

```ts
    resolveAgentProviderSession: async () => null,
```

`src/bridge/create-api.ts`：
1. `RealDomains` Pick 联合类型加 `| 'session'`
2. import 加 `import { createSessionRealApi } from './real/session'`
3. `createRealDomains()` 返回对象加 `session: createSessionRealApi(),`

- [ ] **Step 5: parity 门禁更新**

`src/bridge/real/parity.test.ts`：
1. import 加 `import { createSessionRealApi } from './session'`
2. `realApiFor` switch 加：

```ts
    case 'session':
      return createSessionRealApi()
```

3. 文件末尾（`realApiFor` 之前）加 session 专用键集锁定（子域对象不能进 `surfaceCases` 的函数断言，参照 pty.management 特例模式）：

```ts
const sessionSubApiSurface = [
  'get',
  'set',
  'patch',
  'flush',
  'readTerminalScrollback',
  'setSync'
] as const satisfies readonly (keyof PreloadApi['session']['session'])[]

describe('mock/real parity: session sub-surface', () => {
  it('session implements the full renderer session sub-surface', () => {
    const real = createSessionRealApi().session
    for (const method of sessionSubApiSurface) {
      expect(
        Object.prototype.hasOwnProperty.call(real, method),
        `session.${method} must be explicitly implemented in real mode`
      ).toBe(true)
      expect(typeof real[method], `session.${method} must be a function`).toBe('function')
    }
  })

  it('preflight implements resolveAgentProviderSession in real mode', () => {
    const real = createPreflightRealApi() as unknown as Record<string, unknown>
    expect(Object.prototype.hasOwnProperty.call(real, 'resolveAgentProviderSession')).toBe(true)
    expect(typeof real.resolveAgentProviderSession).toBe('function')
    const mock = createMockAdeApi().preflight as unknown as Record<string, unknown>
    expect(typeof mock.resolveAgentProviderSession).toBe('function')
  })
})
```

- [ ] **Step 6: 跑测试与门禁**

Run: `pnpm vitest run src/bridge && pnpm typecheck`
Expected: bridge 测试全 PASS；typecheck exit 0

- [ ] **Step 7: Commit**

```bash
git add src/bridge src/shared/preload-api/api/preflight-api.ts
git commit -m "feat(renderer): session 域接真 + flush 握手注册面 + preflight transcript 捕获契约"
```

---

### Task 6: 本地 scrollback buffers 保留翻转（R2）

**Files:**
- Modify: `src/shared/workspace-session-terminal-buffers.ts:17-24`
- Test: `src/shared/workspace-session-terminal-buffers.test.ts`

**Interfaces:**
- Consumes: 无
- Produces: `repoNeedsRendererCapturedScrollback` 恒 true（捕获门控与 payload 裁剪两处调用点行为翻转，签名不变）

- [ ] **Step 1: 更新失败测试**

`src/shared/workspace-session-terminal-buffers.test.ts` 中所有断言「本地 repo 裁剪/不保留」的用例反转为「保留」。代表形态（按现有用例结构改写断言与名称）：

```ts
  it('preserves scrollback for local repos too (ade has no daemon — spec R2)', () => {
    // 原断言：本地 repo 的 buffersByLeafId 被 prune 掉；现必须保留。
    const pruned = pruneLocalTerminalScrollbackBuffers(sessionWithLocalBuffers, [localRepo])
    expect(pruned.terminalLayoutsByTabId?.t1?.buffersByLeafId).toBeDefined()
    expect(shouldPreserveTerminalScrollbackBuffers(localWorktreeId, [localRepo])).toBe(true)
  })
```

若文件中存在「远程保留/本地裁剪」的参数化用例表，将本地行并入保留侧；保留「未知 repo 按保留处理」用例不变（其语义被恒 true 吸收，可留作回归）。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/shared/workspace-session-terminal-buffers.test.ts`
Expected: FAIL（现实现裁剪本地 repo）

- [ ] **Step 3: 实现翻转**

`repoNeedsRendererCapturedScrollback` 改为：

```ts
function repoNeedsRendererCapturedScrollback(_repo: RepoTerminalScrollbackOwner): boolean {
  // ade has no out-of-process PTY daemon (spec R2): renderer-captured
  // scrollback is the only durable copy for every repo kind, local included.
  // Signature kept — upstream flips this off when a daemon lands.
  return true
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/shared/workspace-session-terminal-buffers.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/shared/workspace-session-terminal-buffers.ts src/shared/workspace-session-terminal-buffers.test.ts
git commit -m "feat(renderer): 本地 repo scrollback buffers 纳入持久化保留——ade 无 daemon（R2）"
```

---

### Task 7: R3 捕获调度——60s 间隔 + 隐藏触发

**Files:**
- Create: `src/renderer/src/lib/terminal-buffer-capture-scheduler.ts`
- Create: `src/renderer/src/lib/capture-all-terminal-buffers.ts`
- Modify: `src/renderer/src/app-shell/use-app-session-persistence.ts`
- Test: `src/renderer/src/lib/terminal-buffer-capture-scheduler.test.ts`

**Interfaces:**
- Consumes: `shutdownBufferCaptures`（`components/terminal-pane/shutdown-buffer-captures.ts`，值为 `(options?: { includeLocalBuffers?: boolean }) => void`）
- Produces: `createTerminalBufferCaptureScheduler(deps): () => void`、`captureAllMountedTabBuffers(): void`（Task 9 复用）

- [ ] **Step 1: 写失败测试**

创建 `src/renderer/src/lib/terminal-buffer-capture-scheduler.test.ts`：

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createTerminalBufferCaptureScheduler } from './terminal-buffer-capture-scheduler'

describe('createTerminalBufferCaptureScheduler', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('captures on the interval while visible', () => {
    const captureAll = vi.fn()
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => false
    })
    vi.advanceTimersByTime(120_000)
    expect(captureAll).toHaveBeenCalledTimes(2)
    stop()
  })

  it('skips the interval capture while hidden', () => {
    const captureAll = vi.fn()
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => true
    })
    vi.advanceTimersByTime(120_000)
    expect(captureAll).not.toHaveBeenCalled()
    stop()
  })

  it('captures immediately when visibility turns hidden', () => {
    const captureAll = vi.fn()
    let hidden = false
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => hidden
    })
    hidden = true
    document.dispatchEvent(new Event('visibilitychange'))
    expect(captureAll).toHaveBeenCalledTimes(1)
    stop()
  })

  it('stop tears down the interval and the listener', () => {
    const captureAll = vi.fn()
    let hidden = true
    const stop = createTerminalBufferCaptureScheduler({
      captureAll,
      isDocumentHidden: () => hidden
    })
    stop()
    vi.advanceTimersByTime(120_000)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(captureAll).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/terminal-buffer-capture-scheduler.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现两个模块**

`src/renderer/src/lib/terminal-buffer-capture-scheduler.ts`：

```ts
export type TerminalBufferCaptureSchedulerDeps = {
  captureAll: () => void
  isDocumentHidden: () => boolean
  intervalMs?: number
}

/**
 * R3 (spec §5.3): a coarse crash-loss floor, not the primary durability path —
 * graceful quit goes through the host-driven flush (§3.4). Periodic full
 * re-serialize was removed upstream for main-thread stalls (#461); the 60s
 * interval matches the existing resume-capture cadence and skips while hidden.
 */
export function createTerminalBufferCaptureScheduler(
  deps: TerminalBufferCaptureSchedulerDeps
): () => void {
  const intervalMs = deps.intervalMs ?? 60_000
  const captureIfVisible = (): void => {
    if (!deps.isDocumentHidden()) {
      deps.captureAll()
    }
  }
  const onVisibilityChange = (): void => {
    if (deps.isDocumentHidden()) {
      deps.captureAll()
    }
  }
  document.addEventListener('visibilitychange', onVisibilityChange)
  const timer = window.setInterval(captureIfVisible, intervalMs)
  return () => {
    window.clearInterval(timer)
    document.removeEventListener('visibilitychange', onVisibilityChange)
  }
}
```

`src/renderer/src/lib/capture-all-terminal-buffers.ts`：

```ts
import { shutdownBufferCaptures } from '../components/terminal-pane/shutdown-buffer-captures'

/**
 * Serialize every mounted tab's buffers into the store (spec §4.1 triggers B/C).
 * Default capture options keep local buffers — ade has no daemon, so the
 * renderer capture is the only durable scrollback copy (spec R2).
 */
export function captureAllMountedTabBuffers(): void {
  for (const capture of shutdownBufferCaptures.values()) {
    try {
      capture()
    } catch {
      // One pane's serialization failure must not block the rest.
    }
  }
}
```

`use-app-session-persistence.ts`：import 两模块，在 `useAppSessionPersistence` 内（60s resume 捕获 effect 之后）加：

```ts
  // R3 (spec §5.3): coarse crash-loss floor; hidden documents skip serialization.
  useEffect(
    () =>
      createTerminalBufferCaptureScheduler({
        captureAll: captureAllMountedTabBuffers,
        isDocumentHidden: () => document.visibilityState === 'hidden'
      }),
    []
  )
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/lib/terminal-buffer-capture-scheduler.test.ts && pnpm typecheck`
Expected: 4 测试 PASS；typecheck exit 0

- [ ] **Step 5: Commit**

```bash
git add src/renderer/src/lib/terminal-buffer-capture-scheduler.ts src/renderer/src/lib/terminal-buffer-capture-scheduler.test.ts src/renderer/src/lib/capture-all-terminal-buffers.ts src/renderer/src/app-shell/use-app-session-persistence.ts
git commit -m "feat(renderer): R3 scrollback 捕获调度——60s 间隔 + visibilitychange 隐藏触发"
```

---

### Task 8: transcript 捕获模块 + 注册表合并动作

**Files:**
- Create: `src/renderer/src/lib/agent-transcript-capture.ts`
- Modify: `src/renderer/src/store/slices/agent-status-slice-contract.ts:158`（加动作类型）
- Modify: `src/renderer/src/store/slices/agent-status-recovery-actions.ts`（加动作实现）
- Modify: `src/renderer/src/app-shell/use-app-session-persistence.ts`（quit/periodic 接线）
- Test: `src/renderer/src/lib/agent-transcript-capture.test.ts`

**Interfaces:**
- Consumes: Task 5 的 `window.api.preflight.resolveAgentProviderSession`；`titleHasAgentName`（`shared/agent-detection` barrel）；`makePaneKey`（`shared/stable-pane-id.ts`）；`findAgentPaneWorktreeId`（`store/slices/agent-status-pane-key-tab-binding.ts:102`）；`SleepingAgentSessionRecord`（`shared/agent-session-resume.ts:45`）
- Produces: `captureTranscriptAgentSessions(deps): Promise<void>`、`defaultTranscriptCaptureIo`；store 动作 `mergeSleepingAgentSessionRecords(records: SleepingAgentSessionRecord[]): void`（Task 9 复用）

- [ ] **Step 1: 写失败测试**

创建 `src/renderer/src/lib/agent-transcript-capture.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import type { AppState } from '../store'
import {
  captureTranscriptAgentSessions,
  type TranscriptCaptureDeps
} from './agent-transcript-capture'

const LEAF_A = '11111111-1111-4111-8111-111111111111'
const LEAF_B = '22222222-2222-4222-8222-222222222222'

function makeState(overrides?: Partial<Record<string, unknown>>): AppState {
  return {
    tabsByWorktree: {
      'w1::/repo': [
        { id: 'tab1', title: 'claude · working' },
        { id: 'tab2', title: 'zsh' }
      ]
    },
    terminalLayoutsByTabId: {
      tab1: {
        root: { type: 'leaf', leafId: LEAF_A },
        activeLeafId: LEAF_A,
        expandedLeafId: null,
        ptyIdsByLeafId: { [LEAF_A]: 'pty-1' },
        titlesByLeafId: { [LEAF_A]: 'claude · working' }
      },
      tab2: {
        root: { type: 'leaf', leafId: LEAF_B },
        activeLeafId: LEAF_B,
        expandedLeafId: null,
        ptyIdsByLeafId: { [LEAF_B]: 'pty-2' }
      }
    },
    sleepingAgentSessionsByPaneKey: {},
    ...overrides
  } as unknown as AppState
}

function makeDeps(overrides?: Partial<TranscriptCaptureDeps>): TranscriptCaptureDeps {
  const merged: TranscriptCaptureDeps = {
    state: makeState(),
    origin: 'quit',
    resolveCwd: async () => '/repo/work',
    resolveProviderSession: async () => ({ key: 'session_id', id: 'session-uuid' }),
    now: () => 5_000,
    mergeRecords: vi.fn(),
    ...overrides
  }
  return merged
}

describe('captureTranscriptAgentSessions', () => {
  it('captures records for agent-titled panes via the host scan', async () => {
    const mergeRecords = vi.fn()
    await captureTranscriptAgentSessions(makeDeps({ mergeRecords }))
    expect(mergeRecords).toHaveBeenCalledTimes(1)
    const records = mergeRecords.mock.calls[0][0]
    expect(records).toHaveLength(1)
    expect(records[0]).toMatchObject({
      paneKey: `tab1:${LEAF_A}`,
      tabId: 'tab1',
      worktreeId: 'w1::/repo',
      agent: 'claude',
      providerSession: { key: 'session_id', id: 'session-uuid' },
      prompt: '',
      state: 'waiting',
      origin: 'quit'
    })
  })

  it('passes cwd, agentKind and the boot-anchored window to the scan', async () => {
    const resolveProviderSession = vi.fn().mockResolvedValue(null)
    await captureTranscriptAgentSessions(makeDeps({ resolveProviderSession }))
    expect(resolveProviderSession).toHaveBeenCalledWith({
      cwd: '/repo/work',
      agentKind: 'claude',
      windowFromMs: expect.any(Number),
      windowToMs: 5_000
    })
  })

  it('skips shell-titled panes and panes whose record already has a providerSession', async () => {
    const resolveProviderSession = vi.fn()
    const state = makeState({
      sleepingAgentSessionsByPaneKey: {
        [`tab1:${LEAF_A}`]: {
          paneKey: `tab1:${LEAF_A}`,
          worktreeId: 'w1::/repo',
          agent: 'claude',
          providerSession: { key: 'session_id', id: 'existing' },
          prompt: '',
          state: 'waiting',
          capturedAt: 1,
          updatedAt: 1
        }
      }
    })
    await captureTranscriptAgentSessions(makeDeps({ state, resolveProviderSession }))
    expect(resolveProviderSession).not.toHaveBeenCalled()
  })

  it('skips panes without a live ptyId or a failed cwd probe', async () => {
    const resolveProviderSession = vi.fn()
    const noPty = makeState()
    ;(noPty.terminalLayoutsByTabId.tab1 as { ptyIdsByLeafId?: Record<string, string> }).ptyIdsByLeafId = undefined
    await captureTranscriptAgentSessions(makeDeps({ state: noPty, resolveProviderSession }))
    expect(resolveProviderSession).not.toHaveBeenCalled()
    await captureTranscriptAgentSessions(
      makeDeps({ resolveCwd: async () => null, resolveProviderSession })
    )
    expect(resolveProviderSession).not.toHaveBeenCalled()
  })

  it('a null scan result yields no record and no merge', async () => {
    const mergeRecords = vi.fn()
    await captureTranscriptAgentSessions(
      makeDeps({ resolveProviderSession: async () => null, mergeRecords })
    )
    expect(mergeRecords).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/agent-transcript-capture.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现模块与 store 动作**

`src/renderer/src/lib/agent-transcript-capture.ts`：

```ts
import { titleHasAgentName } from '../../../shared/agent-detection'
import type {
  AgentProviderSessionMetadata,
  SleepingAgentSessionRecord
} from '../../../shared/agent-session-resume'
import { makePaneKey } from '../../../shared/stable-pane-id'
import type { TerminalPaneLayoutNode } from '../../../shared/terminal-tab-types'
import type { AppState } from '../store'
import { findAgentPaneWorktreeId } from '../store/slices/agent-status-pane-key-tab-binding'

const CAPTURE_AGENTS = ['claude', 'codex'] as const
type CaptureAgent = (typeof CAPTURE_AGENTS)[number]

/** Renderer boot time is the widest window 2A tracks (spec §3.3). */
export const RENDERER_BOOT_MS = Date.now()

export type TranscriptCaptureDeps = {
  state: AppState
  origin: 'quit' | 'live'
  resolveCwd: (ptyId: string) => Promise<string | null>
  resolveProviderSession: (args: {
    cwd: string
    agentKind: string
    windowFromMs: number
    windowToMs: number
  }) => Promise<AgentProviderSessionMetadata | null>
  now: () => number
  mergeRecords: (records: SleepingAgentSessionRecord[]) => void
}

type CaptureTarget = {
  paneKey: string
  tabId: string
  worktreeId: string
  agent: CaptureAgent
  ptyId: string
}

function collectLeafIds(node: TerminalPaneLayoutNode | null, into: string[]): void {
  if (!node) {
    return
  }
  if (node.type === 'leaf') {
    into.push(node.leafId)
    return
  }
  collectLeafIds(node.first, into)
  collectLeafIds(node.second, into)
}

function collectCaptureTargets(state: AppState): CaptureTarget[] {
  const targets: CaptureTarget[] = []
  for (const [worktreeKey, tabs] of Object.entries(state.tabsByWorktree)) {
    for (const tab of tabs) {
      const layout = state.terminalLayoutsByTabId[tab.id]
      if (!layout) {
        continue
      }
      const leafIds: string[] = []
      collectLeafIds(layout.root, leafIds)
      for (const leafId of leafIds) {
        let paneKey: string
        try {
          paneKey = makePaneKey(tab.id, leafId)
        } catch {
          continue
        }
        if (state.sleepingAgentSessionsByPaneKey[paneKey]?.providerSession) {
          continue
        }
        const title = layout.titlesByLeafId?.[leafId] ?? tab.title ?? ''
        const agent = CAPTURE_AGENTS.find((candidate) => titleHasAgentName(title, candidate))
        if (!agent) {
          continue
        }
        const ptyId = layout.ptyIdsByLeafId?.[leafId]
        if (!ptyId) {
          continue
        }
        targets.push({
          paneKey,
          tabId: tab.id,
          worktreeId: findAgentPaneWorktreeId(state, paneKey) ?? worktreeKey,
          agent,
          ptyId
        })
      }
    }
  }
  return targets
}

/**
 * OSC-title identity + transcript-directory scan → sleeping resume records
 * (spec §5.4). Best-effort by construction: every failure mode converges on
 * "no record" = shell-only restore, never an error surface.
 */
export async function captureTranscriptAgentSessions(deps: TranscriptCaptureDeps): Promise<void> {
  const targets = collectCaptureTargets(deps.state)
  if (targets.length === 0) {
    return
  }
  const now = deps.now()
  const records: SleepingAgentSessionRecord[] = []
  for (const target of targets) {
    try {
      const cwd = await deps.resolveCwd(target.ptyId)
      if (!cwd) {
        continue
      }
      const providerSession = await deps.resolveProviderSession({
        cwd,
        agentKind: target.agent,
        windowFromMs: RENDERER_BOOT_MS,
        windowToMs: now
      })
      if (!providerSession) {
        continue
      }
      records.push({
        paneKey: target.paneKey,
        tabId: target.tabId,
        worktreeId: target.worktreeId,
        agent: target.agent,
        providerSession,
        prompt: '',
        state: 'waiting',
        capturedAt: now,
        updatedAt: now,
        origin: deps.origin
      })
    } catch {
      // Transcript capture is best-effort; absence means shell-only restore.
    }
  }
  if (records.length > 0) {
    deps.mergeRecords(records)
  }
}

/** Default IO against the bridge (spec §3.3/§3.4 contracts). */
export const defaultTranscriptCaptureIo = {
  resolveCwd: async (ptyId: string): Promise<string | null> => {
    try {
      return await window.api.pty.getCwd(ptyId)
    } catch {
      return null
    }
  },
  resolveProviderSession: (
    args: Parameters<TranscriptCaptureDeps['resolveProviderSession']>[0]
  ): Promise<AgentProviderSessionMetadata | null> =>
    window.api.preflight.resolveAgentProviderSession(args)
}
```

`agent-status-slice-contract.ts`（:158 `captureAllSleepingAgentSessions` 行后）加：

```ts
    mergeSleepingAgentSessionRecords: (records: SleepingAgentSessionRecord[]) => void
```

`agent-status-recovery-actions.ts` 的返回对象加（`captureAllSleepingAgentSessions` 实现之后）：

```ts
    mergeSleepingAgentSessionRecords: (records) => {
      set((s) => {
        let next = s.sleepingAgentSessionsByPaneKey
        let changed = false
        for (const record of records) {
          if (next[record.paneKey] !== record) {
            if (!changed) {
              next = { ...next }
              changed = true
            }
            next[record.paneKey] = record
          }
        }
        return changed ? { sleepingAgentSessionsByPaneKey: next } : s
      })
    },
```

接线（`use-app-session-persistence.ts`）：
1. 60s resume 捕获 interval 回调内（`captureAllSleepingAgentSessions('periodic')` 之后）加：

```ts
        void captureTranscriptAgentSessions({
          state: useAppStore.getState(),
          origin: 'live',
          ...defaultTranscriptCaptureIo,
          now: Date.now,
          mergeRecords: (records) =>
            useAppStore.getState().mergeSleepingAgentSessionRecords(records)
        })
```

2. 关停 checkpoint 的 `captureSleepingAgentSessions` 回调内（`captureAllSleepingAgentSessions('quit')` 之后）加同形调用但 `origin: 'quit'`（beforeunload 尽力语义；可靠的 quit 捕获在 Task 9 的 flush 握手内）。

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/lib/agent-transcript-capture.test.ts && pnpm typecheck`
Expected: 5 测试 PASS；typecheck exit 0

- [ ] **Step 5: Commit**

```bash
git add src/renderer/src/lib/agent-transcript-capture.ts src/renderer/src/lib/agent-transcript-capture.test.ts src/renderer/src/store/slices/agent-status-slice-contract.ts src/renderer/src/store/slices/agent-status-recovery-actions.ts src/renderer/src/app-shell/use-app-session-persistence.ts
git commit -m "feat(renderer): transcript 捕获模块——OSC 身份 + transcript 扫描写入休眠注册表"
```

---

### Task 9: flush-requested 关停持久化接线

**Files:**
- Create: `src/renderer/src/lib/session-flush-persist.ts`
- Modify: `src/renderer/src/app-shell/use-app-session-persistence.ts`
- Test: `src/renderer/src/lib/session-flush-persist.test.ts`

**Interfaces:**
- Consumes: Task 5 的 `registerSessionFlushHandler`、Task 7 的 `captureAllMountedTabBuffers`、Task 8 的 `captureTranscriptAgentSessions`/`defaultTranscriptCaptureIo`、`buildWorkspaceSessionPayload`（`lib/workspace-session.ts:283`）、`shouldPersistWorkspaceSession`（`lib/workspace-session.ts:23`）
- Produces: `createSessionFlushPersist(deps): () => Promise<void>`（注入式，可测；hook 内与 `registerSessionFlushHandler` 组装）

- [ ] **Step 1: 写失败测试**

创建 `src/renderer/src/lib/session-flush-persist.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import { createSessionFlushPersist } from './session-flush-persist'

function makeDeps(overrides?: Partial<Parameters<typeof createSessionFlushPersist>[0]>) {
  const captureAll = vi.fn()
  const captureTranscripts = vi.fn().mockResolvedValue(undefined)
  const patch = vi.fn().mockResolvedValue(undefined)
  const flush = vi.fn().mockResolvedValue(undefined)
  const deps = {
    captureAll,
    captureTranscripts,
    buildPayload: () => ({ activeTabId: 't1' }),
    canPersist: () => true,
    patch,
    flush,
    ...overrides
  }
  return { deps, captureAll, captureTranscripts, patch, flush }
}

describe('createSessionFlushPersist', () => {
  it('captures buffers and transcripts before patching and flushing, in order', async () => {
    const { deps, captureAll, captureTranscripts, patch, flush } = makeDeps()
    const order: string[] = []
    captureAll.mockImplementation(() => order.push('capture'))
    captureTranscripts.mockImplementation(async () => {
      order.push('transcripts')
    })
    patch.mockImplementation(async () => {
      order.push('patch')
    })
    flush.mockImplementation(async () => {
      order.push('flush')
    })
    await createSessionFlushPersist(deps)()
    expect(order).toEqual(['capture', 'transcripts', 'patch', 'flush'])
    expect(patch).toHaveBeenCalledWith({ activeTabId: 't1' })
  })

  it('skips patch and flush when persistence is gated off', async () => {
    const { deps, captureAll, patch, flush } = makeDeps({ canPersist: () => false })
    await createSessionFlushPersist(deps)()
    expect(captureAll).toHaveBeenCalledTimes(1)
    expect(patch).not.toHaveBeenCalled()
    expect(flush).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/session-flush-persist.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现并接线**

`src/renderer/src/lib/session-flush-persist.ts`：

```ts
/**
 * `session:flush-requested` handler body (spec §3.4), factored for tests:
 * capture buffers + resume records, then write the full payload directly —
 * bypassing the 150ms debounced subscriber, whose pending write would race
 * the ack.
 */
export type SessionFlushPersistDeps = {
  captureAll: () => void
  captureTranscripts: () => Promise<void>
  buildPayload: () => Record<string, unknown>
  canPersist: () => boolean
  patch: (payload: Record<string, unknown>) => Promise<void>
  flush: () => Promise<void>
}

export function createSessionFlushPersist(
  deps: SessionFlushPersistDeps
): () => Promise<void> {
  return async () => {
    deps.captureAll()
    await deps.captureTranscripts()
    if (!deps.canPersist()) {
      return
    }
    await deps.patch(deps.buildPayload())
    await deps.flush()
  }
}
```

`use-app-session-persistence.ts`：import `registerSessionFlushHandler`（`@/bridge/real/session`——按该文件对 bridge 的引用方式调整；若渲染层禁止直引 bridge 内部，则经 `window.api` 之外的具名导出路径 `../../../bridge/real/session`，与既有相对引用风格一致）、`createSessionFlushPersist`、`buildWorkspaceSessionPayload`/`shouldPersistWorkspaceSession`（已 import）、Task 8 捕获件。新增 effect：

```ts
  // R2 (spec §3.4): host-driven quit flush — one deterministic capture + full
  // payload patch + flush, then the bridge acks and the host exits.
  useEffect(
    () =>
      registerSessionFlushHandler(
        createSessionFlushPersist({
          captureAll: captureAllMountedTabBuffers,
          captureTranscripts: () =>
            captureTranscriptAgentSessions({
              state: useAppStore.getState(),
              origin: 'quit',
              ...defaultTranscriptCaptureIo,
              now: Date.now,
              mergeRecords: (records) =>
                useAppStore.getState().mergeSleepingAgentSessionRecords(records)
            }),
          buildPayload: () => buildWorkspaceSessionPayload(useAppStore.getState()),
          canPersist: () => shouldPersistWorkspaceSession(useAppStore.getState()),
          patch: (payload) => window.api.session.patch(payload as never),
          flush: () => window.api.session.flush()
        })
      ),
    []
  )
```

（`patch(payload as never)`：`buildWorkspaceSessionPayload` 返回 `WorkspaceSessionState` 形状，按 `workspace-session-host-persistence.ts` 既有 cast 风格收敛类型。）

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/lib/session-flush-persist.test.ts && pnpm typecheck`
Expected: 2 测试 PASS；typecheck exit 0

- [ ] **Step 5: Commit**

```bash
git add src/renderer/src/lib/session-flush-persist.ts src/renderer/src/lib/session-flush-persist.test.ts src/renderer/src/app-shell/use-app-session-persistence.ts
git commit -m "feat(renderer): 退出 flush 握手接线——确定性捕获+全量 patch+flush 后 ack（R2）"
```

---

### Task 10: 终验门禁 + 手工验收清单

**Files:**
- 无代码改动（门禁执行 + 验收）

**Interfaces:**
- Consumes: Task 1–9 全部产出
- Produces: 门禁证据；手工验收待办

- [ ] **Step 1: Rust 全量**

Run: `cargo test --workspace`
Expected: 全绿（含既有 174+ 命令面锁定、bindings 新鲜度）

- [ ] **Step 2: TS 全量**

Run: `pnpm test && pnpm typecheck && pnpm build:web`
Expected: 全绿 / exit 0

- [ ] **Step 3: 手工验收（需用户参与，`pnpm dev`）**

按 spec §7.3 逐项验证并记录结果：

1. 终端跑 `claude` 对话两轮 → 退出应用（Cmd+Q）→ 重启 → tabs/split/scrollback/标题恢复，pane 原 cwd 重生
2. 重启后该 pane **自动 resume**：`claude --resume <id>` 投递、历史对话在、可继续对话
3. `codex` 同验（`codex resume <id>`）
4. split 两 pane（一 shell 一 agent）恢复正确
5. `location.reload()` → 画面恢复（尽力语义：hidden 触发捕获；若偶发空白不阻塞验收，记录窗口）
6. 关闭某 tab → 重启 → 该 tab 不复活、无残留 resume 记录
7. SQLite 损坏演练：手工写坏 `ade.sqlite` → 启动正常（降级无持久化）+ 生成 `ade.sqlite.corrupt-*`
8. 观察 devtools Console 无未处理 rejection；退出时观察 flush ack 日志（无「not received」即握手成功）

- [ ] **Step 4: 记录与收尾**

验收结果写入 `docs/phase2a-persistence-session-restore-record.md`（沿用 phase1c 记录格式：任务表、crate/命令面、数据流终态、测试门禁、手工验收表、延后项），然后按 superpowers:finishing-a-development-branch 收口。

---

## 自审记录（Self-Review）

1. **Spec 覆盖**：§3.1→Task 1；§3.2→Task 2；§3.3→Task 3+8；§3.4→Task 4+9；§3.5→零改动（fork 机械，spec §3.5 即备案）；§5.1→Task 5；§5.2→Task 6；§5.3→Task 7；§5.4→Task 8；§5.5→既有机械（Task 9 的 patch 用 `buildWorkspaceSessionPayload` 全量写保证恢复数据在库）；§6 错误行→Task 1（损坏重建）/Task 4（超时）/Task 3（null 分支）；§7 测试→各任务 Steps + Task 10。无缺口。
2. **占位符扫描**：无 TBD/TODO；Task 2 Step 2 的「先红后绿」因 Rust 命令模块必须先注册才能编译，以两段式说明（注册空模块→todo!()→实现），无含糊。
3. **类型一致性**：`Store` 方法名（put/get/all/replace_all/checkpoint_passive/checkpoint_truncate）跨 Task 1/2/4 一致；`SessionFlushSignal`/`arm_session_flush_signal`/`signal_session_flush_ack` 跨 Task 4 定义与使用一致；`AgentProviderSessionMetadata` Rust（Task 3，camelCase serde）与 TS 契约（Task 5 preflight）字段一致（key/id/transcriptPath）；`captureTranscriptAgentSessions` deps 形状跨 Task 8/9 一致（Task 9 的 `captureTranscripts` 是包装闭包，注入 deps 完整）。
