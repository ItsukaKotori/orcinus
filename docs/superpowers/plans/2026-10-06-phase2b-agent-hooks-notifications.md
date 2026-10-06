# Phase 2B：agent hook server + 状态接真 + 完成通知 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** claude hook 回调链路打通——新 crate `ade-hooks` 接收 hook、归因到 pane 并转发 renderer；`agentStatus` 与 `notifications` 两个既有契约域在真实模式下接真，点亮状态面/完成通知/未读面。

**Architecture:** Rust 侧 `ade-hooks` 自带 HTTP server（std TcpListener + token，无 tokio/HTTP 依赖）+ endpoint 发布（0600）+ `last-status.json` 250ms 防抖缓存 + `~/.claude/settings.json` 托管条目写入器 + `~/.ade/agent-hooks/claude-hook.sh` spool 兜底脚本；归一化保持 TS：`agent-hook:raw` Tauri 事件 → `src/bridge/real/agent-status.ts` 复用 `shared/agent-hook-listener.ts` 纯函数 → `AgentStatusIpcPayload` → 既有 55 slices/协调器消费。notifications 域走 `tauri-plugin-notification` + 两个宿主辅助命令（openSystemSettings / 自定义音效读取）。

**Tech Stack:** Rust（std net/threads、serde_json、base64、tauri-specta）、TS（React + zustand + vitest、@tauri-apps/plugin-notification）。

**Spec:** `docs/superpowers/specs/2026-10-06-phase2b-agent-hooks-notifications-design.md`（执行前通读；§4 两处实现裁定见 Task 8/11 的裁决说明）

## Global Constraints

- 门禁：`cargo test --workspace` 全绿；`pnpm test` 全绿；`pnpm typecheck && pnpm build:web` exit 0
- Rust 快测：`cargo test -p ade-hooks -p ade-bridge`（在 `src-tauri/` 下执行）
- 新增 Rust 依赖：`ade-hooks`（serde/serde_json/thiserror/base64/ade-core；dev-deps tempfile）——**不引入 HTTP server crate、不引入 tokio**（std `TcpListener` + 线程）；根 `Cargo.toml` 增 `tauri-plugin-notification = "2"`；`ade-bridge` 增 `ade-hooks`（path）与 `base64`
- 新增 npm 依赖：仅 `@tauri-apps/plugin-notification`（`^2`）；`pnpm install` 后提交 `pnpm-lock.yaml`
- bindings 单一登记点：新命令必须同时进 `src-tauri/crates/ade-bridge/src/specta_export.rs` 的 `collect_commands!` **和** `export_lists_every_command` 测试清单，然后 `cargo run -p ade-bridge --bin export-bindings` 再生成 `src/bridge/real/generated/tauri-bindings.ts`
- 契约文件 `src/shared/preload-api/api/*.ts` 只做加法；`agentStatus` 的 drop 系/migration/infer 系按规格 §2.2、§3.7 维持 fallback noop
- 环境变量名沿用 fork：`ORCA_PANE_KEY` 格式 `${tabId}:${leafId}`（leafId 为 UUID），hook server env 全套 `ORCA_AGENT_HOOK_*`
- 归一化失败/未知事件/非法 JSON：renderer 丢弃、宿主 fail-open（HTTP 204），不阻塞后续
- 提交信息：中文 conventional commits（`feat(hooks): …`），结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 执行前用 superpowers:using-git-worktrees 建隔离工作区

---

### Task 1: ade-hooks 基座 + endpoint 发布

**Files:**
- Create: `src-tauri/crates/ade-hooks/Cargo.toml`
- Create: `src-tauri/crates/ade-hooks/src/lib.rs`
- Create: `src-tauri/crates/ade-hooks/src/endpoint.rs`

**Interfaces:**
- Consumes: `ade_core::ids::new_uuid`（`crates/ade-core/src/ids.rs:3`）
- Produces（后续 Task 5/6/7 依赖）:
  - `ade_hooks::endpoint::{ENDPOINT_FILE_NAME, HOOK_PROTOCOL_VERSION, HOOK_RAW_JSON_TRANSPORT, EndpointFields, is_shell_safe_value, endpoint_env_lines, write_endpoint_file, pty_env}`
  - `EndpointFields { port: u16, token: String, env: String }`
  - `write_endpoint_file(dir: &Path, fields: &EndpointFields) -> io::Result<bool>`（false = 值含 shell 元字符拒绝写）
  - `pty_env(fields: &EndpointFields, endpoint_path: &Path) -> HashMap<String,String>`

- [ ] **Step 1: 建 crate 与 Cargo 依赖**

`src-tauri/crates/ade-hooks/Cargo.toml`：

```toml
[package]
name = "ade-hooks"
version = "0.0.1"
edition = "2021"

[dependencies]
ade-core = { path = "../ade-core" }
base64 = "0.22"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```

`src-tauri/crates/ade-hooks/src/lib.rs`：

```rust
//! Agent hook receiver (spec §2.1/§3): HTTP ingest, attribution, endpoint
//! publication, managed claude settings/script, and the last-status cache.
//! 归一化不在此层——宿主只搬运原始 hook JSON（规格 §3.2）。

pub mod endpoint;
```

- [ ] **Step 2: 写失败测试**

创建 `src-tauri/crates/ade-hooks/src/endpoint.rs`，先只写类型/函数签名（`todo!()` 占位即可视为红）：

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ade_core::ids;

pub const ENDPOINT_FILE_NAME: &str = "endpoint.env";
pub const HOOK_PROTOCOL_VERSION: &str = "1";
pub const HOOK_RAW_JSON_TRANSPORT: &str = "raw-json-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointFields {
    pub port: u16,
    pub token: String,
    pub env: String,
}

pub fn is_shell_safe_value(value: &str) -> bool {
    todo!()
}

pub fn endpoint_env_lines(fields: &EndpointFields) -> Vec<(String, String)> {
    todo!()
}

pub fn write_endpoint_file(dir: &Path, fields: &EndpointFields) -> std::io::Result<bool> {
    todo!()
}

pub fn pty_env(fields: &EndpointFields, endpoint_path: &Path) -> HashMap<String, String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fields() -> EndpointFields {
        EndpointFields {
            port: 43123,
            token: "a1b2-c3".to_string(),
            env: "development".to_string(),
        }
    }

    #[test]
    fn shell_safe_values_reject_metacharacters_and_empty() {
        assert!(is_shell_safe_value("a1B2-c3._:/x"));
        assert!(!is_shell_safe_value(""));
        assert!(!is_shell_safe_value("tok en"));
        assert!(!is_shell_safe_value("tok\n"));
        assert!(!is_shell_safe_value("tok;rm"));
    }

    #[test]
    fn endpoint_lines_carry_port_token_env_version_and_transport() {
        assert_eq!(
            endpoint_env_lines(&fields()),
            vec![
                ("ORCA_AGENT_HOOK_PORT".to_string(), "43123".to_string()),
                ("ORCA_AGENT_HOOK_TOKEN".to_string(), "a1b2-c3".to_string()),
                (
                    "ORCA_AGENT_HOOK_ENV".to_string(),
                    "development".to_string()
                ),
                (
                    "ORCA_AGENT_HOOK_VERSION".to_string(),
                    "1".to_string()
                ),
                (
                    "ORCA_AGENT_HOOK_TRANSPORT".to_string(),
                    "raw-json-v1".to_string()
                ),
            ]
        );
    }

    #[test]
    fn write_endpoint_file_is_atomic_0600_and_shell_sourceable() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("agent-hooks");
        assert!(write_endpoint_file(&target, &fields()).unwrap());
        let path = target.join(ENDPOINT_FILE_NAME);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "ORCA_AGENT_HOOK_PORT=43123\n\
             ORCA_AGENT_HOOK_TOKEN=a1b2-c3\n\
             ORCA_AGENT_HOOK_ENV=development\n\
             ORCA_AGENT_HOOK_VERSION=1\n\
             ORCA_AGENT_HOOK_TRANSPORT=raw-json-v1\n"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let leftovers = std::fs::read_dir(&target)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".endpoint-"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn write_endpoint_file_refuses_unsafe_values_without_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut unsafe_fields = fields();
        unsafe_fields.token = "bad token".to_string();
        assert!(!write_endpoint_file(dir.path(), &unsafe_fields).unwrap());
        assert!(!dir.path().join(ENDPOINT_FILE_NAME).exists());
    }

    #[test]
    fn pty_env_supersets_endpoint_lines_with_the_endpoint_path() {
        let env = pty_env(&fields(), Path::new("/data/agent-hooks/endpoint.env"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_PORT").map(String::as_str), Some("43123"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_TOKEN").map(String::as_str), Some("a1b2-c3"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_ENV").map(String::as_str), Some("development"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_VERSION").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("ORCA_AGENT_HOOK_TRANSPORT").map(String::as_str),
            Some("raw-json-v1")
        );
        assert_eq!(
            env.get("ORCA_AGENT_HOOK_ENDPOINT").map(String::as_str),
            Some("/data/agent-hooks/endpoint.env")
        );
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p ade-hooks`（在 `src-tauri/` 下）
Expected: 编译通过但测试 `todo!()` panic（或 `todo` 未实现即红）

- [ ] **Step 4: 实现**

把 `todo!()` 替换为：

```rust
/// 值会被 shell `source`；正则等价 fork `isShellSafeEndpointValue`
/// （规格 §3.5）：空白、引号、`;`、`$` 等一律拒绝，空串也拒绝
/// （防止 `KEY=` 清空已存在的变量）。
pub fn is_shell_safe_value(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '/' | '-'))
}

pub fn endpoint_env_lines(fields: &EndpointFields) -> Vec<(String, String)> {
    vec![
        ("ORCA_AGENT_HOOK_PORT".to_string(), fields.port.to_string()),
        ("ORCA_AGENT_HOOK_TOKEN".to_string(), fields.token.clone()),
        ("ORCA_AGENT_HOOK_ENV".to_string(), fields.env.clone()),
        (
            "ORCA_AGENT_HOOK_VERSION".to_string(),
            HOOK_PROTOCOL_VERSION.to_string(),
        ),
        (
            "ORCA_AGENT_HOOK_TRANSPORT".to_string(),
            HOOK_RAW_JSON_TRANSPORT.to_string(),
        ),
    ]
}

/// 0600 原子写（oracle `writeEndpointFile` 语义，规格 §5.1）：目录 0700、
/// 临时文件 `create_new` + 0600、rename 落位、失败清 tmp。
pub fn write_endpoint_file(dir: &Path, fields: &EndpointFields) -> std::io::Result<bool> {
    let lines = endpoint_env_lines(fields);
    for (key, value) in &lines {
        if !is_shell_safe_value(value) {
            eprintln!(
                "[ade-hooks] refusing to write endpoint file: {key} contains characters unsafe for shell sourcing"
            );
            return Ok(false);
        }
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let final_path = dir.join(ENDPOINT_FILE_NAME);
    let tmp_path = dir.join(format!(
        ".endpoint-{}-{}.tmp",
        std::process::id(),
        ids::new_uuid()
    ));
    let mut contents = lines
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    contents.push('\n');
    let result = (|| -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp_path)?;
            file.write_all(contents.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&tmp_path, contents.as_bytes())?;
        }
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result.map(|_| true)
}

pub fn pty_env(fields: &EndpointFields, endpoint_path: &Path) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = endpoint_env_lines(fields).into_iter().collect();
    env.insert(
        "ORCA_AGENT_HOOK_ENDPOINT".to_string(),
        endpoint_path.to_string_lossy().into_owned(),
    );
    env
}
```

（把 `use std::path::{Path, PathBuf};` 收敛为实际使用的 `Path`；`PathBuf` 未用则删除。）

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p ade-hooks`
Expected: 5 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/crates/ade-hooks
git commit -m "feat(hooks): ade-hooks 基座 + endpoint.env 原子发布与 PTY env 映射

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: ade-hooks 状态缓存（last-status.json）

**Files:**
- Create: `src-tauri/crates/ade-hooks/src/cache.rs`
- Modify: `src-tauri/crates/ade-hooks/src/lib.rs`（加 `pub mod cache;` 与 `pub use`）

**Interfaces:**
- Consumes: 无
- Produces（Task 5/6 依赖）:
  - `CachedHookEvent { source: String, payload: serde_json::Value, pane_key: String, tab_id/worktree_id/launch_token: Option<String>, received_at: i64, restored: bool }`（serde camelCase；`restored` 序列化时省略 false）
  - `StatusCache::load(path: PathBuf) -> StatusCache`（加载时 version/7 天 TTL 校验、条目标 `restored=true`）
  - `StatusCache::record(&self, event: CachedHookEvent)`（250ms trailing 防抖写）
  - `StatusCache::snapshot(&self) -> Vec<CachedHookEvent>`（按 `received_at` 升序）
  - `StatusCache::flush_sync(&self)` / `StatusCache::shutdown(&self)`

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/crates/ade-hooks/src/cache.rs`（实现先 `todo!()`，测试完整）：

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Condvar, Mutex};
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
    inner: Mutex<CacheInner>,
    wake: Condvar,
    stop: AtomicBool,
    writer: Mutex<Option<JoinHandle<()>>>,
}

struct CacheInner {
    entries: HashMap<String, CachedHookEvent>,
    dirty: bool,
}

impl StatusCache {
    pub fn load(path: PathBuf) -> Self {
        todo!()
    }

    pub fn load_with_debounce(path: PathBuf, debounce: Duration) -> Self {
        todo!()
    }

    pub fn record(&self, event: CachedHookEvent) {
        todo!()
    }

    pub fn snapshot(&self) -> Vec<CachedHookEvent> {
        todo!()
    }

    pub fn flush_sync(&self) {
        todo!()
    }

    pub fn shutdown(&self) {
        todo!()
    }
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p ade-hooks`
Expected: cache 测试 panic（`todo!()`）

- [ ] **Step 3: 实现**

在 `lib.rs` 加 `pub mod cache;` 与 `pub use cache::CachedHookEvent;`。cache.rs 的 `StatusCache`/`CacheInner` 字段换成下面 Arc + `last_record_at` 版本（writer 线程需要共享，且 shutdown 不能被长防抖挂住）：

```rust
pub struct StatusCache {
    path: PathBuf,
    debounce: Duration,
    inner: std::sync::Arc<Mutex<CacheInner>>,
    wake: std::sync::Arc<Condvar>,
    stop: std::sync::Arc<AtomicBool>,
    writer: Mutex<Option<JoinHandle<()>>>,
}
```

（头部 `use` 增 `use std::sync::Arc;`。）用以下实现替换 `todo!()`：

```rust
struct CacheInner {
    entries: HashMap<String, CachedHookEvent>,
    dirty: bool,
    /// 最近一次 record 的时刻；防抖到期才落盘，且等待切成 ≤50ms 片，
    /// 让 shutdown 的 join 不会被长 debounce（测试/未来参数）挂住。
    last_record_at: Option<std::time::Instant>,
}
```

```rust
fn start_writer(cache: &StatusCache) {
    let path = cache.path.clone();
    let debounce = cache.debounce;
    let inner = std::sync::Arc::clone(&cache.inner);
    let wake = std::sync::Arc::clone(&cache.wake);
    let stop = std::sync::Arc::clone(&cache.stop);
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
            inner: std::sync::Arc::new(Mutex::new(CacheInner {
                entries,
                dirty: false,
                last_record_at: None,
            })),
            wake: std::sync::Arc::new(Condvar::new()),
            stop: std::sync::Arc::new(AtomicBool::new(false)),
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
```

其余私有函数：

```rust
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
```

`snapshot_sorts_*` 测试显式传 60s 防抖，靠 `record` 内存即时可见（防抖写不参与该断言）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p ade-hooks`
Expected: Task 1 + cache 共 9 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/crates/ade-hooks
git commit -m "feat(hooks): last-status.json 防抖缓存与重启 hydrate（7 天 TTL/restored 标记）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: ade-hooks 共享脚本（claude-hook.sh）

**Files:**
- Create: `src-tauri/crates/ade-hooks/src/script.rs`
- Modify: `src-tauri/crates/ade-hooks/src/lib.rs`（加 `pub mod script;`）

**Interfaces:**
- Consumes: 无
- Produces（Task 4/5/6 依赖）:
  - `script::managed_hooks_dir(home: &str) -> PathBuf`（`~/.ade/agent-hooks`）
  - `script::managed_script_path(home: &str) -> PathBuf`（`~/.ade/agent-hooks/claude-hook.sh`）
  - `script::write_managed_script(home: &str) -> io::Result<PathBuf>`（0755 原子写、幂等）
  - `script::managed_script_contents() -> String`

脚本语义（规格 §3.4，对齐 fork `hook-service.ts`/`hook-stdin-contract.ts`）：无条件 `printf "{}\n"`（PermissionRequest 需要非空 stdout）→ 捕获 stdin（空退出）→ spool 函数（Tool 进度事件不 spool、5MiB 上限、7 天截断、dir 0700/file 0600、单行 JSON）→ source endpoint（先 `unset` transport 防旧值）→ 缺 port/token/paneKey 落 spool 退出 → raw-json 优先、form 回退、curl 失败落 spool。

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/crates/ade-hooks/src/script.rs`：

```rust
use std::io;
use std::path::{Path, PathBuf};

pub fn managed_hooks_dir(home: &str) -> PathBuf {
    Path::new(home).join(".ade").join("agent-hooks")
}

pub fn managed_script_path(home: &str) -> PathBuf {
    managed_hooks_dir(home).join("claude-hook.sh")
}

pub fn managed_script_contents() -> String {
    todo!()
}

pub fn write_managed_script(home: &str) -> io::Result<PathBuf> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn script_starts_with_neutral_stdout_and_captures_stdin_before_anything_else() {
        let script = managed_script_contents();
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines[0], "#!/bin/sh");
        assert_eq!(lines[1], "printf '{}\\n'");
        assert!(script.contains("payload=$( { command -p cat 2>/dev/null || cat; } )"));
        assert!(script.contains("if [ -z \"$payload\" ]; then"));
    }

    #[test]
    fn script_posts_raw_json_with_base64_meta_and_form_fallback() {
        let script = managed_script_contents();
        assert!(script.contains("ORCA_AGENT_HOOK_TRANSPORT:-}\" = \"raw-json-v1\""));
        assert!(script.contains("printf '%s\\037%s\\037%s\\037%s\\037%s\\037%s'"));
        assert!(script.contains("X-Orca-Agent-Hook-Meta-Encoding: base64"));
        assert!(script.contains("--data-urlencode \"payload@-\""));
        assert!(script.contains("unset ORCA_AGENT_HOOK_TRANSPORT"));
        assert!(script.contains(". \"$ORCA_AGENT_HOOK_ENDPOINT\""));
    }

    #[test]
    fn script_spool_skips_tool_progress_and_bounds_the_file() {
        let script = managed_script_contents();
        assert!(script.contains("*'\"PreToolUse\"'*"));
        assert!(script.contains("spool_dir=\"$spool_base/spool\""));
        assert!(script.contains("5242880"));
        assert!(script.contains("-mtime +7"));
        assert!(script.contains("chmod 600 \"$spool_file\""));
    }

    #[test]
    fn write_managed_script_is_executable_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        let first = write_managed_script(home).unwrap();
        assert_eq!(first, managed_script_path(home));
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let before = std::fs::read_to_string(&first).unwrap();
        write_managed_script(home).unwrap();
        assert_eq!(std::fs::read_to_string(&first).unwrap(), before);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p ade-hooks script`
Expected: panic（`todo!()`）

- [ ] **Step 3: 实现**

`lib.rs` 加 `pub mod script;`。`managed_script_contents` 返回常量字符串（用 `concat!` 或 `r#"…"#` 均可，此处用字面量）：

```rust
pub fn managed_script_contents() -> String {
    r#"#!/bin/sh
printf '{}\n'
payload=$( { command -p cat 2>/dev/null || cat; } )
if [ -z "$payload" ]; then
  exit 0
fi
spool_hook_event() {
  case "$payload" in
    *'"PreToolUse"'*|*'"PostToolUse"'*|*'"PostToolUseFailure"'*) return 0 ;;
  esac
  [ -n "${ORCA_AGENT_HOOK_ENDPOINT:-}" ] || return 0
  [ -n "${ORCA_PANE_KEY:-}" ] || return 0
  [ -r "$ORCA_AGENT_HOOK_ENDPOINT" ] || return 0
  spool_base=${ORCA_AGENT_HOOK_ENDPOINT%/*}
  spool_dir="$spool_base/spool"
  mkdir -p "$spool_dir" 2>/dev/null || return 0
  chmod 700 "$spool_dir" 2>/dev/null || :
  spool_id=$(printf %s "${ORCA_PANE_KEY:-unknown}" | tail -c 36 | tr '/:' '__')
  spool_file="$spool_dir/pane-$spool_id.jsonl"
  if [ -f "$spool_file" ] && find "$spool_file" -mtime +7 -print -quit 2>/dev/null | grep -q .; then : > "$spool_file"; fi
  [ -f "$spool_file" ] || : > "$spool_file"
  spool_size=$(wc -c < "$spool_file" 2>/dev/null || printf 0)
  [ "$spool_size" -lt 5242880 ] || return 0
  spool_now=$(date +%s 2>/dev/null || printf 0)
  spool_now=$((spool_now * 1000))
  spool_json_escape() { printf %s "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/[[:cntrl:]]/ /g'; }
  { printf '\n{"paneKey":"%s","tabId":"%s","worktreeId":"%s","env":"%s","version":"%s","launchToken":"%s","source":"claude","receivedAt":%s,"payload":%s}\n' \
    "$(spool_json_escape "${ORCA_PANE_KEY:-}")" \
    "$(spool_json_escape "${ORCA_TAB_ID:-}")" \
    "$(spool_json_escape "${ORCA_WORKTREE_ID:-}")" \
    "$(spool_json_escape "${ORCA_AGENT_HOOK_ENV:-}")" \
    "$(spool_json_escape "${ORCA_AGENT_HOOK_VERSION:-}")" \
    "$(spool_json_escape "${ORCA_AGENT_LAUNCH_TOKEN:-}")" \
    "$spool_now" "$payload"; } >> "$spool_file" 2>/dev/null || :
  chmod 600 "$spool_file" 2>/dev/null || :
}
if [ -n "${ORCA_AGENT_HOOK_ENDPOINT:-}" ] && [ -r "$ORCA_AGENT_HOOK_ENDPOINT" ]; then
  unset ORCA_AGENT_HOOK_TRANSPORT
  . "$ORCA_AGENT_HOOK_ENDPOINT" 2>/dev/null || :
fi
if [ -z "${ORCA_AGENT_HOOK_PORT:-}" ] || [ -z "${ORCA_AGENT_HOOK_TOKEN:-}" ] || [ -z "${ORCA_PANE_KEY:-}" ]; then
  spool_hook_event
  exit 0
fi
if [ "${ORCA_AGENT_HOOK_TRANSPORT:-}" = "raw-json-v1" ] && command -v base64 >/dev/null 2>&1 && command -v tr >/dev/null 2>&1; then
  orca_hook_metadata=$(printf '%s\037%s\037%s\037%s\037%s\037%s' "$ORCA_PANE_KEY" "$ORCA_TAB_ID" "$ORCA_AGENT_LAUNCH_TOKEN" "$ORCA_WORKTREE_ID" "$ORCA_AGENT_HOOK_ENV" "$ORCA_AGENT_HOOK_VERSION" | base64 | tr -d '\n') && \
  [ -n "$orca_hook_metadata" ] && \
  printf '%s' "$payload" | curl -sS -X POST "http://127.0.0.1:${ORCA_AGENT_HOOK_PORT}/hook/claude" \
    --connect-timeout "${connect_timeout:-0.5}" --max-time "${max_time:-1.5}" \
    --noproxy "127.0.0.1" \
    -H "Content-Type: application/json" \
    -H "X-Orca-Agent-Hook-Token: ${ORCA_AGENT_HOOK_TOKEN}" \
    -H "X-Orca-Agent-Hook-Meta-Encoding: base64" \
    -H "X-Orca-Agent-Hook-Meta: ${orca_hook_metadata}" \
    --data-binary @- >/dev/null 2>&1 || spool_hook_event
else
  printf '%s' "$payload" | curl -sS -X POST "http://127.0.0.1:${ORCA_AGENT_HOOK_PORT}/hook/claude" \
    --connect-timeout "${connect_timeout:-0.5}" --max-time "${max_time:-1.5}" \
    --noproxy "127.0.0.1" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -H "X-Orca-Agent-Hook-Token: ${ORCA_AGENT_HOOK_TOKEN}" \
    --data-urlencode "paneKey=${ORCA_PANE_KEY}" \
    --data-urlencode "tabId=${ORCA_TAB_ID}" \
    --data-urlencode "launchToken=${ORCA_AGENT_LAUNCH_TOKEN}" \
    --data-urlencode "worktreeId=${ORCA_WORKTREE_ID}" \
    --data-urlencode "env=${ORCA_AGENT_HOOK_ENV}" \
    --data-urlencode "version=${ORCA_AGENT_HOOK_VERSION}" \
    --data-urlencode "payload@-" >/dev/null 2>&1 || spool_hook_event
fi
exit 0
"#
    .to_string()
}

pub fn write_managed_script(home: &str) -> io::Result<PathBuf> {
    let dir = managed_hooks_dir(home);
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let path = managed_script_path(home);
    let contents = managed_script_contents();
    if std::fs::read_to_string(&path).map(|existing| existing == contents).unwrap_or(false) {
        return Ok(path);
    }
    let tmp = dir.join(format!(".claude-hook-{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}
```

> 注：`r#"…"#` 内 `\037`/`\n` 是脚本字面量（printf 的八进制转义），不会在 Rust 字符串转义；上面的 `\n` 在 `printf '\n{...}'` 处是两字符 `\`+`n`（对 raw string 正确）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p ade-hooks script`
Expected: 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/crates/ade-hooks
git commit -m "feat(hooks): claude-hook.sh 托管脚本（neutral stdout + spool 兜底 + raw/form 双通道）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: ade-hooks claude settings 安装器

**Files:**
- Create: `src-tauri/crates/ade-hooks/src/installer.rs`
- Modify: `src-tauri/crates/ade-hooks/src/lib.rs`（加 `pub mod installer;` 与 `pub use`）

**Interfaces:**
- Consumes: `script::{write_managed_script, managed_script_path}`
- Produces（Task 5/8 依赖）:
  - `installer::{HookInstallState, HookInstallSkipReason, install_claude_hooks, remove_claude_hooks, is_claude_cli_available, claude_settings_path, hooks_installation_present}`
  - `install_claude_hooks(home: &str, enabled: bool, cli_present: bool) -> HookInstallState`
  - `remove_claude_hooks(home: &str) -> HookInstallState`
  - 事件集（12）：SessionStart、UserPromptSubmit、Stop、StopFailure、SubagentStart、SubagentStop、TeammateIdle、PreToolUse(`*`)、PostToolUse(`*`)、PostToolUseFailure(`*`)、PermissionRequest(`*`)、PostCompact（规格 §3.3；不装 Notification/PreCompact）

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/crates/ade-hooks/src/installer.rs`：

```rust
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::script;

pub const MANAGED_HOOK_TIMEOUT_SECONDS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookInstallState {
    Installed,
    Skipped(HookInstallSkipReason),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookInstallSkipReason {
    HooksDisabled,
    CliNotFound,
}

pub fn claude_events() -> Vec<(&'static str, Option<&'static str>)> {
    todo!()
}

pub fn claude_settings_path(home: &str) -> PathBuf {
    Path::new(home).join(".claude").join("settings.json")
}

pub fn managed_command() -> String {
    todo!()
}

pub fn managed_command_matcher(command: &str) -> bool {
    todo!()
}

pub fn apply_managed_hooks(config: &Value) -> Value {
    todo!()
}

pub fn remove_managed_hooks(config: &Value) -> (Value, bool) {
    todo!()
}

pub fn install_claude_hooks(home: &str, enabled: bool, cli_present: bool) -> HookInstallState {
    todo!()
}

pub fn remove_claude_hooks(home: &str) -> HookInstallState {
    todo!()
}

pub fn is_claude_cli_available(home: &str) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed_definitions(value: &Value, event: &str) -> usize {
        value["hooks"][event]
            .as_array()
            .map(|definitions| {
                definitions
                    .iter()
                    .filter(|definition| {
                        definition["hooks"]
                            .as_array()
                            .map(|hooks| {
                                hooks.iter().any(|hook| {
                                    hook["command"]
                                        .as_str()
                                        .map(managed_command_matcher)
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn managed_command_survives_missing_script_with_neutral_json() {
        let command = managed_command();
        assert!(command.contains("agent-hooks/claude-hook.sh"));
        assert!(command.contains("/bin/sh"));
        assert!(command.contains("printf '{}\\n'"));
    }

    #[test]
    fn apply_installs_all_twelve_events_and_preserves_user_entries() {
        let config = json!({
            "hooks": {
                "Stop": [ { "hooks": [ { "type": "command", "command": "/usr/local/bin/user-stop" } ] } ],
                "UserPromptSubmit": [ { "hooks": [ { "type": "command", "command": "echo hi" } ] } ]
            },
            "permissions": { "allow": ["Bash(ls:*)"] }
        });
        let next = apply_managed_hooks(&config);
        for (event, _) in claude_events() {
            assert_eq!(managed_definitions(&next, event), 1, "{event}");
        }
        assert!(next["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .any(|definition| definition["hooks"][0]["command"] == "/usr/local/bin/user-stop"));
        assert_eq!(next["permissions"], config["permissions"]);
        // 幂等：重复 apply 不叠加托管条目。
        assert_eq!(managed_definitions(&apply_managed_hooks(&next), "Stop"), 1);
    }

    #[test]
    fn apply_uses_star_matcher_only_on_tool_and_permission_events() {
        let next = apply_managed_hooks(&json!({}));
        for (event, matcher) in claude_events() {
            let managed = next["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .find(|definition| {
                    definition["hooks"][0]["command"]
                        .as_str()
                        .map(managed_command_matcher)
                        .unwrap_or(false)
                })
                .unwrap();
            assert_eq!(managed.get("matcher").and_then(Value::as_str), matcher, "{event}");
            assert_eq!(managed["hooks"][0]["timeout"], 10);
            assert_eq!(managed["hooks"][0]["type"], "command");
        }
    }

    #[test]
    fn remove_strips_managed_entries_but_keeps_user_entries_and_other_keys() {
        let config = apply_managed_hooks(&json!({
            "hooks": {
                "Stop": [ { "hooks": [ { "type": "command", "command": "/usr/local/bin/user-stop" } ] } ]
            },
            "theme": "dark"
        }));
        let (next, changed) = remove_managed_hooks(&config);
        assert!(changed);
        for (event, _) in claude_events() {
            assert_eq!(managed_definitions(&next, event), 0, "{event}");
        }
        assert!(next["hooks"]["Stop"].as_array().unwrap().iter().any(|definition| {
            definition["hooks"][0]["command"] == "/usr/local/bin/user-stop"
        }));
        assert_eq!(next["theme"], "dark");
        let (_, changed_again) = remove_managed_hooks(&next);
        assert!(!changed_again);
    }

    #[test]
    fn install_skip_paths_never_touch_settings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        let settings_path = claude_settings_path(home);
        assert!(matches!(
            install_claude_hooks(home, false, true),
            HookInstallState::Skipped(HookInstallSkipReason::HooksDisabled)
        ));
        assert!(!settings_path.exists());
        assert!(matches!(
            install_claude_hooks(home, true, false),
            HookInstallState::Skipped(HookInstallSkipReason::CliNotFound)
        ));
        assert!(!settings_path.exists());
    }

    #[test]
    fn install_writes_settings_and_script_with_rolling_backup() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        std::fs::create_dir_all(claude_settings_path(home).parent().unwrap()).unwrap();
        std::fs::write(claude_settings_path(home), r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(
            install_claude_hooks(home, true, true),
            HookInstallState::Installed
        );
        let stored: Value =
            serde_json::from_str(&std::fs::read_to_string(claude_settings_path(home)).unwrap())
                .unwrap();
        assert_eq!(stored["theme"], "dark");
        assert_eq!(managed_definitions(&stored, "Stop"), 1);
        assert!(claude_settings_path(home).with_extension("json.bak").exists());
        assert!(script::managed_script_path(home).exists());
    }

    #[test]
    fn install_dereferences_symlinked_settings_target() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        let real_dir = dir.path().join("dotfiles");
        std::fs::create_dir_all(&real_dir).unwrap();
        let real = real_dir.join("settings.json");
        std::fs::write(&real, "{}").unwrap();
        let link_dir = Path::new(home).join(".claude");
        std::fs::create_dir_all(&link_dir).unwrap();
        let link = link_dir.join("settings.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            install_claude_hooks(home, true, true),
            HookInstallState::Installed
        );
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&real).unwrap()).unwrap();
        assert_eq!(managed_definitions(&stored, "Stop"), 1);
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn install_refuses_to_clobber_malformed_settings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        std::fs::create_dir_all(claude_settings_path(home).parent().unwrap()).unwrap();
        std::fs::write(claude_settings_path(home), "{ not json").unwrap();
        assert!(matches!(
            install_claude_hooks(home, true, true),
            HookInstallState::Error(_)
        ));
        assert_eq!(
            std::fs::read_to_string(claude_settings_path(home)).unwrap(),
            "{ not json"
        );
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p ade-hooks installer`
Expected: panic（`todo!()`）

- [ ] **Step 3: 实现**

`lib.rs` 加：

```rust
pub mod installer;
pub use cache::CachedHookEvent;
pub use installer::{install_claude_hooks, HookInstallSkipReason, HookInstallState};
```

installer.rs 实现：

```rust
pub fn claude_events() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("SessionStart", None),
        ("UserPromptSubmit", None),
        ("Stop", None),
        ("StopFailure", None),
        ("SubagentStart", None),
        ("SubagentStop", None),
        ("TeammateIdle", None),
        ("PreToolUse", Some("*")),
        ("PostToolUse", Some("*")),
        ("PostToolUseFailure", Some("*")),
        ("PermissionRequest", Some("*")),
        ("PostCompact", None),
    ]
}

/// 托管条目的调用串：脚本存在 → `/bin/sh` 执行；缺失/不可读 → 排空 stdin
/// 后回 `{}`（PermissionRequest fail-closed 防线，规格 §3.3/§3.4）。
pub fn managed_command() -> String {
    "if [ -f \"${HOME-}/.ade/agent-hooks/claude-hook.sh\" ] && [ -r \"${HOME-}/.ade/agent-hooks/claude-hook.sh\" ] && [ -x \"${HOME-}/.ade/agent-hooks/claude-hook.sh\" ]; then /bin/sh \"${HOME-}/.ade/agent-hooks/claude-hook.sh\"; else { command -p cat 2>/dev/null || cat; } >/dev/null 2>&1 || :; printf '{}\\n'; fi".to_string()
}

pub fn managed_command_matcher(command: &str) -> bool {
    command.replace('\\', "/").contains("agent-hooks/claude-hook.sh")
}

fn managed_hook_definition(matcher: Option<&str>) -> Value {
    let hook = json!({
        "type": "command",
        "command": managed_command(),
        "timeout": MANAGED_HOOK_TIMEOUT_SECONDS,
    });
    match matcher {
        Some(matcher) => json!({ "matcher": matcher, "hooks": [hook] }),
        None => json!({ "hooks": [hook] }),
    }
}

fn definition_is_managed(definition: &Value) -> bool {
    if let Some(command) = definition.get("command").and_then(Value::as_str) {
        if managed_command_matcher(command) {
            return true;
        }
    }
    definition["hooks"]
        .as_array()
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook["command"]
                    .as_str()
                    .map(managed_command_matcher)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub fn apply_managed_hooks(config: &Value) -> Value {
    let mut next = config.clone();
    let root = next.as_object_mut().expect("settings object");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("hooks object");
    for (event, matcher) in claude_events() {
        let existing = hooks
            .get(event)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut cleaned: Vec<Value> = existing
            .into_iter()
            .filter(|definition| !definition_is_managed(definition))
            .collect();
        cleaned.push(managed_hook_definition(matcher));
        hooks.insert(event.to_string(), Value::Array(cleaned));
    }
    next
}

pub fn remove_managed_hooks(config: &Value) -> (Value, bool) {
    let mut next = config.clone();
    let mut changed = false;
    let Some(root) = next.as_object_mut() else {
        return (next, false);
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return (next, false);
    };
    let event_names: Vec<String> = hooks.keys().cloned().collect();
    for event in event_names {
        let Some(definitions) = hooks.get(&event).and_then(Value::as_array).cloned() else {
            continue;
        };
        let before = definitions.len();
        let cleaned: Vec<Value> = definitions
            .into_iter()
            .filter(|definition| !definition_is_managed(definition))
            .collect();
        if cleaned.len() != before {
            changed = true;
        }
        if cleaned.is_empty() {
            hooks.remove(&event);
        } else {
            hooks.insert(event, Value::Array(cleaned));
        }
    }
    (next, changed)
}

fn read_settings_json(path: &Path) -> io::Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error),
    }
}

/// 写入路径解引用 symlink（dotfiles 管理器断链防线，规格 §3.3）。
fn resolve_write_path(path: &Path) -> io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            std::fs::canonicalize(path).map_err(|error| io::Error::new(io::ErrorKind::NotFound, error))
        }
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(error),
    }
}

fn write_settings_json(path: &Path, value: &Value) -> io::Result<()> {
    let write_path = resolve_write_path(path)?;
    if let Some(parent) = write_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut serialized = serde_json::to_string_pretty(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    serialized.push('\n');
    if std::fs::read_to_string(&write_path)
        .map(|existing| existing == serialized)
        .unwrap_or(false)
    {
        return Ok(());
    }
    let tmp = write_path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, serialized.as_bytes())?;
    if write_path.exists() {
        let backup = PathBuf::from(format!("{}.bak", write_path.to_string_lossy()));
        let _ = std::fs::copy(&write_path, &backup);
    }
    std::fs::rename(&tmp, &write_path)?;
    Ok(())
}

pub fn install_claude_hooks(home: &str, enabled: bool, cli_present: bool) -> HookInstallState {
    if !enabled {
        return HookInstallState::Skipped(HookInstallSkipReason::HooksDisabled);
    }
    if !cli_present {
        return HookInstallState::Skipped(HookInstallSkipReason::CliNotFound);
    }
    if let Err(error) = script::write_managed_script(home) {
        return HookInstallState::Error(format!("failed to write managed hook script: {error}"));
    }
    let path = claude_settings_path(home);
    let config = match read_settings_json(&path) {
        Ok(config) => config,
        Err(error) => return HookInstallState::Error(format!("failed to read claude settings: {error}")),
    };
    let next = apply_managed_hooks(&config);
    match write_settings_json(&path, &next) {
        Ok(()) => HookInstallState::Installed,
        Err(error) => HookInstallState::Error(format!("failed to write claude settings: {error}")),
    }
}

/// 显式关闭开关 = 移除托管条目（脚本保留）；启动期关闭 = 从不调用本函数
/// （规格 §3.3 与 §4 裁定：启动 skip 不删防多 profile 互删，显式 toggle 才删）。
pub fn remove_claude_hooks(home: &str) -> HookInstallState {
    let path = claude_settings_path(home);
    let config = match read_settings_json(&path) {
        Ok(config) => config,
        Err(error) => return HookInstallState::Error(format!("failed to read claude settings: {error}")),
    };
    let (next, _) = remove_managed_hooks(&config);
    match write_settings_json(&path, &next) {
        Ok(()) => HookInstallState::Installed,
        Err(error) => HookInstallState::Error(format!("failed to write claude settings: {error}")),
    }
}

pub fn is_claude_cli_available(home: &str) -> bool {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    for extra in [".local/bin", ".claude/local", ".bun/bin", ".npm-global/bin"] {
        dirs.push(Path::new(home).join(extra));
    }
    dirs.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ]);
    dirs.into_iter().any(|dir| is_executable_file(&dir.join("claude")))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p ade-hooks installer`
Expected: 8 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/crates/ade-hooks
git commit -m "feat(hooks): claude settings 托管条目写入器（12 事件/backup/symlink 解引用/skip 语义）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

### Task 5: ade-hooks HTTP server + 归因 + spool 重放

**Files:**
- Create: `src-tauri/crates/ade-hooks/src/server.rs`
- Modify: `src-tauri/crates/ade-hooks/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`（根 `[dependencies]` 增 `ade-hooks = { path = "crates/ade-hooks" }`，供 `src/lib.rs` 直接引用；Task 6 用）

**Interfaces:**
- Consumes: `endpoint::{write_endpoint_file, pty_env, EndpointFields}`、`cache::{CachedHookEvent, StatusCache, now_ms}`、`script::managed_script_path`、`installer::{install_claude_hooks, remove_claude_hooks, is_claude_cli_available, HookInstallState}`
- Produces（Task 6/7/8 依赖）:
  - `AgentHookServer::start(options: StartOptions, callback: HookCallback) -> Arc<AgentHookServer>`
  - `StartOptions { app_data_dir: PathBuf, home: String, env: String, install_enabled: bool }`（`app_data_dir` 即 `<app_data>/agent-hooks`）
  - `AgentHookServer::{port, token, active, pty_env, snapshot, shutdown, set_hooks_enabled, install_state}`
  - HTTP 契约（规格 §3.2/§4）：`/hook/claude` 之外的路径 404、非 POST 与 token 失败 403、body ≤1MB（413）、5s slowloris 断连、成功/畸形载荷 204（fail-open）
  - 归因：`x-orca-agent-hook-meta`(base64, `\x1f` 六段) 或单头/表单字段；空 paneKey 计数丢弃

- [ ] **Step 1: 写失败测试（纯函数 + 端到端 raw TCP）**

创建 `src-tauri/crates/ade-hooks/src/server.rs`（先写测试与签名）：

```rust
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use base64::Engine;
use serde_json::Value;

use crate::cache::{now_ms, CachedHookEvent, StatusCache};
use crate::endpoint::{self, EndpointFields};
use crate::installer::{self, HookInstallState};

pub const HOOK_REQUEST_MAX_BYTES: usize = 1_000_000;
pub const HOOK_REQUEST_SLOWLORIS: Duration = Duration::from_secs(5);
const MAX_HEADER_BYTES: usize = 64 * 1024;
const ACCEPT_POLL: Duration = Duration::from_millis(10);

pub type HookCallback = Box<dyn Fn(CachedHookEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct StartOptions {
    pub app_data_dir: PathBuf,
    pub home: String,
    pub env: String,
    pub install_enabled: bool,
}

pub struct AgentHookServer {
    port: u16,
    token: String,
    env: String,
    endpoint_path: PathBuf,
    pty_env_map: HashMap<String, String>,
    cache: Arc<StatusCache>,
    callback: Arc<dyn Fn(CachedHookEvent) + Send + Sync>,
    stop: Arc<AtomicBool>,
    accept_thread: Mutex<Option<JoinHandle<()>>>,
    install_state: Mutex<HookInstallState>,
    active: bool,
}

enum RequestError {
    Malformed,
    TooLarge,
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_test_server(dir: &Path, callback: HookCallback) -> Arc<AgentHookServer> {
        AgentHookServer::start(
            StartOptions {
                app_data_dir: dir.to_path_buf(),
                home: dir.join("home").to_string_lossy().into_owned(),
                env: "development".to_string(),
                install_enabled: false,
            },
            callback,
        )
    }

    fn request(
        port: u16,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n");
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        (status, response)
    }

    fn captured() -> (Arc<Mutex<Vec<CachedHookEvent>>>, HookCallback) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        (
            events,
            Box::new(move |event| sink.lock().unwrap().push(event)),
        )
    }

    #[test]
    fn posts_authenticated_raw_json_with_packed_meta_and_forwards() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        assert!(server.active);
        assert!(server.port > 0);
        let pane_key = "t1:123e4567-e89b-42d3-a456-426614174000";
        let meta = [pane_key, "t1", "tok-1", "r1::/wt", "development", "1"].join("\u{1f}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(meta);
        let body = br#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#;
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[
                ("Content-Type", "application/json"),
                ("X-Orca-Agent-Hook-Token", &server.token),
                ("X-Orca-Agent-Hook-Meta-Encoding", "base64"),
                ("X-Orca-Agent-Hook-Meta", &encoded),
            ],
            body,
        );
        assert_eq!(status, 204);
        let forwarded = events.lock().unwrap();
        assert_eq!(forwarded.len(), 1);
        assert_eq!(forwarded[0].pane_key, pane_key);
        assert_eq!(forwarded[0].tab_id.as_deref(), Some("t1"));
        assert_eq!(forwarded[0].worktree_id.as_deref(), Some("r1::/wt"));
        assert_eq!(forwarded[0].launch_token.as_deref(), Some("tok-1"));
        assert_eq!(forwarded[0].source, "claude");
        assert_eq!(forwarded[0].payload["hook_event_name"], "Stop");
        assert!(!forwarded[0].restored);
        server.shutdown();
    }

    #[test]
    fn accepts_form_fallback_and_single_headers() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let body = "paneKey=t1%3A123e4567-e89b-42d3-a456-426614174000&tabId=t1&payload=%7B%22hook_event_name%22%3A%22UserPromptSubmit%22%7D";
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("X-Orca-Agent-Hook-Token", &server.token),
            ],
            body.as_bytes(),
        );
        assert_eq!(status, 204);
        assert_eq!(events.lock().unwrap()[0].payload["hook_event_name"], "UserPromptSubmit");
        // 单头回退（无 form 元数据）。
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[
                ("Content-Type", "application/json"),
                ("X-Orca-Agent-Hook-Token", &server.token),
                ("X-Orca-Pane-Key", "t2:123e4567-e89b-42d3-a456-426614174000"),
                ("X-Orca-Tab-Id", "t2"),
            ],
            br#"{"hook_event_name":"Stop"}"#,
        );
        assert_eq!(status, 204);
        assert_eq!(events.lock().unwrap()[1].pane_key, "t2:123e4567-e89b-42d3-a456-426614174000");
        server.shutdown();
    }

    #[test]
    fn rejects_wrong_token_non_post_and_unknown_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[("X-Orca-Agent-Hook-Token", "wrong")],
            b"{}",
        );
        assert_eq!(status, 403);
        let (status, _) = request(server.port, "GET", "/hook/claude", &[], b"");
        assert_eq!(status, 403);
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/codex",
            &[("X-Orca-Agent-Hook-Token", &server.token)],
            b"{}",
        );
        assert_eq!(status, 404);
        assert!(events.lock().unwrap().is_empty());
        server.shutdown();
    }

    #[test]
    fn rejects_oversized_body_with_413_and_drops_empty_pane_key() {
        let dir = tempfile::tempdir().unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let oversized = format!(
            "POST /hook/claude HTTP/1.1\r\nHost: x\r\nX-Orca-Agent-Hook-Token: {}\r\nContent-Length: {}\r\n\r\n",
            server.token,
            HOOK_REQUEST_MAX_BYTES + 1
        )
        .into_bytes();
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        stream.write_all(&oversized).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 413"));
        // 空 paneKey：归因失败计数丢弃，回 204（fail-open）。
        let (status, _) = request(
            server.port,
            "POST",
            "/hook/claude",
            &[("X-Orca-Agent-Hook-Token", &server.token)],
            b"{\"hook_event_name\":\"Stop\"}",
        );
        assert_eq!(status, 204);
        assert!(events.lock().unwrap().is_empty());
        server.shutdown();
    }

    #[test]
    fn snapshot_and_cache_survive_restart_and_spool_replays_on_start() {
        let dir = tempfile::tempdir().unwrap();
        {
            let (_, callback) = captured();
            let server = start_test_server(dir.path(), callback);
            let meta = ["t9:123e4567-e89b-42d3-a456-426614174000", "t9", "", "", "development", "1"].join("\u{1f}");
            let encoded = base64::engine::general_purpose::STANDARD.encode(meta);
            let (status, _) = request(
                server.port,
                "POST",
                "/hook/claude",
                &[
                    ("X-Orca-Agent-Hook-Token", &server.token),
                    ("X-Orca-Agent-Hook-Meta-Encoding", "base64"),
                    ("X-Orca-Agent-Hook-Meta", &encoded),
                ],
                br#"{"hook_event_name":"Stop"}"#,
            );
            assert_eq!(status, 204);
            server.shutdown();
        }
        // spool 重放：模拟脚本落盘的行。
        let spool = dir.path().join("spool");
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::write(
            spool.join("pane-t9.jsonl"),
            "{\"paneKey\":\"t8:123e4567-e89b-42d3-a456-426614174000\",\"tabId\":\"t8\",\"worktreeId\":\"\",\"env\":\"development\",\"version\":\"1\",\"launchToken\":\"\",\"source\":\"claude\",\"receivedAt\":1,\"payload\":{\"hook_event_name\":\"Stop\"}}\n",
        )
        .unwrap();
        let (events, callback) = captured();
        let server = start_test_server(dir.path(), callback);
        let snapshot = server.snapshot();
        assert_eq!(snapshot.len(), 2);
        let hydrated = snapshot
            .iter()
            .find(|entry| entry.pane_key.starts_with("t9:"))
            .unwrap();
        let replayed = snapshot
            .iter()
            .find(|entry| entry.pane_key.starts_with("t8:"))
            .unwrap();
        // 磁盘 hydrate 的行标 restored；spool 重放的行是「迟到送达」，不标。
        assert!(hydrated.restored);
        assert!(!replayed.restored);
        assert!(events.lock().unwrap().iter().any(|entry| entry.pane_key.starts_with("t8:")));
        server.shutdown();
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p ade-hooks server`
Expected: 编译失败或 `todo!()` panic

- [ ] **Step 3: 实现**

`lib.rs` 加：

```rust
pub mod server;
pub use cache::CachedHookEvent;
pub use installer::{install_claude_hooks, HookInstallSkipReason, HookInstallState};
pub use server::{AgentHookServer, HookCallback, StartOptions};
```

server.rs 追加实现（放在 `tests` 模块之前）：

```rust
type SharedCallback = Arc<dyn Fn(CachedHookEvent) + Send + Sync>;

impl AgentHookServer {
    pub fn start(options: StartOptions, callback: HookCallback) -> Arc<Self> {
        let token = ade_core::ids::new_uuid();
        let cache = Arc::new(StatusCache::load(options.app_data_dir.join("last-status.json")));
        let callback: SharedCallback = Arc::from(callback);
        let endpoint_path = options.app_data_dir.join(endpoint::ENDPOINT_FILE_NAME);

        // 启动序（规格 §3.1）：hydrate（在 load 内）→ spool 重放 → bind →
        // settings reconcile。spool 重放先于 bind，避免与实时 POST 竞争。
        drain_spool(&options.app_data_dir.join("spool"), &cache, &callback);

        // 安装策略（规格 §3.3/§4 裁定）：启动关闭 = skip 不删；开启 = 安装/更新。
        let install_state = if options.install_enabled {
            let cli_present = installer::is_claude_cli_available(&options.home);
            installer::install_claude_hooks(&options.home, true, cli_present)
        } else {
            HookInstallState::Skipped(installer::HookInstallSkipReason::HooksDisabled)
        };

        let listener = bind_with_retry(3);
        let (port, active) = match listener.as_ref() {
            Some(listener) => {
                let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
                (port, true)
            }
            None => {
                eprintln!("[ade-hooks] failed to bind loopback after 3 attempts; hook server degraded (2A transcript fallback stays in charge)");
                (0, false)
            }
        };
        let fields = EndpointFields {
            port,
            token: token.clone(),
            env: options.env.clone(),
        };
        let endpoint_written = active
            && endpoint::write_endpoint_file(&options.app_data_dir, &fields).unwrap_or(false);
        let pty_env_map = if endpoint_written {
            endpoint::pty_env(&fields, &endpoint_path)
        } else {
            HashMap::new()
        };

        let server = Arc::new(AgentHookServer {
            port,
            token,
            env: options.env.clone(),
            endpoint_path,
            pty_env_map,
            cache,
            callback,
            stop: Arc::new(AtomicBool::new(false)),
            accept_thread: Mutex::new(None),
            install_state: Mutex::new(install_state),
            active,
        });

        if let Some(listener) = listener {
            let accept_server = Arc::clone(&server);
            let handle = thread::spawn(move || {
                listener
                    .set_nonblocking(true)
                    .expect("set nonblocking listener");
                while !accept_server.stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _addr)) => {
                            let connection_server = Arc::clone(&accept_server);
                            thread::spawn(move || {
                                let _ = connection_server.handle_connection(stream);
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(ACCEPT_POLL);
                        }
                        Err(error) => {
                            eprintln!("[ade-hooks] accept failed: {error}");
                            thread::sleep(ACCEPT_POLL);
                        }
                    }
                }
            });
            *server.accept_thread.lock().unwrap() = Some(handle);
        }
        server
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn env(&self) -> &str {
        &self.env
    }

    pub fn endpoint_path(&self) -> &Path {
        &self.endpoint_path
    }

    pub fn pty_env(&self) -> HashMap<String, String> {
        self.pty_env_map.clone()
    }

    pub fn snapshot(&self) -> Vec<CachedHookEvent> {
        self.cache.snapshot()
    }

    pub fn install_state(&self) -> HookInstallState {
        self.install_state.lock().unwrap().clone()
    }

    /// 显式开关切换（settings 写路径）：开 = 安装/更新；关 = 移除托管条目。
    pub fn set_hooks_enabled(&self, enabled: bool, home: &str) {
        let next = if enabled {
            let cli_present = installer::is_claude_cli_available(home);
            installer::install_claude_hooks(home, true, cli_present)
        } else {
            installer::remove_claude_hooks(home)
        };
        *self.install_state.lock().unwrap() = next;
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.cache.shutdown();
    }

    fn handle_connection(&self, stream: TcpStream) -> Result<(), RequestError> {
        let mut stream = stream;
        stream
            .set_read_timeout(Some(HOOK_REQUEST_SLOWLORIS))
            .map_err(|_| RequestError::Malformed)?;
        // 同一个 BufReader 读写——跨两个 reader 会让头解析的预读字节丢掉。
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|_| RequestError::Malformed)?,
        );
        let (method, path, headers) = read_request_head(&mut reader)?;
        // 规格 §4：非 POST 与 token 失败都按鉴权失败计（403）；未知路由 404。
        if method != "POST" {
            respond(&mut stream, 403);
            return Ok(());
        }
        if path != "/hook/claude" {
            respond(&mut stream, 404);
            return Ok(());
        }
        let Some(token) = headers.get("x-orca-agent-hook-token") else {
            respond(&mut stream, 403);
            return Ok(());
        };
        if token != &self.token {
            respond(&mut stream, 403);
            return Ok(());
        }
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        if content_length > HOOK_REQUEST_MAX_BYTES {
            respond(&mut stream, 413);
            return Ok(());
        }
        let body = read_body(&mut reader, content_length)?;
        let event = build_cached_event(&headers, &body);
        match event {
            Some(event) => {
                self.cache.record(event.clone());
                (self.callback)(event);
                respond(&mut stream, 204);
            }
            None => respond(&mut stream, 204),
        }
        Ok(())
    }
}

fn bind_with_retry(attempts: u32) -> Option<TcpListener> {
    for _ in 0..attempts {
        match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => return Some(listener),
            Err(error) => {
                eprintln!("[ade-hooks] bind attempt failed: {error}");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    None
}

fn read_request_head(
    reader: &mut BufReader<TcpStream>,
) -> Result<(String, String, HashMap<String, String>), RequestError> {
    let mut raw = String::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|_| RequestError::Timeout)?;
        if read == 0 {
            return Err(RequestError::Malformed);
        }
        if raw.len() + read > MAX_HEADER_BYTES {
            return Err(RequestError::TooLarge);
        }
        raw.push_str(&line);
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    let mut lines = raw.split("\r\n");
    let request_line = lines.next().ok_or(RequestError::Malformed)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(RequestError::Malformed)?.to_string();
    let path = parts.next().ok_or(RequestError::Malformed)?.to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok((method, path, headers))
}

fn read_body(reader: &mut BufReader<TcpStream>, content_length: usize) -> Result<Vec<u8>, RequestError> {
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|_| RequestError::Timeout)?;
    Ok(body)
}

fn respond(stream: &mut TcpStream, status: u16) {
    let reason = match status {
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "No Content",
    };
    let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.flush();
}

fn meta_attribution(headers: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let encoding = headers
        .get("x-orca-agent-hook-meta-encoding")
        .map(|value| value.trim().to_ascii_lowercase());
    if encoding.as_deref() == Some("base64") {
        if let Some(decoded) = headers
            .get("x-orca-agent-hook-meta")
            .and_then(|value| decode_base64_header(value))
        {
            let fields: Vec<&str> = decoded.split('\u{1f}').collect();
            if fields.len() == 6 && !fields[0].is_empty() {
                for (key, value) in [
                    ("paneKey", fields[0]),
                    ("tabId", fields[1]),
                    ("launchToken", fields[2]),
                    ("worktreeId", fields[3]),
                    ("env", fields[4]),
                    ("version", fields[5]),
                ] {
                    if !value.is_empty() {
                        out.insert(key.to_string(), value.to_string());
                    }
                }
                return out;
            }
        }
    }
    for (header, key) in [
        ("x-orca-pane-key", "paneKey"),
        ("x-orca-tab-id", "tabId"),
        ("x-orca-launch-token", "launchToken"),
        ("x-orca-worktree-id", "worktreeId"),
        ("x-orca-agent-hook-env", "env"),
        ("x-orca-agent-hook-version", "version"),
    ] {
        if let Some(value) = headers.get(header).filter(|value| !value.is_empty()) {
            out.insert(key.to_string(), value.clone());
        }
    }
    out
}

fn decode_base64_header(value: &str) -> Option<String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value.as_bytes())
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let normalized_input = value.trim_end_matches('=');
    let round_trip = base64::engine::general_purpose::STANDARD
        .encode(text.as_bytes());
    if round_trip.trim_end_matches('=') == normalized_input {
        Some(text)
    } else {
        None
    }
}

fn parse_form_urlencoded(input: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in input.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(percent_decode(key), percent_decode(value));
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&input[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn build_cached_event(headers: &HashMap<String, String>, body: &[u8]) -> Option<CachedHookEvent> {
    let is_form = headers
        .get("content-type")
        .map(|value| value.contains("application/x-www-form-urlencoded"))
        .unwrap_or(false);
    let (payload, attribution) = if is_form {
        let fields = parse_form_urlencoded(&String::from_utf8_lossy(body));
        let payload: Value = serde_json::from_str(fields.get("payload")?).ok()?;
        (payload, fields)
    } else {
        let payload: Value = serde_json::from_slice(body).ok()?;
        (payload, meta_attribution(headers))
    };
    let pane_key = attribution.get("paneKey").cloned().unwrap_or_default();
    if pane_key.trim().is_empty() {
        eprintln!("[ade-hooks] dropping hook event with empty paneKey");
        return None;
    }
    Some(CachedHookEvent {
        source: "claude".to_string(),
        payload,
        pane_key,
        tab_id: attribution.get("tabId").cloned(),
        worktree_id: attribution.get("worktreeId").cloned(),
        launch_token: attribution.get("launchToken").cloned(),
        received_at: now_ms(),
        restored: false,
    })
}

/// spool 重放（规格 §3.4）：启动时 drain，失败行保留文件下轮再试；成功则清空。
fn drain_spool(dir: &Path, cache: &Arc<StatusCache>, callback: &SharedCallback) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut all_parsed = true;
        for line in raw.lines().filter(|line| !line.trim().is_empty()) {
            match serde_json::from_str::<CachedHookEvent>(line) {
                Ok(mut event) => {
                    event.restored = false;
                    cache.record(event.clone());
                    callback(event);
                }
                Err(_) => {
                    all_parsed = false;
                }
            }
        }
        if all_parsed {
            let _ = std::fs::write(&path, "");
        }
    }
}
```

删除 `script` 未使用的 import（`managed_script_path` 不需要；Task 4 已在 installer 内部写脚本）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p ade-hooks`
Expected: 全部 PASS（server 6 个）

- [ ] **Step 5: 根 Cargo 挂 ade-hooks**

`src-tauri/Cargo.toml` `[dependencies]` 增：

```toml
ade-hooks = { path = "crates/ade-hooks" }
```

Run: `cargo check --workspace`
Expected: exit 0（根包此时尚未使用 ade-hooks，无影响；Task 6 使用）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/crates/ade-hooks
git commit -m "feat(hooks): HTTP server（token/路由/1MB/slowloris）+ 归因 + spool 重放

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: ade-bridge 接线（事件 + 启动 + 快照命令 + bindings）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/Cargo.toml`
- Modify: `src-tauri/crates/ade-bridge/src/events.rs`
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`
- Create: `src-tauri/crates/ade-bridge/src/commands/agent_status.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Modify: `src-tauri/src/lib.rs`（Exit 收尾加 hooks.shutdown）
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: `ade_hooks::{AgentHookServer, StartOptions, CachedHookEvent}`、`AppState::{settings_store, home, app}`
- Produces:
  - `events::AGENT_HOOK_RAW = "agent-hook:raw"`；`events::AgentHookRawPayload { source, payload, paneKey, tabId?, worktreeId?, launchToken?, receivedAt, restored }`（camelCase，`restored` false 省略）
  - `AppState.hooks: Arc<ade_hooks::AgentHookServer>`
  - 命令 `agent_status_get_snapshot() -> Vec<AgentHookSnapshotEntry>`
  - 设置读取：`agentStatusHooksEnabled`（缺省 true）在启动时决定安装 reconcile

- [ ] **Step 1: 写失败测试**

在 `events.rs` `mod tests` 追加：

```rust
    #[test]
    fn agent_hook_raw_payload_serializes_camel_case_and_omits_false_restored() {
        let payload = AgentHookRawPayload::from(ade_hooks::CachedHookEvent {
            source: "claude".to_string(),
            payload: json!({ "hook_event_name": "Stop" }),
            pane_key: "t1:leaf".to_string(),
            tab_id: Some("t1".to_string()),
            worktree_id: None,
            launch_token: None,
            received_at: 42,
            restored: false,
        });
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            json!({
                "source": "claude",
                "payload": { "hook_event_name": "Stop" },
                "paneKey": "t1:leaf",
                "tabId": "t1",
                "receivedAt": 42
            })
        );
        assert_eq!(AGENT_HOOK_RAW, "agent-hook:raw");
    }
```

在 `commands/agent_status.rs` 写：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_entry_serializes_camel_case_with_restored_flag() {
        let entry = AgentHookSnapshotEntry::from(ade_hooks::CachedHookEvent {
            source: "claude".to_string(),
            payload: serde_json::json!({ "hook_event_name": "Stop" }),
            pane_key: "t1:leaf".to_string(),
            tab_id: None,
            worktree_id: None,
            launch_token: None,
            received_at: 7,
            restored: true,
        });
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({
                "source": "claude",
                "payload": { "hook_event_name": "Stop" },
                "paneKey": "t1:leaf",
                "receivedAt": 7,
                "restored": true
            })
        );
    }
}
```

在 `specta_export.rs` `export_lists_every_command` 清单里加 `"agent_status_get_snapshot"`（`collect_commands!` 与 `.typ::<commands::events::AgentHookRawPayload>()` 在 Step 3 加）。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-bridge`
Expected: 编译失败（类型/命令缺失）

- [ ] **Step 3: 实现**

`src-tauri/crates/ade-bridge/Cargo.toml` 增：

```toml
ade-hooks = { path = "../ade-hooks" }
```

`events.rs`：

```rust
/// hook server 原始事件（归一化在 renderer，规格 §3.2）。
pub const AGENT_HOOK_RAW: &str = "agent-hook:raw";

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookRawPayload {
    pub source: String,
    pub payload: serde_json::Value,
    pub pane_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_token: Option<String>,
    pub received_at: i64,
    #[serde(skip_serializing_if = "restored_is_false")]
    pub restored: bool,
}

fn restored_is_false(value: &bool) -> bool {
    !*value
}

impl From<ade_hooks::CachedHookEvent> for AgentHookRawPayload {
    fn from(event: ade_hooks::CachedHookEvent) -> Self {
        Self {
            source: event.source,
            payload: event.payload,
            pane_key: event.pane_key,
            tab_id: event.tab_id,
            worktree_id: event.worktree_id,
            launch_token: event.launch_token,
            received_at: event.received_at,
            restored: event.restored,
        }
    }
}
```

`commands/mod.rs` 增 `pub mod agent_status;`；新建 `commands/agent_status.rs`：

```rust
use serde::Serialize;
use tauri::State;

use crate::errors::BridgeError;
use crate::state::AppState;

/// `agent_status_get_snapshot` 元素：与 `AgentHookRawPayload` 同形，
/// `restored` 显式携带（hydrate 回放标记，规格 §3.6/§3.7）。
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookSnapshotEntry {
    pub source: String,
    pub payload: serde_json::Value,
    pub pane_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_token: Option<String>,
    pub received_at: i64,
    pub restored: bool,
}

impl From<ade_hooks::CachedHookEvent> for AgentHookSnapshotEntry {
    fn from(event: ade_hooks::CachedHookEvent) -> Self {
        Self {
            source: event.source,
            payload: event.payload,
            pane_key: event.pane_key,
            tab_id: event.tab_id,
            worktree_id: event.worktree_id,
            launch_token: event.launch_token,
            received_at: event.received_at,
            restored: event.restored,
        }
    }
}

/// 重启后 renderer hydration 的原始缓存快照（升序；归一化在 renderer）。
#[tauri::command]
#[specta::specta]
pub async fn agent_status_get_snapshot(
    state: State<'_, AppState>,
) -> Result<Vec<AgentHookSnapshotEntry>, BridgeError> {
    Ok(state
        .hooks
        .snapshot()
        .into_iter()
        .map(AgentHookSnapshotEntry::from)
        .collect())
}
```

`state.rs`：

1. `use` 区加 `use ade_hooks::{AgentHookServer, StartOptions};`（文件顶部已有 `use serde_json::Value` 等，按现有风格）。
2. `AppState` 结构体在 `pub session: Arc<Store>,` 后加：

```rust
    /// hook server（规格 §3.1；Task 6）：HTTP 接收 + endpoint 发布 + 状态缓存。
    pub hooks: Arc<AgentHookServer>,
```

3. `initialize` 中 `let settings = Arc::new(Mutex::new(persisted.settings));` 之后（`load_persisted_state` 已带回 defaults ∪ stored，`agentStatusHooksEnabled` 默认 true 已在 defaults 里）加：

```rust
        let hooks_enabled = lock(&settings)
            .get()
            .get("agentStatusHooksEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let hooks = {
            let app_handle = app.clone();
            AgentHookServer::start(
                StartOptions {
                    app_data_dir: data_dir.join("agent-hooks"),
                    home: home.clone(),
                    env: if cfg!(debug_assertions) {
                        "development".to_string()
                    } else {
                        "production".to_string()
                    },
                    // 启动策略（规格 §3.3）：关闭 = skip 不删；开启 = 安装/更新。
                    install_enabled: hooks_enabled,
                },
                Box::new(move |event| {
                    events::emit_json(&app_handle, events::AGENT_HOOK_RAW, events::AgentHookRawPayload::from(event));
                }),
            )
        };
```

4. `Ok(Self { ... })` 里 `session,` 后加 `hooks,`。

`src-tauri/src/lib.rs` `RunEvent::Exit` 块内、`state.pty_host.shutdown_all();` 之前加：

```rust
                // hook 缓存/安装器收尾（规格 §3.1）。
                state.hooks.shutdown();
```

`specta_export.rs`：`collect_commands!` 在 `commands::agent_sessions::agent_sessions_resolve_capture,` 后加 `commands::agent_status::agent_status_get_snapshot,`；`.typ` 链上加 `.typ::<crate::events::AgentHookRawPayload>()` 与 `.typ::<commands::agent_status::AgentHookSnapshotEntry>()`；`export_lists_every_command` 清单加 `"agent_status_get_snapshot"`。

- [ ] **Step 4: 重新生成 bindings**

Run: `cargo run -p ade-bridge --bin export-bindings`（在 `src-tauri/` 下）
Expected: `src/bridge/real/generated/tauri-bindings.ts` 出现 `agent_status_get_snapshot` 与两个新类型

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p ade-bridge`
Expected: PASS（含 `bindings_are_fresh`/`export_lists_every_command`）

- [ ] **Step 6: Commit**

```bash
git add src-tauri src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): hook server 启动/退出接线 + agent-hook:raw 事件 + agent_status_get_snapshot

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: ade-bridge pty env 注入（tabId/leafId 接真）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/pty.rs`

**Interfaces:**
- Consumes: `AppState.hooks.pty_env()`（Task 6）、`PtySpawnArgs` 新增字段
- Produces:
  - `PtySpawnArgs { tab_id: Option<String>, leaf_id: Option<String>, launch_token: Option<String>, ... }`
  - `build_spawn_env(base, hooks_env, tab_id, leaf_id, worktree_id, launch_token) -> HashMap<String,String>`
  - spawn env 追加：`ORCA_AGENT_HOOK_{PORT,TOKEN,ENV,VERSION,TRANSPORT,ENDPOINT}`、`ORCA_PANE_KEY=${tabId}:${leafId}`、`ORCA_TAB_ID`、`ORCA_WORKTREE_ID`、`ORCA_AGENT_LAUNCH_TOKEN`

- [ ] **Step 1: 写失败测试**

在 `commands/pty.rs` `mod tests` 追加：

```rust
    #[test]
    fn spawn_args_read_tab_leaf_and_launch_token() {
        let args: PtySpawnArgs = serde_json::from_value(json!({
            "cols": 80,
            "rows": 24,
            "tabId": "t1",
            "leafId": "123e4567-e89b-42d3-a456-426614174000",
            "launchToken": "tok-9"
        }))
        .expect("deserialize spawn args");
        assert_eq!(args.tab_id.as_deref(), Some("t1"));
        assert_eq!(
            args.leaf_id.as_deref(),
            Some("123e4567-e89b-42d3-a456-426614174000")
        );
        assert_eq!(args.launch_token.as_deref(), Some("tok-9"));
    }

    #[test]
    fn build_spawn_env_injects_hook_endpoint_and_pane_identity() {
        let base = HashMap::from([("K".to_string(), "V".to_string())]);
        let hooks = HashMap::from([
            ("ORCA_AGENT_HOOK_PORT".to_string(), "43123".to_string()),
            ("ORCA_AGENT_HOOK_TOKEN".to_string(), "tok".to_string()),
        ]);
        let env = build_spawn_env(
            base,
            &hooks,
            Some("t1"),
            Some("123e4567-e89b-42d3-a456-426614174000"),
            Some("r1::/wt"),
            Some("launch-1"),
        );
        assert_eq!(env.get("K").map(String::as_str), Some("V"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_PORT").map(String::as_str), Some("43123"));
        assert_eq!(
            env.get("ORCA_PANE_KEY").map(String::as_str),
            Some("t1:123e4567-e89b-42d3-a456-426614174000")
        );
        assert_eq!(env.get("ORCA_TAB_ID").map(String::as_str), Some("t1"));
        assert_eq!(env.get("ORCA_WORKTREE_ID").map(String::as_str), Some("r1::/wt"));
        assert_eq!(env.get("ORCA_AGENT_LAUNCH_TOKEN").map(String::as_str), Some("launch-1"));
    }

    #[test]
    fn build_spawn_env_skips_pane_key_without_both_ids_and_overrides_renderer_env() {
        let base = HashMap::from([("ORCA_PANE_KEY".to_string(), "stale".to_string())]);
        let hooks = HashMap::from([("ORCA_AGENT_HOOK_PORT".to_string(), "1".to_string())]);
        let env = build_spawn_env(base, &hooks, Some("t1"), None, None, None);
        assert_eq!(env.get("ORCA_PANE_KEY").map(String::as_str), Some("stale"));
        let env = build_spawn_env(
            HashMap::new(),
            &hooks,
            Some("t1"),
            Some("leaf-1"),
            None,
            None,
        );
        assert_eq!(env.get("ORCA_PANE_KEY").map(String::as_str), Some("t1:leaf-1"));
        assert_eq!(env.get("ORCA_AGENT_HOOK_PORT").map(String::as_str), Some("1"));
    }
```

并把 `spawn_args_ignore_contract_only_fields_and_default_missing` 的 fixture 中 `"tabId": "t1", "leafId": "l1",` 两行删除（它们已不是 contract-only 字段），保留 `"launchToken": "tok"` 仍在 fixture（该测试只断言已建模字段，launchToken 现在已建模但断言不涉及）。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-bridge pty`
Expected: 编译失败（字段/函数缺失）

- [ ] **Step 3: 实现**

`PtySpawnArgs` 增字段：

```rust
    /// pane 归因身份（2A 忽略、2B 接真，规格 §3.5）：`ORCA_PANE_KEY = ${tabId}:${leafId}`。
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub leaf_id: Option<String>,
    /// launchToken：hook 归因元数据与 pending 队列 TTL 已用，spawn 时注入 `ORCA_AGENT_LAUNCH_TOKEN`。
    #[serde(default)]
    pub launch_token: Option<String>,
```

新增纯函数（`resolve_spawn_cwd` 附近）：

```rust
/// spawn env 终态（规格 §3.5）：renderer env 打底 → hook endpoint env 覆盖 →
/// pane 身份/launchToken 覆盖（宿主是权威，防 renderer 陈旧值）。
pub fn build_spawn_env(
    base: HashMap<String, String>,
    hooks_env: &HashMap<String, String>,
    tab_id: Option<&str>,
    leaf_id: Option<&str>,
    worktree_id: Option<&str>,
    launch_token: Option<&str>,
) -> HashMap<String, String> {
    let mut env = base;
    for (key, value) in hooks_env {
        env.insert(key.clone(), value.clone());
    }
    if let (Some(tab_id), Some(leaf_id)) = (tab_id, leaf_id) {
        if !tab_id.is_empty() && !leaf_id.is_empty() {
            env.insert("ORCA_PANE_KEY".to_string(), format!("{tab_id}:{leaf_id}"));
        }
    }
    if let Some(tab_id) = tab_id.filter(|value| !value.is_empty()) {
        env.insert("ORCA_TAB_ID".to_string(), tab_id.to_string());
    }
    if let Some(worktree_id) = worktree_id.filter(|value| !value.is_empty()) {
        env.insert("ORCA_WORKTREE_ID".to_string(), worktree_id.to_string());
    }
    if let Some(launch_token) = launch_token.filter(|value| !value.is_empty()) {
        env.insert("ORCA_AGENT_LAUNCH_TOKEN".to_string(), launch_token.to_string());
    }
    env
}
```

`pty_spawn` 内 `let request = ade_pty::SpawnRequest { ... env: args.env, ... }` 改为：

```rust
    let env = build_spawn_env(
        args.env,
        &state.hooks.pty_env(),
        args.tab_id.as_deref(),
        args.leaf_id.as_deref(),
        args.worktree_id.as_deref(),
        args.launch_token.as_deref(),
    );
    let request = ade_pty::SpawnRequest {
        cols: args.cols,
        rows: args.rows,
        cwd: Some(resolved_cwd.path),
        env,
        env_to_delete: args.env_to_delete,
        command: args.command,
        shell_override: args.shell_override,
    };
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p ade-bridge pty`
Expected: PASS（新增 3 个 + 既有回归全绿）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/crates/ade-bridge/src/commands/pty.rs
git commit -m "feat(bridge): pty spawn env 注入 hook endpoint + pane 身份（tabId/leafId 接真）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: settings 开关运行时 reconcile + notifications 宿主命令 + 插件接线

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/settings.rs`
- Create: `src-tauri/crates/ade-bridge/src/commands/notifications.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Modify: `src-tauri/crates/ade-bridge/Cargo.toml`（`base64`）
- Modify: `src-tauri/Cargo.toml`（`tauri-plugin-notification = "2"`）
- Modify: `src-tauri/src/lib.rs`（`.plugin(tauri_plugin_notification::init())`）
- Modify: `src-tauri/capabilities/default.json`（`notification:default`）
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: `AppState.{hooks, home}`、`events::SETTINGS_CHANGED`
- Produces:
  - `settings_set` 内：changed 含 `agentStatusHooksEnabled` → `hooks.set_hooks_enabled(enabled, home)`（**裁定**：显式 toggle 关 = 移除托管条目；启动期关闭仍 skip 不删，见 Task 5 实现与规格 §3.3/§4）
  - 命令 `notifications_open_system_settings()`
  - 命令 `notifications_read_sound({ args: { path } }) -> NotificationSoundReadResult { ok, dataBase64?, mimeType?, path?, reason? }`

- [ ] **Step 1: 写失败测试**

`settings.rs` tests 追加：

```rust
    #[test]
    fn extracts_agent_status_hooks_toggle_from_changed_keys() {
        assert_eq!(
            hooks_toggle_from_changes(&json!({ "agentStatusHooksEnabled": false })),
            Some(false)
        );
        assert_eq!(
            hooks_toggle_from_changes(&json!({ "theme": "dark" })),
            None
        );
    }
```

`notifications.rs` tests：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_type_supports_the_fork_extensions_only() {
        assert_eq!(sound_mime_type("a.wav"), Some("audio/wav"));
        assert_eq!(sound_mime_type("a.MP3"), Some("audio/mpeg"));
        assert_eq!(sound_mime_type("a.ogg"), Some("audio/ogg"));
        assert_eq!(sound_mime_type("a.flac"), Some("audio/flac"));
        assert_eq!(sound_mime_type("a.txt"), None);
        assert_eq!(sound_mime_type("noext"), None);
    }

    #[test]
    fn read_sound_reports_missing_and_unsupported_without_touching_fs() {
        assert_eq!(
            load_sound(Path::new("/definitely/missing.wav")).reason.as_deref(),
            Some("missing-path")
        );
        assert_eq!(
            load_sound(Path::new("/tmp/song.txt")).reason.as_deref(),
            Some("unsupported-type")
        );
    }

    #[test]
    fn read_sound_returns_base64_for_a_small_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ding.wav");
        std::fs::write(&path, b"RIFF").unwrap();
        let result = load_sound(&path);
        assert!(result.ok);
        assert_eq!(result.mime_type.as_deref(), Some("audio/wav"));
        assert_eq!(result.data_base64.as_deref(), Some("UklGRg=="));
    }
}
```

（`tempfile` 已在 ade-bridge dev-deps。）

`specta_export.rs`：`collect_commands!` 加 `commands::notifications::notifications_open_system_settings,`、`commands::notifications::notifications_read_sound,`；`.typ` 加 `.typ::<commands::notifications::NotificationSoundReadResult>()`；清单加两条命令名。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-bridge settings notifications`
Expected: 编译失败

- [ ] **Step 3: 实现**

`settings.rs`：加纯函数与运行时接线：

```rust
/// 从 `settings:changed` 载荷提取 hooks 开关（缺省不动作）。
pub fn hooks_toggle_from_changes(changed: &Value) -> Option<bool> {
    changed.get("agentStatusHooksEnabled").and_then(Value::as_bool)
}
```

`settings_set` 在 `events::emit_json(...)` 之后加：

```rust
    // 显式 toggle（规格 §3.3/§4 裁定）：开 = 安装/更新；关 = 移除托管条目。
    // 启动期关闭只 skip 不删；用户显式关闭才移除（oracle `applyAgentStatusHooksEnabled`）。
    if let Some(enabled) = hooks_toggle_from_changes(&changed) {
        state.hooks.set_hooks_enabled(enabled, &state.home);
    }
```

`commands/mod.rs` 加 `pub mod notifications;`。新建 `commands/notifications.rs`：

```rust
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Serialize;

use crate::errors::BridgeError;

const MAX_SOUND_BYTES: u64 = 10 * 1024 * 1024;
const ALLOWED_EXTENSIONS: &[(&str, &str)] = &[
    ("ogg", "audio/ogg"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("m4a", "audio/mp4"),
    ("aac", "audio/aac"),
    ("flac", "audio/flac"),
];

pub fn sound_mime_type(path: &str) -> Option<&'static str> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    ALLOWED_EXTENSIONS
        .iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, mime)| *mime)
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSoundReadResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn failure(reason: &str) -> NotificationSoundReadResult {
    NotificationSoundReadResult {
        ok: false,
        data_base64: None,
        mime_type: None,
        path: None,
        reason: Some(reason.to_string()),
    }
}

pub fn load_sound(path: &Path) -> NotificationSoundReadResult {
    let Some(mime) = sound_mime_type(&path.to_string_lossy()) else {
        return failure("unsupported-type");
    };
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return failure("missing-path"),
        Err(_) => return failure("read-failed"),
    };
    if metadata.len() > MAX_SOUND_BYTES {
        return failure("too-large");
    }
    match std::fs::read(path) {
        Ok(bytes) => NotificationSoundReadResult {
            ok: true,
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            mime_type: Some(mime.to_string()),
            path: Some(path.to_string_lossy().into_owned()),
            reason: None,
        },
        Err(_) => failure("read-failed"),
    }
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsReadSoundArgs {
    pub path: String,
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_read_sound(
    args: NotificationsReadSoundArgs,
) -> Result<NotificationSoundReadResult, BridgeError> {
    let path = PathBuf::from(&args.path);
    crate::commands::run_blocking(move || Ok(load_sound(&path))).await
}

/// 打开 macOS 通知系统设置（blocked-by-system 回退入口）。
#[tauri::command]
#[specta::specta]
pub async fn notifications_open_system_settings() -> Result<(), BridgeError> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.notifications")
            .spawn()
            .map_err(|error| {
                BridgeError::message(format!("failed to open notification settings: {error}"))
            })?;
    }
    Ok(())
}
```

`src-tauri/crates/ade-bridge/Cargo.toml` 增 `base64 = "0.22"`。

`src-tauri/Cargo.toml` 增 `tauri-plugin-notification = "2"`；`src/lib.rs` builder 链上加（`tauri::Builder::default()` 之后）：

```rust
        .plugin(tauri_plugin_notification::init())
```

`src-tauri/capabilities/default.json`：

```json
{
  "identifier": "default",
  "description": "ade default capability",
  "windows": ["main"],
  "permissions": ["core:default", "notification:default"]
}
```

- [ ] **Step 4: 重新生成 bindings 并跑测试**

Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`
Expected: `notifications_open_system_settings`/`notifications_read_sound` 进 bindings；全绿

- [ ] **Step 5: Commit**

```bash
git add src-tauri src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): hooks 开关运行时 reconcile + notifications 宿主命令 + 通知插件接线

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

### Task 9: renderer node 内置模块 shim + vite alias

**Files:**
- Create: `src/renderer/src/lib/browser-node-shims.ts`
- Create: `src/renderer/src/lib/browser-node-shims.test.ts`
- Modify: `vite.config.ts`

**背景（写计划时的实地核查）**：`src/shared/agent-hook-listener.ts` 的归一化图（72 文件）包含 `transcript-reader.ts`（`node:fs` + 模块顶层 `Buffer.alloc(0)`）、`hook-envelope.ts`（`node:buffer`）、`agent-hook-relay.ts`/`command-code-transcript.ts`（`node:crypto`）、`grok-result-discovery.ts`（`node:os`）、`grok-session-paths.ts`（`node:fs/promises`/`node:path`）。rolldown-vite 生产构建把 node 内置外置成 `{}`、dev server 外置成会抛错的 Proxy——renderer 直接 import 会在 dev/加载期炸。用精确 alias 把这 7 个 specifier 指到同一 shim：claude 路径真正用到的只有 `Buffer.alloc(0)`（模块加载）与函数内的 transcript fallback（调用点已被 try/catch fail-open 包住，shim 抛错等价读失败）。

- [ ] **Step 1: 写失败测试**

`src/renderer/src/lib/browser-node-shims.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import {
  Buffer,
  createHash,
  homedir,
  isAbsolute,
  join,
  openSync,
  statSync
} from './browser-node-shims'

describe('browser node shims for the agent-hook normalization graph', () => {
  it('provides a Buffer sufficient for module-load constant allocation', () => {
    expect(Buffer.alloc(0)).toBeInstanceOf(Uint8Array)
    const joined = Buffer.concat([Buffer.from('ab'), Buffer.from('c')])
    expect(joined).toBeInstanceOf(Uint8Array)
    expect(Buffer.byteLength('héllo')).toBe(6)
  })

  it('throws only when a node-only transcript/crypto path is actually reached', () => {
    expect(() => statSync('/tmp/x')).toThrow(/not available in the renderer/)
    expect(() => openSync('/tmp/x', 'r')).toThrow(/not available in the renderer/)
    expect(() => createHash('sha256')).toThrow(/not available in the renderer/)
  })

  it('keeps the pure path helpers used by lazily-reached transcript discovery benign', () => {
    expect(homedir()).toBe('')
    expect(join('a', 'b', 'c')).toBe('a/b/c')
    expect(isAbsolute('/tmp')).toBe(true)
    expect(isAbsolute('tmp')).toBe(false)
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/renderer/src/lib/browser-node-shims.test.ts`
Expected: 模块不存在 → FAIL

- [ ] **Step 3: 实现 shim 与 alias**

`src/renderer/src/lib/browser-node-shims.ts`：

```ts
// Why: the shared agent-hook normalization graph imports node builtins for the
// main-process/relay listeners. The renderer reuses its pure claude mapping, so
// every node specifier in that graph aliases here (vite.config.ts). Buffer must
// be real enough for module-load constants; fs/crypto throw because every call
// site either never fires for claude hooks or fails open inside try/catch.

export class BrowserBuffer extends Uint8Array {
  static alloc(size: number): BrowserBuffer {
    return new BrowserBuffer(size)
  }

  static allocUnsafe(size: number): BrowserBuffer {
    return new BrowserBuffer(size)
  }

  static concat(chunks: Uint8Array[]): BrowserBuffer {
    const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0)
    const out = new BrowserBuffer(total)
    let offset = 0
    for (const chunk of chunks) {
      out.set(chunk, offset)
      offset += chunk.length
    }
    return out
  }

  static byteLength(value: string): number {
    return new TextEncoder().encode(value).length
  }

  static from(value: string | ArrayLike<number>): BrowserBuffer {
    if (typeof value === 'string') {
      return new BrowserBuffer(new TextEncoder().encode(value).buffer)
    }
    return new BrowserBuffer(value)
  }

  toString(): string {
    return new TextDecoder().decode(this)
  }
}

export const Buffer = BrowserBuffer

function unavailable(name: string): (...args: unknown[]) => never {
  return () => {
    throw new Error(`${name} is not available in the renderer (agent-hook node shim)`)
  }
}

export const closeSync = unavailable('fs.closeSync')
export const openSync = unavailable('fs.openSync')
export const readSync = unavailable('fs.readSync')
export const statSync = unavailable('fs.statSync')
export const lstatSync = unavailable('fs.lstatSync')
export const readdirSync = unavailable('fs.readdirSync')
export const readFileSync = unavailable('fs.readFileSync')
export const writeFileSync = unavailable('fs.writeFileSync')
export const existsSync = unavailable('fs.existsSync')
export const createHash = unavailable('crypto.createHash')
export const createHmac = unavailable('crypto.createHmac')
export const randomBytes = unavailable('crypto.randomBytes')
export const randomUUID = (): string => crypto.randomUUID()
export const homedir = (): string => ''
export const tmpdir = (): string => '/tmp'

export function join(...parts: string[]): string {
  return parts.filter(Boolean).join('/').replace(/\/{2,}/g, '/')
}

export function basename(value: string): string {
  return value.split('/').pop() ?? value
}

export function dirname(value: string): string {
  const parts = value.split('/')
  parts.pop()
  return parts.join('/') || '.'
}

export function extname(value: string): string {
  const base = basename(value)
  const index = base.lastIndexOf('.')
  return index > 0 ? base.slice(index) : ''
}

export function isAbsolute(value: string): boolean {
  return value.startsWith('/')
}

export function readFile(): Promise<never> {
  return Promise.reject(new Error('fs/promises.readFile is not available in the renderer'))
}
```

`vite.config.ts` `resolve.alias` 从对象改为数组（顺序敏感，前 7 条为精确 regex）；保留 `@renderer`/`@`：

```ts
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const agentHookNodeShim = resolve('src/renderer/src/lib/browser-node-shims.ts')

export default defineConfig({
  root: resolve('src/renderer'),
  base: './',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: [
      { find: /^node:buffer$/, replacement: agentHookNodeShim },
      { find: /^node:fs$/, replacement: agentHookNodeShim },
      { find: /^node:fs\/promises$/, replacement: agentHookNodeShim },
      { find: /^node:crypto$/, replacement: agentHookNodeShim },
      { find: /^node:os$/, replacement: agentHookNodeShim },
      { find: /^node:path$/, replacement: agentHookNodeShim },
      { find: /^node:http$/, replacement: agentHookNodeShim },
      { find: '@renderer', replacement: resolve('src/renderer/src') },
      { find: '@', replacement: resolve('src/renderer/src') }
    ]
  },
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    outDir: resolve('dist'),
    emptyOutDir: true
  },
  worker: { format: 'es' }
})
```

- [ ] **Step 4: 运行确认通过**

Run: `pnpm vitest run src/renderer/src/lib/browser-node-shims.test.ts && pnpm build:web`
Expected: 3 tests PASS；build 成功（若出现 node 外置告警，说明 alias 未命中，必须修 regex 直到无 `externalized` 告警）

- [ ] **Step 5: Commit**

```bash
git add vite.config.ts src/renderer/src/lib/browser-node-shims.ts src/renderer/src/lib/browser-node-shims.test.ts
git commit -m "feat(renderer): agent-hook 归一化图的 node 内置浏览器 shim + vite alias

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: renderer agent-status real 域

**Files:**
- Create: `src/bridge/real/agent-status.ts`
- Create: `src/bridge/real/agent-status.test.ts`
- Modify: `src/bridge/create-api.ts`
- Modify: `src/bridge/real/parity.test.ts`

**Interfaces:**
- Consumes: `shared/agent-hook-listener.ts:normalizeHookPayload`、`shared/agent-hook-listener/listener-state.ts:createHookListenerState`、`AgentStatusIpcPayload`、`real/invoke.ts:{invokeCommand, subscribeToEvent}`、命令 `agent_status_get_snapshot`、事件 `agent-hook:raw`
- Produces: `createAgentStatusRealApi(): PreloadApi['agentStatus']`
  - `onSet`：订阅 `agent-hook:raw` → 归一化 → `AgentStatusIpcPayload`（失败丢弃）
  - `onClear`：立即返回退订的 noop 订阅（宿主不产 clear，规格 §3.7）
  - `getSnapshot`：命令 → 逐条归一化（`restored && state !== 'done'` → `restoredUnconfirmed`）
  - 其余契约方法留 fallback（规格 §2.2/§3.7）

- [ ] **Step 1: 写失败测试**

`src/bridge/real/agent-status.test.ts`：

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
let eventHandler: ((message: { payload: unknown }) => void) | null = null

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args)
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_event: string, handler: (message: { payload: unknown }) => void) => {
    eventHandler = handler
    return () => {
      eventHandler = null
    }
  })
}))

import { createAgentStatusRealApi } from './agent-status'

const PANE = 't1:123e4567-e89b-42d3-a456-426614174000'

function emitRaw(payload: Record<string, unknown>, receivedAt = 1000) {
  eventHandler?.({
    payload: {
      source: 'claude',
      payload,
      paneKey: PANE,
      tabId: 't1',
      worktreeId: 'r1::/wt',
      launchToken: 'tok-1',
      receivedAt,
      restored: false
    }
  })
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

describe('agentStatus real bridge', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    eventHandler = null
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('normalizes claude working/waiting/done and tracks stateStartedAt epochs', async () => {
    const api = createAgentStatusRealApi()
    const seen: { state: string; stateStartedAt: number }[] = []
    const unsubscribe = api.onSet((payload) => {
      seen.push({ state: payload.state, stateStartedAt: payload.stateStartedAt })
    })
    await flush()
    emitRaw({ hook_event_name: 'UserPromptSubmit', prompt: 'Fix login bug', session_id: 's1' }, 1000)
    emitRaw({ hook_event_name: 'PostToolUse', tool_name: 'Bash', session_id: 's1' }, 1100)
    emitRaw({ hook_event_name: 'PermissionRequest', tool_name: 'Bash', session_id: 's1' }, 1200)
    emitRaw({ hook_event_name: 'Stop', last_assistant_message: 'Done.', session_id: 's1' }, 1300)
    expect(seen.map((entry) => entry.state)).toEqual(['working', 'working', 'waiting', 'done'])
    // 同 state 的后续事件不重置 epoch；state 变化才换 epoch。
    expect(seen.map((entry) => entry.stateStartedAt)).toEqual([1000, 1000, 1200, 1300])
    unsubscribe()
  })

  it('assembles the IPC envelope and drops unnormalizable payloads', async () => {
    const api = createAgentStatusRealApi()
    const seen: unknown[] = []
    api.onSet((payload) => seen.push(payload))
    await flush()
    emitRaw({ hook_event_name: 'Stop', last_assistant_message: 'Done.', session_id: 's1' })
    emitRaw({ not_a_hook: true })
    expect(seen).toHaveLength(1)
    expect(seen[0]).toMatchObject({
      paneKey: PANE,
      tabId: 't1',
      worktreeId: 'r1::/wt',
      launchToken: 'tok-1',
      connectionId: null,
      receivedAt: 1000,
      stateStartedAt: 1000,
      state: 'done'
    })
  })

  it('onClear is an immediate noop subscription', () => {
    const api = createAgentStatusRealApi()
    const unsubscribe = api.onClear(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })

  it('getSnapshot replays cached entries and marks restored non-done rows', async () => {
    const api = createAgentStatusRealApi()
    invokeMock.mockResolvedValueOnce([
      {
        source: 'claude',
        payload: { hook_event_name: 'PermissionRequest', tool_name: 'Bash' },
        paneKey: PANE,
        tabId: 't1',
        worktreeId: 'r1::/wt',
        receivedAt: 500,
        restored: true
      },
      {
        source: 'claude',
        payload: { hook_event_name: 'Stop', last_assistant_message: 'Done.' },
        paneKey: 't2:123e4567-e89b-42d3-a456-426614174001',
        receivedAt: 600,
        restored: true
      }
    ])
    const snapshot = await api.getSnapshot()
    expect(invokeMock).toHaveBeenCalledWith('agent_status_get_snapshot')
    expect(snapshot[0]).toMatchObject({ state: 'waiting', restoredUnconfirmed: true })
    expect(snapshot[1]).toMatchObject({ state: 'done' })
    expect('restoredUnconfirmed' in snapshot[1]).toBe(false)
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/bridge/real/agent-status.test.ts`
Expected: 模块不存在 → FAIL

- [ ] **Step 3: 实现**

`src/bridge/real/agent-status.ts`：

```ts
import type { PreloadApi } from '../../shared/preload-api/api-types'
import type { AgentStatusIpcPayload } from '../../shared/agent-status-ipc-payload'
import { normalizeHookPayload } from '../../shared/agent-hook-listener'
import { createHookListenerState } from '../../shared/agent-hook-listener/listener-state'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/** `agent-hook:raw` 事件载荷（Rust `events::AgentHookRawPayload` 同形）。 */
type AgentHookRawEvent = {
  source: string
  payload: Record<string, unknown>
  paneKey: string
  tabId?: string
  worktreeId?: string
  launchToken?: string
  receivedAt: number
  restored?: boolean
  env?: string
}

/** `agent_status_get_snapshot` 元素（Rust `AgentHookSnapshotEntry` 同形）。 */
type AgentHookSnapshotEntry = AgentHookRawEvent

export function createAgentStatusRealApi(): PreloadApi['agentStatus'] {
  // Why: 归一化在 renderer（规格 §3.2）；每个域实例持有 fork 监听器的
  // per-pane 缓存（prompt/tool/lead 状态）与 stateStartedAt epoch，跨事件演化。
  const listenerState = createHookListenerState()
  const stateStartedAtByPaneKey = new Map<string, { state: string; startedAt: number }>()

  const buildIpcPayload = (raw: AgentHookRawEvent): AgentStatusIpcPayload | null => {
    const normalized = normalizeHookPayload(
      listenerState,
      'claude',
      {
        paneKey: raw.paneKey,
        tabId: raw.tabId,
        worktreeId: raw.worktreeId,
        launchToken: raw.launchToken,
        payload: raw.payload
      },
      raw.env ?? ''
    )
    if (!normalized) {
      return null
    }
    const state = normalized.payload.state
    const prior = stateStartedAtByPaneKey.get(normalized.paneKey)
    const stateStartedAt =
      prior && prior.state === state ? prior.startedAt : raw.receivedAt
    stateStartedAtByPaneKey.set(normalized.paneKey, { state, startedAt: stateStartedAt })
    const restoredUnconfirmed = raw.restored === true && state !== 'done'
    return {
      ...normalized.payload,
      paneKey: normalized.paneKey,
      ...(normalized.launchToken ? { launchToken: normalized.launchToken } : {}),
      ...(normalized.tabId ? { tabId: normalized.tabId } : {}),
      ...(normalized.worktreeId ? { worktreeId: normalized.worktreeId } : {}),
      connectionId: null,
      receivedAt: raw.receivedAt,
      stateStartedAt,
      ...(restoredUnconfirmed ? { restoredUnconfirmed: true } : {}),
      ...(normalized.promptInteractionKey
        ? { promptInteractionKey: normalized.promptInteractionKey }
        : {}),
      ...(normalized.providerSession ? { providerSession: normalized.providerSession } : {}),
      ...(normalized.providerSessionOnly ? { providerSessionOnly: true } : {})
    }
  }

  return withMethodFallback<PreloadApi['agentStatus']>('agentStatus', {
    onSet: (callback) =>
      subscribeToEvent<AgentHookRawEvent>('agent-hook:raw', (raw) => {
        const payload = buildIpcPayload(raw)
        if (payload) {
          callback(payload)
        }
      }),
    // 宿主不产 clear 事件；通道保留为立即退订的 noop（规格 §3.7）。
    onClear: () => () => {},
    getSnapshot: async () => {
      const entries = await invokeCommand<AgentHookSnapshotEntry[]>('agent_status_get_snapshot')
      const payloads: AgentStatusIpcPayload[] = []
      for (const entry of entries) {
        const payload = buildIpcPayload(entry)
        if (payload) {
          payloads.push(payload)
        }
      }
      return payloads
    }
  })
}
```

Rust 事件当前不携带 `env`；TS 侧 `raw.env` 为 undefined 时 `expectedEnv=''`，`warnOnHookEnvOrVersionMismatch` 对空值直接跳过告警（`listener-limits.ts:53` 判空）。

`create-api.ts`：

- `RealDomains` 的 `Pick` 联合加 `| 'agentStatus'`（放 `app` 前，保持字母序）。
- import 加 `import { createAgentStatusRealApi } from './real/agent-status'`。
- `createRealDomains()` 对象加 `agentStatus: createAgentStatusRealApi(),`。

`parity.test.ts`：

- import 加 `import { createAgentStatusRealApi } from './agent-status'`。
- `surfaceCases` 增：

```ts
  {
    domain: 'agentStatus',
    explicit: ['onSet', 'onClear', 'getSnapshot'],
    missing: [
      'inferInterrupt',
      'inferQuestionAnswered',
      'getMigrationUnsupportedSnapshot',
      'onMigrationUnsupported',
      'onMigrationUnsupportedClear',
      'onLegacyWorkerTerminalRecovery',
      'drop',
      'dropPersisted',
      'dropPersistedBatch',
      'reconcileEndedProcess',
      'dropByTabPrefix',
      'retirePaneAuthority',
      'restorePaneAuthority',
      'transferPaneAuthority'
    ]
  },
```

- `realApiFor` 增 `case 'agentStatus': return createAgentStatusRealApi()`。

- [ ] **Step 4: 运行确认通过**

Run: `pnpm vitest run src/bridge/real/agent-status.test.ts src/bridge/real/parity.test.ts src/bridge/create-api.test.ts src/bridge/mock/boot-namespaces.test.ts`
Expected: 全绿（parity 的 agentStatus case 断言 real 显式方法与 fallback 拒答）

- [ ] **Step 5: typecheck**

Run: `pnpm typecheck`
Expected: exit 0

- [ ] **Step 6: Commit**

```bash
git add src/bridge/real/agent-status.ts src/bridge/real/agent-status.test.ts src/bridge/create-api.ts src/bridge/real/parity.test.ts
git commit -m "feat(renderer): agentStatus real 域接真（raw→归一化→IPC payload + snapshot 回放）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 11: renderer notifications real 域 + mock + parity

**Files:**
- Modify: `package.json` + `pnpm-lock.yaml`（`@tauri-apps/plugin-notification`）
- Create: `src/bridge/real/notifications.ts`
- Create: `src/bridge/real/notifications.test.ts`
- Create: `src/bridge/mock/notifications-api.ts`
- Modify: `src/bridge/create-api.ts`
- Modify: `src/bridge/real/parity.test.ts`
- Modify: `src/bridge/mock/boot-namespaces.test.ts`（补一个 mock 冒烟）

**实现裁定（规格 §3.8 “细节计划阶段核实”）**：

1. **dispatch** 走 `@tauri-apps/plugin-notification`（`isPermissionGranted`/`requestPermission`/`sendNotification`）。desktop 的 `isPermissionGranted` 在本插件语义下是 Web Notification 权限；未授权且拒绝/未决 → `{delivered:false, reason:'blocked-by-system'}`（对齐 fork macOS 语义）。设置门与 focus/cooldown 也在 renderer 域内实现（fork 的 main 侧门禁平移到 TS 宿主适配层，规格 §2.1）。
2. **dismiss**：插件 `removeActive` 只收 32-bit 数字 id，`buildAgentNotificationId` 是字符串稳定 id（规格 §3.8）。用 FNV-1a 32 位哈希把字符串 id 映射为插件 id，`removeActive` 失败/不支持时返回 `{dismissed:0}`（本域无生产消费方，不阻塞验收）。
3. **playSound**：仅支持 `customSoundId === 'custom'` + `customSoundPath`（内置 9 音效资产不在 2B 范围）；经宿主 `notifications_read_sound` 取字节 → Blob → `Audio`，播放中同路径 → `deduped`。其余情况 `missing-path`。

- [ ] **Step 1: 加 npm 依赖**

Run: `pnpm add @tauri-apps/plugin-notification@^2`
Expected: package.json dependencies 出现该包；lockfile 更新

- [ ] **Step 2: 写失败测试**

`src/bridge/real/notifications.test.ts`（`document` 需要 DOM 环境）：

```ts
// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
const sendNotificationMock = vi.fn()
let permissionGranted = true
let requestResult: 'granted' | 'denied' | 'default' = 'granted'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args)
}))
vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: vi.fn(async () => permissionGranted),
  requestPermission: vi.fn(async () => requestResult),
  sendNotification: (...args: unknown[]) => sendNotificationMock(...args),
  removeActive: vi.fn(async () => undefined)
}))

import {
  buildNotificationCopy,
  createNotificationsRealApi,
  reserveNotificationCooldown
} from './notifications'

function installSettings(notifications: Record<string, unknown>): void {
  ;(globalThis as Record<string, unknown>).__ADE_BOOTSTRAP__ = {
    settings: {
      notifications: {
        enabled: true,
        agentTaskComplete: true,
        terminalBell: true,
        suppressWhenFocused: false,
        customSoundId: 'system',
        customSoundPath: null,
        customSoundVolume: 0.5,
        ...notifications
      }
    },
    platform: { platform: 'darwin' },
    schemaVersion: 1
  }
}

describe('notifications real bridge', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    sendNotificationMock.mockReset()
    permissionGranted = true
    requestResult = 'granted'
    installSettings({})
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterEach(() => {
    delete (globalThis as Record<string, unknown>).__ADE_BOOTSTRAP__
    vi.restoreAllMocks()
  })

  it('dispatches an OS notification when settings and permission allow', async () => {
    const api = createNotificationsRealApi()
    const result = await api.dispatch({
      source: 'agent-task-complete',
      worktreeId: 'r1::/wt',
      paneKey: 't1:leaf',
      worktreeLabel: 'feature/login',
      terminalTitle: 'claude',
      agentState: 'waiting',
      agentPrompt: 'Fix login bug'
    })
    expect(result).toEqual({ delivered: true })
    expect(sendNotificationMock).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'claude', body: expect.stringContaining('Fix login bug') })
    )
  })

  it('gates on disabled/source-disabled/suppressed-focus/cooldown', async () => {
    installSettings({ enabled: false })
    expect(await createNotificationsRealApi().dispatch({ source: 'agent-task-complete' })).toEqual({
      delivered: false,
      reason: 'disabled'
    })
    installSettings({ agentTaskComplete: false })
    expect(await createNotificationsRealApi().dispatch({ source: 'agent-task-complete' })).toEqual({
      delivered: false,
      reason: 'source-disabled'
    })
    installSettings({ suppressWhenFocused: true })
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    expect(
      await createNotificationsRealApi().dispatch({
        source: 'agent-task-complete',
        isActiveWorktree: true,
        worktreeId: 'r-cooldown'
      })
    ).toEqual({ delivered: false, reason: 'suppressed-focus' })
    installSettings({})
    const api = createNotificationsRealApi()
    await api.dispatch({ source: 'agent-task-complete', worktreeId: 'r-cooldown' })
    expect(await api.dispatch({ source: 'agent-task-complete', worktreeId: 'r-cooldown' })).toEqual({
      delivered: false,
      reason: 'cooldown'
    })
  })

  it('reports blocked-by-system when permission stays denied', async () => {
    permissionGranted = false
    requestResult = 'denied'
    const api = createNotificationsRealApi()
    expect(await api.dispatch({ source: 'agent-task-complete' })).toEqual({
      delivered: false,
      reason: 'blocked-by-system'
    })
    expect(await api.probeDelivery()).toEqual({ state: 'blocked', authoritative: false })
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: false
    })
  })

  it('builds deterministic copy and reserves cooldown per worktree', () => {
    expect(
      buildNotificationCopy({
        source: 'agent-task-complete',
        terminalTitle: 'claude',
        worktreeLabel: 'feature/login',
        agentState: 'waiting',
        agentPrompt: 'Fix login bug'
      })
    ).toEqual({ title: 'claude', body: 'Fix login bug' })
    const map = new Map<string, number>()
    expect(reserveNotificationCooldown(map, 'w1', 1000)).toBe(true)
    expect(reserveNotificationCooldown(map, 'w1', 2000)).toBe(false)
    expect(reserveNotificationCooldown(map, 'w1', 7000)).toBe(true)
  })

  it('plays custom sounds through the host reader and dedupes while playing', async () => {
    installSettings({ customSoundId: 'custom', customSoundPath: '/tmp/ding.wav' })
    invokeMock.mockResolvedValueOnce({
      ok: true,
      dataBase64: 'UklGRg==',
      mimeType: 'audio/wav',
      path: '/tmp/ding.wav'
    })
    const played: string[] = []
    class FakeAudio {
      volume = 1
      onended: (() => void) | null = null
      onerror: (() => void) | null = null
      play(): Promise<void> {
        played.push('play')
        this.onended?.()
        return Promise.resolve()
      }
    }
    vi.stubGlobal('Audio', FakeAudio)
    vi.stubGlobal('URL', {
      createObjectURL: () => 'blob:fake',
      revokeObjectURL: () => {}
    })
    const api = createNotificationsRealApi()
    expect(await api.playSound({ volume: 0.3 })).toEqual({ played: true })
    expect(played).toEqual(['play'])
    vi.unstubAllGlobals()
  })
})
```

`boot-namespaces.test.ts` 追加：

```ts
  it('notifications: mock dispatch reports not-supported and subscription-free methods resolve', async () => {
    const notifications = createNotificationsApi()
    await expect(
      notifications.dispatch({ source: 'agent-task-complete' })
    ).resolves.toEqual({ delivered: false, reason: 'not-supported' })
    await expect(notifications.dismiss(['a'])).resolves.toEqual({ dismissed: 0 })
    await expect(notifications.getDesktopAwayState()).resolves.toBeUndefined()
  })
```

（顶部加 `import { createNotificationsApi } from './notifications-api'`。）

- [ ] **Step 3: 运行确认失败**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts`
Expected: 模块不存在 → FAIL

- [ ] **Step 4: 实现**

`src/bridge/mock/notifications-api.ts`：

```ts
import type { PreloadApi } from '../../preload/api-types'

/** Phase 0 mock：mock 模式与 web-stub 同形（规格 §5.2）。 */
export function createNotificationsApi(): PreloadApi['notifications'] {
  return {
    getDesktopAwayState: async () => undefined,
    dispatch: async () => ({ delivered: false, reason: 'not-supported' }),
    dismiss: async () => ({ dismissed: 0 }),
    openSystemSettings: async () => {},
    getPermissionStatus: async () => ({ supported: false, platform: 'darwin', requested: false }),
    probeDelivery: async () => ({ state: 'unsupported', authoritative: false }),
    playSound: async () => ({ played: false, reason: 'missing-path' })
  }
}
```

`src/bridge/real/notifications.ts`：

```ts
import {
  isPermissionGranted,
  removeActive,
  requestPermission,
  sendNotification
} from '@tauri-apps/plugin-notification'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import type {
  NotificationDispatchRequest,
  NotificationDispatchResult,
  NotificationPermissionStatusResult,
  NotificationSoundResult
} from '../../shared/notification-settings-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { getBootstrap } from './bootstrap'
import { invokeCommand } from './invoke'

const NOTIFICATION_COOLDOWN_MS = 5_000
const MAX_RECENT_NOTIFICATION_KEYS = 50
const recentDesktopNotifications = new Map<string, number>()
const playingSoundPaths = new Set<string>()

/** 规格 §3.8 裁决 1：标题/正文取最不惊讶的稳定字段（fork 文案的 2B 子集）。 */
export function buildNotificationCopy(args: NotificationDispatchRequest): {
  title: string
  body: string
} {
  const title = args.terminalTitle?.trim() || args.worktreeLabel?.trim() || 'Orcinus'
  const body =
    args.agentLastAssistantMessage?.trim() ||
    args.agentPrompt?.trim() ||
    (args.agentState === 'waiting'
      ? 'Waiting for input'
      : args.agentState === 'done'
        ? 'Task complete'
        : args.worktreeLabel?.trim() || 'Agent activity')
  return { title, body }
}

export function reserveNotificationCooldown(
  map: Map<string, number>,
  key: string,
  now: number
): boolean {
  const last = map.get(key) ?? 0
  if (now - last < NOTIFICATION_COOLDOWN_MS) {
    return false
  }
  map.delete(key)
  map.set(key, now)
  while (map.size > MAX_RECENT_NOTIFICATION_KEYS) {
    const oldest = map.keys().next()
    if (oldest.done) {
      break
    }
    map.delete(oldest.value)
  }
  return true
}

/** 插件 id 是 32-bit int，稳定字符串 id 经 FNV-1a 映射（裁决 2）。 */
export function hashNotificationId(id: string): number {
  let hash = 0x811c9dc5
  for (let index = 0; index < id.length; index += 1) {
    hash ^= id.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193)
  }
  return hash | 0
}

function platformForPermissionStatus(): NodeJS.Platform {
  const bootstrap = getBootstrap()
  const platform = bootstrap?.platform?.platform
  return (platform as NodeJS.Platform | undefined) ?? 'darwin'
}

export function createNotificationsRealApi(): PreloadApi['notifications'] {
  return withMethodFallback<PreloadApi['notifications']>('notifications', {
    getDesktopAwayState: async () => undefined,
    dispatch: async (args) => {
      const settings = getBootstrap()?.settings?.notifications
      if (settings && !settings.enabled) {
        return { delivered: false, reason: 'disabled' }
      }
      if (
        settings &&
        args.source === 'agent-task-complete' &&
        !settings.agentTaskComplete
      ) {
        return { delivered: false, reason: 'source-disabled' }
      }
      if (settings && args.source === 'terminal-bell' && !settings.terminalBell) {
        return { delivered: false, reason: 'source-disabled' }
      }
      if (
        args.source !== 'test' &&
        settings?.suppressWhenFocused &&
        args.isActiveWorktree &&
        typeof document !== 'undefined' &&
        document.hasFocus()
      ) {
        return { delivered: false, reason: 'suppressed-focus' }
      }
      if (args.source !== 'test') {
        const dedupeKey = args.worktreeId ?? args.worktreeLabel ?? 'global'
        if (!reserveNotificationCooldown(recentDesktopNotifications, dedupeKey, Date.now())) {
          return { delivered: false, reason: 'cooldown' }
        }
      }
      let granted = await isPermissionGranted()
      if (!granted) {
        granted = (await requestPermission()) === 'granted'
      }
      if (!granted) {
        return { delivered: false, reason: 'blocked-by-system' }
      }
      const copy = buildNotificationCopy(args)
      sendNotification({ title: copy.title, body: copy.body })
      return { delivered: true }
    },
    dismiss: async (ids) => {
      const unique = Array.from(new Set(ids.filter((id) => typeof id === 'string' && id.length > 0)))
      if (unique.length === 0) {
        return { dismissed: 0 }
      }
      try {
        await removeActive(unique.map((id) => ({ id: hashNotificationId(id) })))
        return { dismissed: unique.length }
      } catch {
        return { dismissed: 0 }
      }
    },
    openSystemSettings: () => invokeCommand('notifications_open_system_settings'),
    getPermissionStatus: async (): Promise<NotificationPermissionStatusResult> => ({
      supported: true,
      platform: platformForPermissionStatus(),
      requested: await isPermissionGranted()
    }),
    probeDelivery: async () => {
      if (await isPermissionGranted()) {
        return { state: 'delivered', authoritative: false }
      }
      const permission = await requestPermission()
      if (permission === 'granted') {
        return { state: 'delivered', authoritative: false }
      }
      if (permission === 'denied') {
        return { state: 'blocked', authoritative: false }
      }
      return { state: 'awaiting-decision', authoritative: false }
    },
    playSound: async (options): Promise<NotificationSoundResult> => {
      const settings = getBootstrap()?.settings?.notifications
      const path = settings?.customSoundPath
      if (!path || settings?.customSoundId !== 'custom') {
        return { played: false, reason: 'missing-path' }
      }
      if (playingSoundPaths.has(path)) {
        return { played: false, reason: 'deduped' }
      }
      const loaded = await invokeCommand<{
        ok: boolean
        dataBase64?: string
        mimeType?: string
        reason?: string
      }>('notifications_read_sound', { args: { path } })
      if (!loaded.ok || !loaded.dataBase64 || !loaded.mimeType) {
        const reason = loaded.reason
        return {
          played: false,
          reason:
            reason === 'missing-path' ||
            reason === 'invalid-path' ||
            reason === 'unsupported-type' ||
            reason === 'too-large' ||
            reason === 'read-failed'
              ? reason
              : 'read-failed'
        }
      }
      const bytes = Uint8Array.from(atob(loaded.dataBase64), (char) => char.charCodeAt(0))
      const objectUrl = URL.createObjectURL(new Blob([bytes], { type: loaded.mimeType }))
      playingSoundPaths.add(path)
      try {
        await new Promise<void>((resolve, reject) => {
          const audio = new Audio(objectUrl)
          if (typeof options?.volume === 'number') {
            audio.volume = Math.min(1, Math.max(0, options.volume))
          }
          audio.onended = () => resolve()
          audio.onerror = () => reject(new Error('playback failed'))
          void audio.play().catch(reject)
        })
        return { played: true }
      } catch {
        return { played: false, reason: 'playback-failed' }
      } finally {
        playingSoundPaths.delete(path)
        URL.revokeObjectURL(objectUrl)
      }
    }
  })
}
```

`create-api.ts`：

- import `createNotificationsApi`（mock）与 `createNotificationsRealApi`（real）。
- `RealDomains` 联合加 `| 'notifications'`。
- `createMockDomains()` 加 `notifications: createNotificationsApi(),`。
- `createRealDomains()` 加 `notifications: createNotificationsRealApi(),`。

`parity.test.ts`：

- import `import { createNotificationsRealApi } from './notifications'`。
- `surfaceCases` 增：

```ts
  {
    domain: 'notifications',
    explicit: [
      'getDesktopAwayState',
      'dispatch',
      'dismiss',
      'openSystemSettings',
      'getPermissionStatus',
      'probeDelivery',
      'playSound'
    ],
    missing: []
  },
```

- `realApiFor` 增 `case 'notifications': return createNotificationsRealApi()`。

- [ ] **Step 5: 运行确认通过**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts src/bridge/real/parity.test.ts src/bridge/create-api.test.ts src/bridge/mock/boot-namespaces.test.ts`
Expected: 全绿

- [ ] **Step 6: typecheck**

Run: `pnpm typecheck`
Expected: exit 0（若 `document`/`Audio`/`URL` 在 node 类型下缺失，测试文件用 `// @vitest-environment happy-dom` pragma 或 `vi.stubGlobal`；按报错调整，不得改契约）

- [ ] **Step 7: Commit**

```bash
git add package.json pnpm-lock.yaml src/bridge/real/notifications.ts src/bridge/real/notifications.test.ts src/bridge/mock/notifications-api.ts src/bridge/create-api.ts src/bridge/real/parity.test.ts src/bridge/mock/boot-namespaces.test.ts
git commit -m "feat(renderer): notifications real 域接真（plugin 通知/权限探测/自定义音效/dismiss 映射）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 12: 收尾记录 + 全量门禁 + 手工验收

**Files:**
- Create: `docs/phase2b-agent-hooks-notifications-record.md`
- Modify: 计划文件本任务勾选状态

**Interfaces:**
- Consumes: 前 11 个 Task 的全部产物
- Produces: 验收记录（本仓 Phase 记录的最后一环）

- [ ] **Step 1: 全量自动门禁**

Run（并行或顺序）：

```bash
cd src-tauri && cargo test --workspace
pnpm test
pnpm typecheck && pnpm build:web
```

Expected: 三组全绿 / exit 0

- [ ] **Step 2: 手工验收（规格 §1/§5.3）**

前置：`pnpm dev` 启动；Settings → Agents 确认 `agentStatusHooksEnabled` 开；`~/.claude/settings.json` 出现 12 个托管事件；`~/.ade/agent-hooks/claude-hook.sh` 存在且 0755。

1. 在终端 pane 启动 `claude`，发一条 prompt → tab/侧栏状态变 working（hook `UserPromptSubmit`）
2. 触发一次权限请求 → waiting + 桌面通知 + 未读徽标/高亮
3. 完成回复 → done；点进 pane → 自动已读、徽标清除
4. Settings 关 `agentStatusHooksEnabled` → `~/.claude/settings.json` 托管条目被移除 → 新对话不再驱动状态；再打开 → 条目回来
5. 重启 app（不重启 claude 进程）→ 状态经 `agent_status_get_snapshot` 回放（restored 行显示为未确认，不重新通知）
6. 断网/服务降级兜底：确认 2A transcript 扫描在 server 停用时仍能捕获休眠记录

任一手工项失败 → 记录到 record 文档的偏差节并修复后重跑对应自动门禁。

- [ ] **Step 3: 写记录文档**

`docs/phase2b-agent-hooks-notifications-record.md` 按既有记录体例（参考 `docs/phase2a-persistence-session-restore-record.md`）写：交付清单（crate/命令/事件/TS 域/插件）、验收证据（三组门禁输出摘要 + 手工链结果）、规格偏差备案（至少包括：dismiss 字符串 id→32 位哈希映射、playSound 仅 custom、启动关闭 skip/显式 toggle 删除的裁定、probeDelivery `authoritative:false`）、后续（2C/2D/其余 17 源）。

- [ ] **Step 4: 计划勾选同步**

把本计划全部 `- [ ]` 改为 `- [x]`（未执行项保留 `- [ ]` 并在记录文档注明）。

- [ ] **Step 5: Commit**

```bash
git add docs/phase2b-agent-hooks-notifications-record.md docs/superpowers/plans/2026-10-06-phase2b-agent-hooks-notifications.md
git commit -m "docs: Phase 2 子项目 B agent hook server + 状态接真 + 完成通知 实施记录

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## 自查结论（写计划时完成）

- **规格覆盖**：§2.1 六行实现面 → Task 1（endpoint）/2（cache）/3（script）/4（installer）/5（server）/6（bridge）/7（env）/8（开关+notifications 宿主）/9-11（TS 域）；§5.1/§5.2 测试面逐条落在各 Task Step；§2.2 排除项未开工。
- **三处规格内部张力已裁定并显式写死**：①§4 “开关关闭 skip 不删”与 §1 验收 “关开关 → 状态停驱”——裁决：启动关闭 skip、显式 toggle 关闭移除（oracle `applyAgentStatusHooksEnabled` 语义，Task 5/8）；②oracle “超限 fail-open 204” 与规格 §4 表 “413”——裁决：按规格回 413（Task 5 测试钉死）；③oracle “非 POST 404” 与规格 §4 表 “非 POST 403 计数”——裁决：按规格 403，未知路由仍 404（Task 5 测试钉死）。
- **类型一致性**：`CachedHookEvent`（Rust）→ `AgentHookRawPayload`/`AgentHookSnapshotEntry`（bridge）→ `AgentHookRawEvent`（TS）字段一一对应；命令名 `agent_status_get_snapshot`、`notifications_open_system_settings`、`notifications_read_sound` 在三处登记点（collect_commands/test 清单/bindings）一致。
- **已知风险**：Task 9 alias 未命中时 build:web 会留下 node 外置告警——该 Step 的 Expected 明确要求零告警；Task 11 在 node 环境下 `document`/`Audio` 的 stub 需按 typecheck 报错微调（只动测试 pragma/stub，不动契约）。
