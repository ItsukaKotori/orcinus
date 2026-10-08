# Phase 2D.1 GitHub 连接与只读基础 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 接真 GitHub 只读面：gh 执行器 + 连接/身份 + PR-for-branch + checks + 速率快照 + hosted review + preflight，点亮现有渲染层 UI。

**Architecture:** Rust 只加薄执行器（`gh_exec`/`gh_env_probe`/`git_remote_urls`：PATH 解析、超时进程组杀、并发闸、`GH_PROMPT_DISABLED`）；TS 在 `src/renderer/src/lib/github/` 用可注入 executor 实现解析/缓存/查询阶梯/错误分类，`src/bridge/real/gh.ts` 等薄域接线。参照实现全在 `/Users/itsuka/CodeSpace/orca`（只读），本计划中 `orca:` 前缀均为其内路径。

**Tech Stack:** Rust（ade-bridge、libc）、TypeScript（Vitest、Tauri IPC）、gh CLI（运行时外部依赖，macOS）。

**Spec:** `docs/superpowers/specs/2026-10-08-phase2d1-github-readonly-design.md`（执行者须同时阅读）

## Global Constraints

- 不新增任何 npm 依赖；Rust 仅允许 `ade-bridge` 新增 `libc = "0.2.189"`（与 ade-pty 同版本，已在 workspace lock 中）。
- bindings 唯一生成方式：`cargo run -p ade-bridge --bin export-bindings`（workdir `src-tauri`）；`bindings_are_fresh` 会校验；新命令名加入 `specta_export.rs` 的 `collect_commands!` 与 `export_lists_every_command` 清单。
- 所有提交信息用中文 conventional 格式 + `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- cargo 命令在 `/Users/itsuka/CodeSpace/ade/src-tauri` 下执行；pnpm 命令在仓库根执行。
- 平台：仅 macOS/POSIX 路径；不写 Windows/WSL 分支（披露于 spec §7）。
- 可选字段序列化时**省略**（`skip_serializing_if` / TS `undefined`），不物化 null。
- 错误分类顺序（固定，禁止改序）：`rate_limited → repo_unavailable → server_error/network → permission → gh_unavailable → auth → unknown`。
- 渲染层 TS 模块必须可注入 executor（纯函数优先），禁止模块顶层触碰 `window`（除 bridge real 域）。
- 参照代码逐字移植时保持常量/正则/argv 原样；改动处须在报告中说明。

---

### Task 1: Rust `gh_exec` 执行器 + `gh_env_probe` + AppState（并发闸/路径缓存）

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/gh.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`（加 `pub mod gh;`）
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（AppState 两个新字段 + newtype + 初始化）
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`（注册两命令 + 名称清单）
- Modify: `src-tauri/crates/ade-bridge/Cargo.toml`（`libc = "0.2.189"`）
- Test: `src-tauri/crates/ade-bridge/tests/gh_exec.rs`
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: `AppState.home`（已有）、`run_blocking`（`commands/mod.rs:22-32`）、`lock`（`state.rs:60-64`）、`BridgeError`。
- Produces（后续任务与 TS 依赖）：
  - 命令 `gh_exec(args: {args: string[], cwd?: string, timeoutMs?: number, maxBuffer?: number}) -> {stdout: string, stderr: string, code: number|null}`（非零退出是 Ok 结果）
  - 命令 `gh_env_probe() -> {token: string|null}`（`'GH_TOKEN' | 'GITHUB_TOKEN' | null`）
  - `AppState.gh_gate: Arc<GhConcurrencyGate>`、`AppState.gh_path_cache: GhPathCache`

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/crates/ade-bridge/tests/gh_exec.rs`（环境锁 + PATH EnvGuard 模式；fake gh 脚本用 preflight.rs:338-348 的 write_executable 写法）：

```rust
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static ENV_INIT: OnceLock<()> = OnceLock::new();

fn env_lock() -> MutexGuard<'static, ()> {
    let guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    ENV_INIT.get_or_init(|| ());
    guard
}

struct PathGuard {
    saved: Option<std::ffi::OsString>,
}
impl PathGuard {
    fn set(paths: &[&Path]) -> Self {
        let saved = std::env::var_os("PATH");
        let joined = std::env::join_paths(paths.iter().map(|p| p.as_os_str())).expect("join PATH");
        std::env::set_var("PATH", joined);
        Self { saved }
    }
}
impl Drop for PathGuard {
    fn drop(&mut self) {
        match &self.saved {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}

struct TestDir { path: PathBuf }
impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ade-bridge-gh-it-{name}-{}", std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test dir");
        Self { path: path.canonicalize().expect("canonicalize") }
    }
    fn file(&self, name: &str) -> PathBuf { self.path.join(name) }
    fn write_executable(&self, name: &str, body: &str) -> PathBuf {
        let path = self.file(name);
        std::fs::write(&path, body).expect("write script");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }
}
impl Drop for TestDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.path); }
}

use ade_bridge::commands::gh::{gh_env_probe_impl, gh_exec_impl, resolve_gh_path_in};
use ade_bridge::state::{GhConcurrencyGate, GhPathCache};

#[test]
fn gh_exec_returns_stdout_and_zero_code() {
    let _env = env_lock();
    let dir = TestDir::new("stdout");
    let gh = dir.write_executable("gh", "#!/bin/sh\necho hello\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024).unwrap();
    assert_eq!(result.stdout.trim(), "hello");
    assert_eq!(result.stderr, "");
    assert_eq!(result.code, Some(0));
}

#[test]
fn gh_exec_keeps_nonzero_exit_as_result() {
    let _env = env_lock();
    let dir = TestDir::new("nonzero");
    let gh = dir.write_executable("gh", "#!/bin/sh\necho boom >&2\nexit 3\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024).unwrap();
    assert_eq!(result.code, Some(3));
    assert!(result.stderr.contains("boom"));
}

#[test]
fn gh_exec_kills_process_group_on_timeout() {
    let _env = env_lock();
    let dir = TestDir::new("timeout");
    let pid_file = dir.file("grandchild.pid");
    let script = format!(
        "#!/bin/sh\nsleep 300 &\necho $! > '{}'\nsleep 300\n",
        pid_file.display()
    );
    let gh = dir.write_executable("gh", &script);
    let started = Instant::now();
    let error = gh_exec_impl(&gh, &[], None, Duration::from_millis(400), 1024).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5), "must not wait for sleep 300");
    assert!(error.to_string().contains("timed out"));
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive { break; }
        assert!(Instant::now() < deadline, "grandchild {pid} survived the group kill");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn gh_exec_enforces_max_buffer() {
    let _env = env_lock();
    let dir = TestDir::new("maxbuffer");
    let gh = dir.write_executable("gh", "#!/bin/sh\nhead -c 100000 /dev/zero | tr '\\0' 'a'\n");
    let error = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 4096).unwrap_err();
    assert!(error.to_string().contains("max buffer"));
}

#[test]
fn gh_exec_injects_prompt_disabled() {
    let _env = env_lock();
    std::env::remove_var("GH_PROMPT_DISABLED");
    let dir = TestDir::new("prompt");
    let gh = dir.write_executable("gh", "#!/bin/sh\nprintf '%s' \"$GH_PROMPT_DISABLED\"\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024).unwrap();
    assert_eq!(result.stdout, "1");
}

#[test]
fn resolve_gh_path_falls_back_to_extra_dirs() {
    let _env = env_lock();
    let dir = TestDir::new("resolve");
    let extra = dir.file("extra");
    std::fs::create_dir_all(&extra).unwrap();
    let gh = extra.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();

    let cache = GhPathCache::default();
    let empty = dir.file("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let _guard = PathGuard::set(&[&empty]);
    let resolved = resolve_gh_path_in(&cache, &[extra.clone()]).unwrap();
    assert_eq!(resolved, gh);
    // 第二次命中缓存（删除文件后仍返回缓存路径）
    std::fs::remove_file(&gh).unwrap();
    assert_eq!(resolve_gh_path_in(&cache, &[extra]).unwrap(), gh);
}

#[test]
fn gh_gate_limits_concurrency() {
    let _env = env_lock();
    let dir = TestDir::new("gate");
    let gh = dir.write_executable("gh", "#!/bin/sh\nsleep 0.25\n");
    let gate = std::sync::Arc::new(GhConcurrencyGate::new(4));
    let started = Instant::now();
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let gh = gh.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let _guard = gate.acquire();
                gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024).unwrap()
            })
        })
        .collect();
    for handle in handles { handle.join().unwrap(); }
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(450), "6 calls / 4 permits must take >=2 batches: {elapsed:?}");
    assert!(elapsed < Duration::from_millis(1500), "gate must not serialize: {elapsed:?}");
}

#[test]
fn gh_env_probe_reports_token() {
    let _env = env_lock();
    std::env::remove_var("GH_TOKEN");
    std::env::remove_var("GITHUB_TOKEN");
    assert_eq!(gh_env_probe_impl().token, None);
    std::env::set_var("GITHUB_TOKEN", "x");
    assert_eq!(gh_env_probe_impl().token.as_deref(), Some("GITHUB_TOKEN"));
    std::env::set_var("GH_TOKEN", "y");
    assert_eq!(gh_env_probe_impl().token.as_deref(), Some("GH_TOKEN"));
    std::env::remove_var("GH_TOKEN");
    std::env::remove_var("GITHUB_TOKEN");
}

#[test]
fn gh_exec_reports_missing_binary() {
    let _env = env_lock();
    let cache = GhPathCache::default();
    let dir = TestDir::new("missing");
    let error = resolve_gh_path_in(&cache, &[dir.file("nope")]).unwrap_err();
    assert!(error.to_string().contains("gh: command not found"));
}
```

注意：测试需要 `libc` 与 `ade_bridge` 作为 dev 依赖可访问（`libc` 加在 `[dependencies]`，集成测试可直接 `libc::kill`；若 `ade_bridge` 库未导出 `commands::gh`，需在 `ade-bridge/src/lib.rs` 保持 `pub mod commands;` 已有前提下工作）。

- [ ] **Step 2: 跑测试确认失败**

Run（workdir src-tauri）: `cargo test -p ade-bridge --test gh_exec`
Expected: 编译失败（`commands::gh` 不存在）。

- [ ] **Step 3: 实现**

`Cargo.toml` 的 `[dependencies]` 加 `libc = "0.2.189"`。

`state.rs` 追加（放 `GitCancelRegistry` 之后；文件顶部需确保 `use std::sync::Condvar;` 与 `use std::sync::Arc;` 已引入，按现有 import 补）：

```rust
/// gh 子进程并发闸（全局 4，FIFO 近似——Condvar 唤醒不保证顺序，可接受）。
pub struct GhConcurrencyGate {
    permits: Mutex<usize>,
    available: Condvar,
}

impl GhConcurrencyGate {
    pub fn new(max: usize) -> Self {
        Self { permits: Mutex::new(max), available: Condvar::new() }
    }

    pub fn acquire(&self) -> GhGateGuard<'_> {
        let mut permits = lock(&self.permits);
        while *permits == 0 {
            permits = self.available.wait(permits).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        *permits -= 1;
        GhGateGuard { gate: self }
    }
}

pub struct GhGateGuard<'a> { gate: &'a GhConcurrencyGate }

impl Drop for GhGateGuard<'_> {
    fn drop(&mut self) {
        let mut permits = lock(&self.gate.permits);
        *permits += 1;
        self.gate.available.notify_one();
    }
}

/// gh 可执行文件解析结果的进程内缓存（成功才写；失败每次重试）。
#[derive(Clone, Default)]
pub struct GhPathCache(Arc<Mutex<Option<std::path::PathBuf>>>);

impl GhPathCache {
    pub fn get(&self) -> Option<std::path::PathBuf> { lock(&self.0).clone() }
    pub fn set(&self, value: std::path::PathBuf) { *lock(&self.0) = Some(value); }
}
```

`AppState` struct 加字段（放在 `path_hydration_cache` 附近）：

```rust
    /// gh 执行器并发闸（规格 §3.1；全局 4）。
    pub gh_gate: Arc<GhConcurrencyGate>,
    /// gh 可执行文件解析缓存（成功才写）。
    pub gh_path_cache: GhPathCache,
```

`AppState::initialize` 的 struct literal 加 `gh_gate: Arc::new(GhConcurrencyGate::new(4)), gh_path_cache: GhPathCache::default(),`。

创建 `commands/gh.rs`：

```rust
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::state::{AppState, GhConcurrencyGate, GhPathCache};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_MAX_BUFFER: usize = 10 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GhExecArgs {
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_buffer: Option<usize>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GhExecResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GhEnvProbe {
    pub token: Option<String>,
}

/// 生产用附加搜索目录（GUI 应用 PATH 不含 Homebrew）。
fn default_extra_dirs(home: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from(home).join(".local/bin"),
        PathBuf::from(home).join("bin"),
    ]
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// 在 `PATH` 目录 + 附加目录中解析 `gh`；成功写缓存，失败每次重试。
pub fn resolve_gh_path_in(cache: &GhPathCache, extra_dirs: &[PathBuf]) -> Result<PathBuf, BridgeError> {
    if let Some(cached) = cache.get() {
        return Ok(cached);
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    dirs.extend(extra_dirs.iter().cloned());
    for dir in dirs {
        let candidate = dir.join("gh");
        if is_executable_file(&candidate) {
            cache.set(candidate.clone());
            return Ok(candidate);
        }
    }
    Err(BridgeError::message("gh: command not found on PATH"))
}

pub fn read_timeout_ms(value: Option<u64>) -> Duration {
    let millis = value
        .or_else(|| {
            std::env::var("ORCA_GH_EXEC_TIMEOUT_MS")
                .ok()
                .and_then(|raw| raw.trim().parse::<u64>().ok())
        })
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    Duration::from_millis(millis.max(1))
}

/// 运行 gh：非零退出是 Ok 结果；spawn/超时/超限是 Err。
pub fn gh_exec_impl(
    gh_path: &Path,
    args: &[String],
    cwd: Option<&str>,
    timeout: Duration,
    max_buffer: usize,
) -> Result<GhExecResult, BridgeError> {
    let mut command = Command::new(gh_path);
    command.args(args);
    if let Some(cwd) = cwd.filter(|value| !value.trim().is_empty()) {
        command.current_dir(cwd);
    }
    if std::env::var_os("GH_PROMPT_DISABLED").is_none() {
        command.env("GH_PROMPT_DISABLED", "1");
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Why: gh 常是 mise/asdf shim；独立进程组保证超时能连孙进程一起杀。
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        BridgeError::message(format!("gh: command not found (spawn failed: {error})"))
    })?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_handle = stdout_pipe.map(|pipe| spawn_reader(pipe, max_buffer, exceeded.clone()));
    let stderr_handle = stderr_pipe.map(|pipe| spawn_reader(pipe, max_buffer, exceeded.clone()));

    let deadline = Instant::now() + timeout;
    let status = loop {
        if exceeded.load(Ordering::SeqCst) {
            kill_process_group(&mut child);
            return Err(BridgeError::message("gh exec output exceeded max buffer"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                kill_process_group(&mut child);
                return Err(BridgeError::message(format!("gh exec wait failed: {error}")));
            }
        }
        if Instant::now() >= deadline {
            kill_process_group(&mut child);
            return Err(BridgeError::message("gh exec timed out"));
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let stdout = join_reader(stdout_handle)?;
    let stderr = join_reader(stderr_handle)?;
    if exceeded.load(Ordering::SeqCst) {
        return Err(BridgeError::message("gh exec output exceeded max buffer"));
    }
    Ok(GhExecResult { stdout, stderr, code: status.code() })
}

fn spawn_reader(
    mut pipe: impl Read + Send + 'static,
    max_buffer: usize,
    exceeded: Arc<AtomicBool>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut collected: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if collected.len() + read > max_buffer {
                        exceeded.store(true, Ordering::SeqCst);
                        break;
                    }
                    collected.extend_from_slice(&buffer[..read]);
                }
            }
        }
        String::from_utf8_lossy(&collected).into_owned()
    })
}

fn join_reader(handle: Option<std::thread::JoinHandle<String>>) -> Result<String, BridgeError> {
    match handle {
        Some(handle) => handle
            .join()
            .map_err(|_| BridgeError::message("gh exec reader panicked")),
        None => Ok(String::new()),
    }
}

fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // Why: 子进程自成进程组（process_group(0)），负 pid 连组内孙进程一起杀。
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}

pub fn gh_env_probe_impl() -> GhEnvProbe {
    let token = if std::env::var_os("GH_TOKEN").is_some() {
        Some("GH_TOKEN".to_string())
    } else if std::env::var_os("GITHUB_TOKEN").is_some() {
        Some("GITHUB_TOKEN".to_string())
    } else {
        None
    };
    GhEnvProbe { token }
}

#[tauri::command]
#[specta::specta]
pub async fn gh_exec(state: State<'_, AppState>, args: GhExecArgs) -> Result<GhExecResult, BridgeError> {
    let gate = Arc::clone(&state.gh_gate);
    let cache = state.gh_path_cache.clone();
    let extra_dirs = default_extra_dirs(&state.home);
    run_blocking(move || {
        let _guard = gate.acquire();
        let gh_path = resolve_gh_path_in(&cache, &extra_dirs)?;
        let timeout = read_timeout_ms(args.timeout_ms);
        let max_buffer = args.max_buffer.unwrap_or(DEFAULT_MAX_BUFFER);
        gh_exec_impl(&gh_path, &args.args, args.cwd.as_deref(), timeout, max_buffer)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn gh_env_probe() -> Result<GhEnvProbe, BridgeError> {
    Ok(gh_env_probe_impl())
}
```

`commands/mod.rs` 加 `pub mod gh;`。`specta_export.rs`：`collect_commands!` 加 `commands::gh::gh_exec, commands::gh::gh_env_probe,`，`export_lists_every_command` 清单加 `"gh_exec", "gh_env_probe",`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p ade-bridge --test gh_exec`
Expected: 9 条全 PASS（并发闸用例约 0.5s）。

- [ ] **Step 5: 重生成 bindings + 全 crate 测试**

Run: `cargo run -p ade-bridge --bin export-bindings`
Run: `cargo test -p ade-bridge`
Expected: `bindings_are_fresh` 绿，全部既有测试不回归。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/ade-bridge/Cargo.toml \
  src-tauri/crates/ade-bridge/src/commands/gh.rs \
  src-tauri/crates/ade-bridge/src/commands/mod.rs \
  src-tauri/crates/ade-bridge/src/state.rs \
  src-tauri/crates/ade-bridge/src/specta_export.rs \
  src-tauri/crates/ade-bridge/tests/gh_exec.rs \
  src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): gh 执行器（PATH 解析/超时进程组杀/并发闸）与 env 探针

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Rust `git_remote_urls`

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/git.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Test: `src-tauri/crates/ade-bridge/tests/git_commands.rs`
- Regenerate: bindings

**Interfaces:**
- Consumes: `ade_git::runner::run_git_in(cwd: &str, args: &[&str], timeout: Duration, cancel: Option<&CancelToken>) -> Result<GitOutput, CoreError>`（`ade-git/src/runner.rs:40`，pub）；`require_authorized_worktree`、`GitWorktreeArgs`、`run_blocking`。
- Produces: 命令 `git_remote_urls({worktreePath: string}) -> {name: string, url: string}[]`（仅 fetch URL，按首次出现去重）。

- [ ] **Step 1: 写失败测试**

在 `tests/git_commands.rs` 末尾追加（该文件已有 `TestDir`/`git()`/`init_git_repo` 辅助）：

```rust
#[test]
fn remote_urls_returns_fetch_urls_deduped() {
    let dir = TestDir::new("remote-urls");
    let repo = init_git_repo(&dir, "repo");
    git(&repo, &["remote", "add", "origin", "git@github.com:owner/repo.git"]);
    git(&repo, &["remote", "add", "upstream", "https://github.com/up/repo.git"]);

    let urls = remote_urls_impl(repo.to_str().unwrap()).unwrap();
    assert_eq!(urls.len(), 2);
    assert_eq!(urls[0].name, "origin");
    assert_eq!(urls[0].url, "git@github.com:owner/repo.git");
    assert_eq!(urls[1].name, "upstream");
    assert_eq!(urls[1].url, "https://github.com/up/repo.git");
}

#[test]
fn remote_urls_is_empty_without_remotes() {
    let dir = TestDir::new("remote-urls-empty");
    let repo = init_git_repo(&dir, "repo");
    assert!(remote_urls_impl(repo.to_str().unwrap()).unwrap().is_empty());
}
```

（若 `git_commands.rs` 的 `init_git_repo` 返回 `PathBuf`，按该文件现有用法调整；`git()` 是既有辅助。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p ade-bridge --test git_commands remote_urls`
Expected: 编译失败（`remote_urls_impl` 不存在）。

- [ ] **Step 3: 实现**

`git.rs` 追加：

```rust
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitRemoteUrl {
    pub name: String,
    pub url: String,
}

/// `git remote -v` 的 fetch URL，按 remote 名首次出现去重。
pub fn parse_remote_urls(stdout: &str) -> Vec<GitRemoteUrl> {
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for line in stdout.lines() {
        let Some((name, rest)) = line.split_once('\t') else { continue };
        let Some((url, kind)) = rest.rsplit_once(' ') else { continue };
        if kind != "(fetch)" || !seen.insert(name.to_string()) {
            continue;
        }
        rows.push(GitRemoteUrl { name: name.to_string(), url: url.to_string() });
    }
    rows
}

pub fn remote_urls_impl(worktree_path: &str) -> Result<Vec<GitRemoteUrl>, BridgeError> {
    let output = ade_git::runner::run_git_in(
        worktree_path,
        &["remote", "-v"],
        std::time::Duration::from_secs(10),
        None,
    )?;
    Ok(parse_remote_urls(&output.stdout))
}

#[tauri::command]
#[specta::specta]
pub async fn git_remote_urls(
    state: State<'_, AppState>,
    args: GitWorktreeArgs,
) -> Result<Vec<GitRemoteUrl>, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || remote_urls_impl(&args.worktree_path)).await
}
```

注意 `git.rs` 顶部 import 需补 `serde::Serialize`（若未引入）与 `specta` 相关（该文件已有其它 `specta::Type` 结构体，按现有 import 补）。`specta_export.rs` 注册 `commands::git::git_remote_urls` + 名称清单。

- [ ] **Step 4: 跑测试 + bindings**

Run: `cargo test -p ade-bridge --test git_commands remote_urls`
Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/ade-bridge/src/commands/git.rs \
  src-tauri/crates/ade-bridge/src/specta_export.rs \
  src-tauri/crates/ade-bridge/tests/git_commands.rs \
  src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): git_remote_urls 命令（身份解析输入）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: TS gh-exec-client + auth-diagnose + 错误分类

**Files:**
- Create: `src/renderer/src/lib/github/gh-exec-client.ts`
- Create: `src/renderer/src/lib/github/auth-diagnose.ts`
- Create: `src/renderer/src/lib/github/gh-error-classification.ts`
- Test: `src/renderer/src/lib/github/gh-exec-client.test.ts`、`auth-diagnose.test.ts`、`gh-error-classification.test.ts`

**Interfaces:**
- Produces:
  - `type GhExecResult = { stdout: string; stderr: string; code: number | null }`
  - `type GhExecutor = (args: string[], options?: { cwd?: string; timeoutMs?: number; maxBuffer?: number }) => Promise<GhExecResult>`
  - `createGhExecClient(executor: GhExecutor): { run(args, options?): Promise<GhExecResult>; runOrThrow(args, options?): Promise<string> }`（`run` 带只读重试；`runOrThrow` 非零/异常时抛 `GhRunError`）
  - `class GhRunError extends Error { stderr: string; stdout: string; code: number | null }`
  - `defaultGhExecutor(): GhExecutor`（`invokeCommand('gh_exec', {args})`）
  - `parseAuthStatus(text: string): GhAuthAccount[]`、`computeAuthDiagnostic(input): GhAuthDiagnostic`（纯函数）
  - `classifyPRRefreshError(error: unknown): PRRefreshErrorType`、`safePRRefreshErrorMessage(type): string`

- [ ] **Step 1: 写失败测试**

`gh-exec-client.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import { createGhExecClient, GhRunError } from './gh-exec-client'

describe('gh exec client', () => {
  it('returns the executor result on success', async () => {
    const executor = vi.fn(async () => ({ stdout: 'ok', stderr: '', code: 0 }))
    const client = createGhExecClient(executor)
    await expect(client.run(['auth', 'status'])).resolves.toEqual({ stdout: 'ok', stderr: '', code: 0 })
    expect(executor).toHaveBeenCalledTimes(1)
  })

  it('retries transient failures up to 3 attempts', async () => {
    vi.useFakeTimers()
    const executor = vi
      .fn()
      .mockResolvedValueOnce({ stdout: '', stderr: 'HTTP 502 Bad Gateway', code: 1 })
      .mockResolvedValueOnce({ stdout: 'ok', stderr: '', code: 0 })
    const client = createGhExecClient(executor)
    const pending = client.run(['api', 'rate_limit'])
    await vi.runAllTimersAsync()
    await expect(pending).resolves.toMatchObject({ stdout: 'ok' })
    expect(executor).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })

  it('does not retry rate-limit stderr with Retry-After', async () => {
    const executor = vi.fn(async () => ({
      stdout: '',
      stderr: 'HTTP 429: rate limit exceeded\nRetry-After: 60',
      code: 1
    }))
    const client = createGhExecClient(executor)
    await expect(client.run(['api', 'rate_limit'])).resolves.toMatchObject({ code: 1 })
    expect(executor).toHaveBeenCalledTimes(1)
  })

  it('runOrThrow raises GhRunError carrying stderr', async () => {
    const executor = vi.fn(async () => ({ stdout: '', stderr: 'boom', code: 2 }))
    const client = createGhExecClient(executor)
    await expect(client.runOrThrow(['pr', 'view', '1'])).rejects.toMatchObject({
      name: 'GhRunError',
      stderr: 'boom',
      code: 2
    })
  })
})
```

`auth-diagnose.test.ts`（fixture 逐字来自参照测试形态）：

```ts
import { describe, expect, it } from 'vitest'
import { computeAuthDiagnostic, parseAuthStatus } from './auth-diagnose'

const TWO_HOSTS = `github.com
  ✓ Logged in to github.com account alice (keyring)
  - Active account: true
  - Token scopes: 'gist', 'read:org', 'repo'

ghe.internal:8443
  ✓ Logged in to ghe.internal:8443 account bob (GITHUB_TOKEN)
  - Active account: false
  - Token scopes: 'repo'
`

describe('parseAuthStatus', () => {
  it('parses multiple hosts, env tokens, scopes, and active flag', () => {
    const accounts = parseAuthStatus(TWO_HOSTS)
    expect(accounts).toHaveLength(2)
    expect(accounts[0]).toMatchObject({
      host: 'github.com', user: 'alice', active: true, envToken: null, source: 'keyring',
      scopes: ['gist', 'read:org', 'repo']
    })
    expect(accounts[1]).toMatchObject({
      host: 'ghe.internal:8443', user: 'bob', active: false, envToken: 'GITHUB_TOKEN', source: 'env'
    })
  })

  it('returns empty for empty input and tolerates missing host header', () => {
    expect(parseAuthStatus('')).toEqual([])
    const accounts = parseAuthStatus('  ✓ Logged in to github.com account carol (keyring)\n')
    expect(accounts[0]?.host).toBe('github.com')
  })
})

describe('computeAuthDiagnostic', () => {
  it('computes missing scopes, keyring fallback, and env token in process', () => {
    const accounts = parseAuthStatus(TWO_HOSTS)
    const diag = computeAuthDiagnostic({ accounts, ghAvailable: true, envTokenInProcess: 'GH_TOKEN', requiredHost: null })
    expect(diag.activeAccount?.user).toBe('alice')
    expect(diag.missingScopes).toEqual(['project'])
    expect(diag.requiredScopes).toEqual(['project', 'read:org', 'repo'])
    expect(diag.envTokenInProcess).toBe('GH_TOKEN')
    expect(diag.hasKeyringFallback).toBe(false)
    expect(diag.requiredHostAuthenticated).toBeNull()
  })

  it('scopes to a required host', () => {
    const accounts = parseAuthStatus(TWO_HOSTS)
    const diag = computeAuthDiagnostic({
      accounts, ghAvailable: true, envTokenInProcess: null, requiredHost: 'GHE.INTERNAL:8443'
    })
    expect(diag.activeAccount?.user).toBe('bob')
    expect(diag.requiredHost).toBe('ghe.internal:8443')
    expect(diag.requiredHostAuthenticated).toBe(true)
  })

  it('reports gh unavailable with no accounts', () => {
    const diag = computeAuthDiagnostic({
      accounts: [], ghAvailable: false, envTokenInProcess: null, requiredHost: null
    })
    expect(diag.ghAvailable).toBe(false)
    expect(diag.activeAccount).toBeNull()
    expect(diag.missingScopes).toEqual(['project', 'read:org', 'repo'])
  })
})
```

`gh-error-classification.test.ts`：逐类断言（至少 8 条）：`HTTP 429`→rate_limited、`HTTP 404`→repo_unavailable、`HTTP 503`→server_error、`ECONNRESET`→network、`HTTP 403 resource not accessible`→permission、`spawn gh ENOENT`→gh_unavailable、`HTTP 401 bad credentials`→auth、`something else`→unknown；另断言 `safePRRefreshErrorMessage` 对每类返回稳定文案（逐字使用参照文案，见实现）。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现**

`gh-exec-client.ts`：

```ts
import { invokeCommand } from '@/bridge/real/invoke'
import type { GhAuthAccount, GhAuthDiagnostic } from '../../../../shared/github/auth-types'

export type GhExecResult = { stdout: string; stderr: string; code: number | null }
export type GhExecOptions = { cwd?: string; timeoutMs?: number; maxBuffer?: number }
export type GhExecutor = (args: string[], options?: GhExecOptions) => Promise<GhExecResult>

export class GhRunError extends Error {
  readonly stderr: string
  readonly stdout: string
  readonly code: number | null

  constructor(result: GhExecResult) {
    super(result.stderr.trim() || `gh exited with code ${result.code ?? 'unknown'}`)
    this.name = 'GhRunError'
    this.stderr = result.stderr
    this.stdout = result.stdout
    this.code = result.code
  }
}

const RETRY_DELAYS_MS = [250, 1000]
const TRANSIENT_PATTERNS = [
  /http[\s/]*5\d\d/i,
  /internal server error/i,
  /bad gateway/i,
  /service unavailable/i,
  /econnreset|etimedout|socket hang up/i
]

function isTransientFailure(result: GhExecResult): boolean {
  const text = `${result.stderr}\n${result.stdout}`
  if (/retry-after:/i.test(text)) {
    return false
  }
  return TRANSIENT_PATTERNS.some((pattern) => pattern.test(text))
}

export function createGhExecClient(executor: GhExecutor): {
  run: (args: string[], options?: GhExecOptions) => Promise<GhExecResult>
  runOrThrow: (args: string[], options?: GhExecOptions) => Promise<string>
} {
  const run = async (args: string[], options?: GhExecOptions): Promise<GhExecResult> => {
    let last: GhExecResult | null = null
    for (let attempt = 0; attempt <= RETRY_DELAYS_MS.length; attempt++) {
      try {
        const result = await executor(args, options)
        if (result.code === 0 && result.code !== null) {
          return result
        }
        last = result
        if (!isTransientFailure(result)) {
          return result
        }
      } catch (error) {
        last = {
          stdout: '',
          stderr: error instanceof Error ? error.message : String(error),
          code: null
        }
        if (!/timeout|timed out|econnreset|socket hang up/i.test(last.stderr)) {
          throw error
        }
      }
      const delay = RETRY_DELAYS_MS[attempt]
      if (delay !== undefined) {
        await new Promise<void>((resolve) => setTimeout(resolve, delay))
      }
    }
    return last ?? { stdout: '', stderr: 'gh exec failed', code: null }
  }
  const runOrThrow = async (args: string[], options?: GhExecOptions): Promise<string> => {
    const result = await run(args, options)
    if (result.code !== 0) {
      throw new GhRunError(result)
    }
    return result.stdout
  }
  return { run, runOrThrow }
}

export function defaultGhExecutor(): GhExecutor {
  return (args, options) =>
    invokeCommand<GhExecResult>('gh_exec', {
      args: {
        args,
        ...(options?.cwd !== undefined ? { cwd: options.cwd } : {}),
        ...(options?.timeoutMs !== undefined ? { timeoutMs: options.timeoutMs } : {}),
        ...(options?.maxBuffer !== undefined ? { maxBuffer: options.maxBuffer } : {})
      }
    })
}

export type { GhAuthAccount, GhAuthDiagnostic }
```

`auth-diagnose.ts`（解析器逐字移植 `orca:src/main/github/auth-diagnose.ts:31-91`，计算字段逐字移植 `:93-157`，改为纯函数）：

```ts
import type { GhAuthAccount, GhAuthDiagnostic } from '../../../../shared/github/auth-types'

const REQUIRED_SCOPES = ['project', 'read:org', 'repo'] as const

export function parseAuthStatus(text: string): GhAuthAccount[] {
  // 逐字移植 orca:src/main/github/auth-diagnose.ts:31-91（host 头正则、Logged in 行、
  // Active account、Token scopes 剥离引号；host 从登录行回退；空输入 → []）
}

export function computeAuthDiagnostic(input: {
  accounts: GhAuthAccount[]
  ghAvailable: boolean
  envTokenInProcess: 'GITHUB_TOKEN' | 'GH_TOKEN' | null
  requiredHost?: string | null
}): GhAuthDiagnostic {
  // 逐字移植 orca:src/main/github/auth-diagnose.ts:120-156：
  // normalizedRequiredHost = trim+lowercase || null；hostAccounts 过滤；
  // active = hostAccounts.active ?? hostAccounts[0] ?? (无 requiredHost 时 accounts.active ?? accounts[0] ?? null)；
  // missingScopes = REQUIRED_SCOPES 差集（无 active 时为全集）；
  // hasKeyringFallback = active 同 host 存在不同 keyring 账号；
  // requiredHostAuthenticated = requiredHost ? hostAccounts.length > 0 : null。
}
```

（执行者：把参照代码粘贴进来并机械改写 `process.env` → 入参 `envTokenInProcess`、`ghExecFileAsync` 移除；不得改动正则与字段名。）

`gh-error-classification.ts`（逐字移植 `orca:src/main/github/pr-refresh-error-classification.ts:20-143` + `safePRRefreshErrorMessage`；`extractExecError` 的等价物：从 `GhRunError`/`unknown` 读 `stderr/stdout/message/code`；`classifyGitHubUnavailable` 直接 import 端口已有 `../../../../shared/github/api-availability`）。分类顺序与正则一字不改。

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/lib/github`
Run: `pnpm typecheck`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/lib/github/gh-exec-client.ts \
  src/renderer/src/lib/github/gh-exec-client.test.ts \
  src/renderer/src/lib/github/auth-diagnose.ts \
  src/renderer/src/lib/github/auth-diagnose.test.ts \
  src/renderer/src/lib/github/gh-error-classification.ts \
  src/renderer/src/lib/github/gh-error-classification.test.ts
git commit -m "feat(renderer): gh 执行客户端、auth 诊断与 PR 错误分类

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: TS 仓库身份（repoSlug / repoUpstream / GHES 门 + 缓存）

**Files:**
- Create: `src/renderer/src/lib/github/repo-identity.ts`
- Test: `src/renderer/src/lib/github/repo-identity.test.ts`

**Interfaces:**
- Consumes: Task 3 的 `createGhExecClient`/`GhExecutor`；`git_remote_urls` 命令（`invokeCommand('git_remote_urls', {args:{worktreePath}})`）；`shared/git-remote-identity.ts` 的 `deriveGitRemoteIdentity(stdout)`/`splitGitRemoteKey`；`shared/git-remote-host-alias.ts` 的 `normalizeGitHubRemoteHost`；`shared/github/repository-identity-key.ts` 的 `githubRepoIdentityKey`；Task 3 的 `parseAuthStatus`。
- Produces:
  - `type GitHubRepoIdentity = { owner: string; repo: string; host?: string }`
  - `createRepoIdentityResolver(deps: { client: GhExecClient; readRemoteUrls: (worktreePath: string) => Promise<{name: string; url: string}[]>; now?: () => number })` 返回：
    - `resolveCandidates(worktreePath): Promise<{ candidates: GitHubRepoIdentity[]; headRepo: GitHubRepoIdentity | null }>`
    - `getRepoSlug(worktreePath): Promise<GitHubRepoIdentity | null>`
    - `getRepoUpstream(worktreePath): Promise<GitHubRepoIdentity | null>`

- [ ] **Step 1: 写失败测试**

```ts
import { describe, expect, it, vi } from 'vitest'
import { createRepoIdentityResolver } from './repo-identity'

function makeResolver(remotes: Record<string, {name:string;url:string}[]>) {
  const client = { run: vi.fn(), runOrThrow: vi.fn() }
  let nowValue = 0
  const resolver = createRepoIdentityResolver({
    client: client as never,
    readRemoteUrls: async (path) => remotes[path] ?? [],
    now: () => nowValue
  })
  return { resolver, client, advance: (ms: number) => { nowValue += ms } }
}

describe('repo identity', () => {
  it('derives upstream-first candidates with origin as head repo', async () => {
    const { resolver } = makeResolver({
      '/repo': [
        { name: 'origin', url: 'git@github.com:me/fork.git' },
        { name: 'upstream', url: 'https://github.com/org/repo.git' }
      ]
    })
    const { candidates, headRepo } = await resolver.resolveCandidates('/repo')
    expect(candidates).toEqual([
      { owner: 'org', repo: 'repo', host: undefined },
      { owner: 'me', repo: 'fork', host: undefined }
    ])
    expect(headRepo).toEqual({ owner: 'me', repo: 'fork', host: undefined })
  })

  it('returns origin only when upstream is absent or identical', async () => {
    const { resolver } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://github.com/org/repo.git' }]
    })
    const { candidates, headRepo } = await resolver.resolveCandidates('/repo')
    expect(candidates).toEqual([{ owner: 'org', repo: 'repo', host: undefined }])
    expect(headRepo?.owner).toBe('org')
  })

  it('ignores non-GitHub remotes', async () => {
    const { resolver } = makeResolver({
      '/repo': [{ name: 'origin', url: 'git@gitlab.com:org/repo.git' }]
    })
    const { candidates, headRepo } = await resolver.resolveCandidates('/repo')
    expect(candidates).toEqual([])
    expect(headRepo).toBeNull()
  })

  it('caches positive identity for 30s and negative for 5min', async () => {
    const remotes = { '/repo': [{ name: 'origin', url: 'https://github.com/org/repo.git' }] }
    const { resolver, advance } = makeResolver(remotes)
    const readSpy = vi.spyOn(remotes, '0' as never)
    await resolver.resolveCandidates('/repo')
    remotes['/repo'] = []
    await resolver.resolveCandidates('/repo') // 30s 内命中正缓存
    expect((await resolver.resolveCandidates('/repo')).candidates.length).toBe(1)
    advance(31_000)
    expect((await resolver.resolveCandidates('/repo')).candidates.length).toBe(0)
    remotes['/repo'] = [{ name: 'origin', url: 'https://github.com/org/repo.git' }]
    await resolver.resolveCandidates('/repo') // 负缓存 5min 内不重读
    expect((await resolver.resolveCandidates('/repo')).candidates.length).toBe(0)
    advance(5 * 60_000 + 1)
    expect((await resolver.resolveCandidates('/repo')).candidates.length).toBe(1)
  })

  it('gates non-default hosts on gh auth inventory', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'git@ghe.internal:8443/org/repo.git' }]
    })
    client.run.mockResolvedValueOnce({
      stdout: 'ghe.internal:8443\n  ✓ Logged in to ghe.internal:8443 account bob (keyring)\n  - Active account: true\n  - Token scopes: \'repo\'\n',
      stderr: '',
      code: 0
    })
    const slug = await resolver.getRepoSlug('/repo')
    expect(slug).toEqual({ owner: 'org', repo: 'repo', host: 'ghe.internal:8443' })
  })

  it('treats unauthenticated GHES hosts as non-GitHub', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'git@ghe.internal:8443/org/repo.git' }]
    })
    client.run.mockResolvedValueOnce({ stdout: '', stderr: '', code: 0 })
    expect(await resolver.getRepoSlug('/repo')).toBeNull()
  })

  it('resolves upstream via gh repo view parent when no upstream remote', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://github.com/me/fork.git' }]
    })
    client.runOrThrow.mockResolvedValueOnce(
      JSON.stringify({ isFork: true, parent: { name: 'repo', owner: { login: 'org' } } })
    )
    await expect(resolver.getRepoUpstream('/repo')).resolves.toEqual({
      owner: 'org', repo: 'repo', host: undefined
    })
    expect(client.runOrThrow).toHaveBeenCalledWith(
      ['repo', 'view', 'me/fork', '--json', 'isFork,parent'],
      { timeoutMs: 10_000 }
    )
  })
})
```

（测试写法以实际实现接口为准微调；核心断言不变。负缓存用例中删掉无意义的 readSpy 行。）

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github/repo-identity.test.ts`
Expected: FAIL。

- [ ] **Step 3: 实现**

`repo-identity.ts` 要点（执行者按此实现，行为对齐参照）：

- `resolveCandidates(worktreePath)`：
  1. `readRemoteUrls` → 重建成 `git remote -v` 文本（`${name}\t${url} (fetch)\n`）→ `deriveGitRemoteIdentity(text)`（upstream→origin→其它优先级由该助手决定）；若返回 null → 回退逐条解析。
  2. 用 `splitGitRemoteKey(identity.canonicalKey, normalizeGitHubRemoteHost)` 拆 host/tail，再拆 `owner/repo`（tail 恰好两段且 `.git` 已由助手剥离）；host 经 `normalizeGitHubRemoteHost`（`ssh.github.com`→`github.com`）；非 GitHub 形态（尾段数不对、host 为 gitlab 等）→ 该条丢弃。
  3. 构造：origin（若存在）为 headRepo；upstream 存在且 `githubRepoIdentityKey` 与 origin 不同 → 候选 `[upstream, origin]`，否则 `[origin]`。
  4. 缓存：key `${worktreePath}\0candidates`；正 30s（`POSITIVE_TTL_MS`），负 5min（`NEGATIVE_TTL_MS`）；命中 TTL 直接返回；"indeterminate"（例如读取抛错）不写缓存、不吞错（向上抛由调用方处理）。
- `getRepoSlug(worktreePath)`：取 origin 候选；host 为空/`github.com` → 直接返回；否则 `ensureHostAuthenticated(host)`（`client.run(['auth','status'])` 容错解析 `parseAuthStatus`，结果缓存 60s，含负缓存；`gh` 非零也解析 stdout+stderr）→ 未鉴权返回 null；host 精确匹配（trim+lowercase，含 `host:port`）。
- `getRepoUpstream(worktreePath)`：
  1. origin 不可解析 → null。
  2. upstream 候选与 origin 不同 → 返回 upstream。
  3. 否则 `client.runOrThrow(['repo','view', hostPrefix+`${origin.owner}/${origin.repo}`, '--json','isFork,parent'], { timeoutMs: 10_000 })`（hostPrefix = origin.host ? `${origin.host}/` : ''）；解析 `{isFork,parent}`；`isFork && parent.owner.login && parent.name` → `{owner, repo, host: origin.host}`；异常/不满足 → null。

- [ ] **Step 4: 跑测试 + typecheck**

Run: `pnpm vitest run src/renderer/src/lib/github/repo-identity.test.ts`
Run: `pnpm typecheck`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/lib/github/repo-identity.ts src/renderer/src/lib/github/repo-identity.test.ts
git commit -m "feat(renderer): GitHub 仓库身份解析与缓存（GHES 鉴权门）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: TS PR-for-branch（阶梯 + outcome + merged-implicit 隐藏）

**Files:**
- Create: `src/renderer/src/lib/github/pr-for-branch.ts`
- Test: `src/renderer/src/lib/github/pr-for-branch.test.ts`

**Interfaces:**
- Consumes: Task 4 `resolveCandidates`；Task 3 `createGhExecClient`/`GhExecutor`；Task 3 `classifyPRRefreshError`/`safePRRefreshErrorMessage`；`shared/pr-check-status.ts` 的 `derivePRCheckStatusFromRollup(rollup)`；`shared/github/pull-request-for-branch-outcome.ts`（仅类型）。
- Produces:
  - `const PR_LOOKUP_JSON_FIELDS = 'number,title,state,url,statusCheckRollup,updatedAt,isDraft,mergeable,reviewDecision,mergeStateStatus,autoMergeRequest,baseRefName,headRefName,baseRefOid,headRefOid'`
  - `const PR_BRANCH_LIST_JSON_FIELDS = 'number,title,state,url,statusCheckRollup,updatedAt,isDraft,mergeable,baseRefName,headRefName,baseRefOid,headRefOid'`
  - `createPRForBranchLookup(deps): { getPRForBranch(args): Promise<PRInfo | null>; getPRForBranchOutcome(args): Promise<PRRefreshOutcome> }`
  - 入参 `args = { worktreePath: string; branch: string; linkedPRNumber?: number|null; fallbackPRNumber?: number|null; acceptMergedFallbackPR?: boolean; currentHeadOid?: string|null }`

- [ ] **Step 1: 写失败测试**

覆盖（fake executor 断言 argv 序列与结果）：
1. 空 branch 且无 linked/fallback → `no-pr`（不 spawn）。
2. linkedPRNumber 命中：`gh pr view N --repo O/R --json <PR_LOOKUP_JSON_FIELDS>` → found，`checksStatus` 由 rollup 派生、`headSha=headRefOid`、`prRepo`/`headRepo` 填充。
3. 分支命中（REST）：`gh api repos/O/R/pulls?head=HEADOWNER%3Abranch&state=all&per_page=1` 返回数组 → hydrate `gh pr view N ...` → found。
4. REST 抛错且 headRepo 未知 → 回退 `gh pr list --repo O/R --head branch --state all --limit 1 --json <PR_BRANCH_LIST_JSON_FIELDS>`。
5. 分支未命中 + fallbackPRNumber → 精确查询命中。
6. `gh pr view` 404 → null/`no-pr`（不抛）。
7. merged-implicit 隐藏：state=MERGED、无 linked、`currentHeadOid` 与 `headRefOid` 不同 → `no-pr`；相同 → found。
8. 分类：`HTTP 403 resource not accessible` → `upstream-error.permission`；`HTTP 429` → `rate_limited`。
9. legacy `getPRForBranch` 返回 `PRInfo|null`（found 解包）。

关键 argv 断言逐字：

```ts
expect(executor).toHaveBeenCalledWith(
  ['api', 'repos/org/repo/pulls?head=me%3Afeature&state=all&per_page=1'],
  expect.anything()
)
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github/pr-for-branch.test.ts`

- [ ] **Step 3: 实现**

逐字/行为对齐移植（参照路径为 oracle）：
- `mapPRState`（`orca:src/main/github/mappers.ts:112-124`，逐字）：MERGED→merged、CLOSED→closed、isDraft→draft、否则 open。
- `mapRestPRMergeable`、`derivePullRequestMergeable`、`mapRestPullRequest`、`isMergedImplicitPR`、`shouldHideMergedImplicitPR`、`normalizePullRequestLookupData`（`orca:.../lookup/pull-request-lookup-data.ts:82-182`，逐字；`getCurrentHeadOid` 不移植——head oid 由入参提供）。
- `normalizePRMergeable`/`normalizeReviewDecision`/`isAutoMergeEnabled`（`orca:.../map/work-item-field-coercion.ts:166-185`，逐字）。
- 分支查询（`pr-branch-lookup.ts`）：REST head（`getRestPRForBranch`）与 `gh pr list`（`getFallbackPRListForBranch`）argv 逐字；hydrate 用 `gh pr view N --repo O/R --json PR_LOOKUP_JSON_FIELDS`；分支命中失败但 headRepo 已知时抛错（对齐参照 `if (args.headRepo) throw err`）。
- 精确查询（`pr-number-lookup.ts`）：`gh pr view N --repo O/R --json PR_LOOKUP_JSON_FIELDS`；404/`not_found` → null；其它错误降级 `gh api repos/O/R/pulls/N`；REST 404 → null。
- 主流程（`branch-lookup-resolution.ts` 的 2D.1 子集）：candidates 空且 branch 空 → no-pr；linked → 精确查询（遍历候选，not_found 继续，其它错误停止并上抛）；否则分支查询（遍历候选）；未命中且 fallbackPRNumber → 精确查询；merged-implicit 隐藏（用 `currentHeadOid`）；成功 → 组装 outcome。
- 组装（`pr-refresh-outcome-assembly.ts` 去除 stack 分支）：`checksStatus = derivePRCheckStatusFromRollup(data.statusCheckRollup)`；可选字段仅在 `!== undefined` 时携带；`prRepo = dataRepo ?? undefined`、`headRepo = dataHeadRepo ?? undefined`。
- `conflictSummary`：移植 `orca:src/main/github/client/lookup/branch-lookup-derived-data.ts`（该文件较小；照抄其条件与字段，输入为 lookup data + mergeStateStatus）。
- 错误包装：`catch → { kind:'upstream-error', errorType: classifyPRRefreshError(err), message: safePRRefreshErrorMessage(type), fetchedAt: Date.now(), ...(rate_limited 且 stderr 有 Retry-After 时 nextAutoRetryAt/retryDisabledUntil) }`（参照 `gh-error-predicates.ts:23-43`；`parseRetryAfterMs` 移植自 `orca:src/main/git/exec-error.ts:53-67`）。

- [ ] **Step 4: 跑测试 + typecheck**

Run: `pnpm vitest run src/renderer/src/lib/github/pr-for-branch.test.ts && pnpm typecheck`

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/lib/github/pr-for-branch.ts src/renderer/src/lib/github/pr-for-branch.test.ts
git commit -m "feat(renderer): PR-for-branch 查询阶梯与 outcome 组装

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: TS checks 列表与详情

**Files:**
- Create: `src/renderer/src/lib/github/pr-checks.ts`
- Create: `src/renderer/src/lib/github/pr-check-details.ts`
- Test: `src/renderer/src/lib/github/pr-checks.test.ts`、`pr-check-details.test.ts`

**Interfaces:**
- Produces:
  - `PR_CHECKS_ROLLUP_QUERY`（GraphQL 查询，逐字）
  - `createPRChecksClient(deps): { getPRChecks(args): Promise<PRCheckDetail[]>; getPRCheckDetails(args): Promise<PRCheckRunDetails | null> }`
  - `args.getPRChecks = { repo: GitHubRepoIdentity; prNumber: number; headSha?: string; noCache?: boolean }`
  - `args.getPRCheckDetails = { repo: GitHubRepoIdentity; checkRunId?: number; workflowRunId?: number; checkName?: string; url?: string }`

- [ ] **Step 1: 写失败测试**

`pr-checks.test.ts` 覆盖：
1. GraphQL 成功：断言 argv 含 `['api','graphql','--cache','60s','-f','owner=org','-f','repo=repo','-F','pr=7','-f',`query=${PR_CHECKS_ROLLUP_QUERY}`]`；fixture 返回 CheckRun + StatusContext + action_required suite → 断言 `PRCheckDetail[]`（checkRunId/workflowRunId 从 `checkSuite.workflowRun.databaseId` 与 URL 解析、legacy status 名称去重、action_required 合成条目名称含 suite id）。
2. `noCache:true` 时 argv 不含 `--cache`。
3. GraphQL 抛错 + headSha 提供 → REST 三连（check-runs/status/check-suites）→ 合并。
4. GraphQL 与 REST 均空 → `gh pr checks N --json name,state,link` 兜底；`no checks reported` stderr → `[]`。
5. 状态/结论映射表：`mapCheckStatus`/`mapCheckConclusion`（逐字移植 `orca:src/main/github/mappers.ts:69-110`，含 `STALE|STARTUP_FAILURE→failure`、`PENDING|QUEUED|IN_PROGRESS→pending`）与 `conclusionMap`（`:20-30`）。

`pr-check-details.test.ts` 覆盖：
1. `checkRunId` → `gh api repos/org/repo/check-runs/42` + annotations（`?per_page=20`）+ `actions/runs/<wid>/jobs?per_page=100`；断言字段映射（title/summary/text/startedAt/completedAt/annotations/jobs，`logTail: null`）。
2. annotations 失败非致命（jobs 仍在，annotations=[]）。
3. jobs 过滤：存在与 checkName 精确匹配的 job → 只返回匹配项。
4. 超时：executor 永不返回 + `timeoutMs:25_000` 被传递；fake 用 fake timers 触发 → 抛出精确消息 `Timed out loading check details.`（常量 `GITHUB_CHECK_DETAILS_HOST_TIMEOUT_MS = 25_000` 移植到本模块或 `shared/github/check-details-deadline.ts`——端口尚无该文件，创建 `src/shared/github/check-details-deadline.ts` 逐字移植 `orca:src/shared/github/check-details-deadline.ts`）。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github/pr-checks.test.ts src/renderer/src/lib/github/pr-check-details.test.ts`

- [ ] **Step 3: 实现**

- `pr-checks.ts`：GraphQL 查询逐字（`orca:.../check/pr-checks-graphql-query.ts:1-53`）；argv 逐字（`get-pr-checks.ts:183-200`，`-f owner`/`-f repo`/`-F pr`/`-f query`，`--cache 60s` 条件）；REST 兜底 argv 逐字（`:45-92`，含 `per_page=100`、legacy 去重、`action_required` suite 合成）；`gh pr checks` 兜底 argv 逐字（`:146-166`）与 `no checks reported` 处理；映射函数逐字移植（`mappers.ts` + `pr-checks-response-mapping.ts:31-186`，含 `getPendingApprovalCheckSuiteName/Url`、`githubRepositoryWebHost`（`repository.host ?? 'github.com'`）、`parseActionsRunId`）。
- `pr-check-details.ts`：argv 逐字（`get-pr-check-details.ts:57-95`）；25s 宿主死线（`Promise.race`/`setTimeout` + 抛精确消息）；annotations/jobs 失败非致命；映射逐字（`check-detail-field-mapping.ts:10-71`，`logTail: null`，`mapWorkflowJobs` 的 checkName 精确过滤）；**不移植** log tails（spec §2.2）。

- [ ] **Step 4: 跑测试 + typecheck**

Run: `pnpm vitest run src/renderer/src/lib/github && pnpm typecheck`

- [ ] **Step 5: 提交**

```bash
git add src/shared/github/check-details-deadline.ts \
  src/renderer/src/lib/github/pr-checks.ts src/renderer/src/lib/github/pr-checks.test.ts \
  src/renderer/src/lib/github/pr-check-details.ts src/renderer/src/lib/github/pr-check-details.test.ts
git commit -m "feat(renderer): PR checks 列表三阶降级与详情（无日志尾）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: TS 速率快照 + hosted review forBranch + preflight gh 探针

**Files:**
- Create: `src/renderer/src/lib/github/rate-limit.ts`
- Create: `src/renderer/src/lib/github/hosted-review.ts`
- Create: `src/renderer/src/lib/github/preflight-gh.ts`
- Test: 三个对应 `.test.ts`

**Interfaces:**
- Consumes: Task 5 `createPRForBranchLookup`；Task 4 `resolveCandidates`；端口已有 `shared/hosted-review-github.ts` 的 `hostedReviewInfoFromGitHubPRInfo(pr)`；Task 3 `parseAuthStatus`。
- Produces:
  - `createRateLimitClient(deps): { getRateLimit(options?: {force?: boolean}): Promise<GetRateLimitResult> }`（30s 缓存 + 负缓存 + single-flight；argv `['api','rate_limit']` 且 host 固定 `github.com`）
  - `createHostedReviewClient(deps): { forBranch(args: HostedReviewForBranchArgs & { repoPath: string }): Promise<HostedReviewInfo | null> }`
  - `createGhReadinessProbe(deps): () => Promise<{installed: boolean; authenticated: boolean}>`（60s 缓存）

- [ ] **Step 1: 写失败测试**

- rate-limit：快照映射（`resources.core/search/graphql` → `{remaining,limit,resetAt}`，缺失字段 0/now）；30s 内二次调用不 spawn；`force:true` 绕过；失败返回 `{ok:false,error}` 并负缓存 30s。
- hosted-review：GitHub 命中 → `hostedReviewInfoFromGitHubPRInfo` 字段（provider/number/title/state/url/status/updatedAt/mergeable/headSha/githubRepository）；无 PR → null；found 缓存 60s、none 15min、active none 60s（用 `now` 注入推进）；merged 且 head 变化 → 缓存失效重查；非 GitHub repo → null。
- preflight-gh：`gh auth status` 有 active 账号 → `{installed:true, authenticated:true}`；gh 缺失（executor 抛 `gh: command not found`）→ `{installed:false, authenticated:false}`；无账号 → installed true / authenticated false；60s 缓存。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github/rate-limit.test.ts src/renderer/src/lib/github/hosted-review.test.ts src/renderer/src/lib/github/preflight-gh.test.ts`

- [ ] **Step 3: 实现**

- `rate-limit.ts`：逐字移植 `orca:src/main/github/rate-limit.ts:277-324` 的缓存/force/单飞与 `parseBucket`（`:40-55`）；argv 固定 `['api','rate_limit']`（host 固定：本端口 `gh_exec` 暂无 host 参数——**不传 host**，在报告中披露与参照的差异）。
- `hosted-review.ts`：`forBranch(args)`：分支去 `refs/heads/`；空 branch 且无 linked → null；`resolveCandidates`（非 GitHub → null）；`lookup.getPRForBranchOutcome({worktreePath: args.repoPath, branch, linkedPRNumber: args.linkedGitHubPR, fallbackPRNumber: args.linkedGitHubPR == null ? args.fallbackGitHubPR : null, acceptMergedFallbackPR: fallback !== null, currentHeadOid: args.currentHeadOid})`；`upstream-error` → 抛 `Error(`GitHub PR lookup failed (${errorType}): ${message}`)`（参照 `forge-provider.ts:134-141`）；found → `hostedReviewInfoFromGitHubPRInfo(pr)`；no-pr → null。缓存 TTL：found 60s / none（active?60s:15min），key 含 repoPath+branch+linked ids；merged 条目 head-sensitive（`headOid !== currentHeadOid` → miss）。
- `preflight-gh.ts`：60s 缓存；`client.run(['auth','status'])`（非零也解析）→ `parseAuthStatus(stdout+stderr)`；`installed` = 调用未抛 spawn 类错误；`authenticated` = 存在 `active` 账号；executor 抛 `gh: command not found` → installed false。

- [ ] **Step 4: 跑测试 + typecheck**

Run: `pnpm vitest run src/renderer/src/lib/github && pnpm typecheck`

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/lib/github/rate-limit.ts src/renderer/src/lib/github/rate-limit.test.ts \
  src/renderer/src/lib/github/hosted-review.ts src/renderer/src/lib/github/hosted-review.test.ts \
  src/renderer/src/lib/github/preflight-gh.ts src/renderer/src/lib/github/preflight-gh.test.ts
git commit -m "feat(renderer): 速率快照、hosted review forBranch 与 gh 就绪探针

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: 桥接接线（real gh/hosted-review/preflight + create-api/parity）

**Files:**
- Create: `src/bridge/real/gh.ts`
- Create: `src/bridge/real/hosted-review.ts`
- Modify: `src/bridge/real/preflight.ts`
- Modify: `src/bridge/create-api.ts`
- Modify: `src/bridge/real/parity.test.ts`
- Modify: `src/bridge/create-api.test.ts`
- Test: `src/bridge/real/gh.test.ts`、`src/bridge/real/hosted-review.test.ts`、`src/bridge/real/preflight.test.ts`（扩展）

**Interfaces:**
- Consumes: Tasks 3–7 全部工厂与 `defaultGhExecutor`；`invokeCommand`；`withMethodFallback`；`noopUnsubscribe` 模式（`real/ui.ts:5-6`）。
- Produces: `createGhRealApi()`、`createHostedReviewRealApi()`；`RealDomains` 加入 `'gh' | 'hostedReview'`。

- [ ] **Step 1: 写失败测试**

`real/gh.test.ts`（模式照 `real/preflight.test.ts`：mock `@tauri-apps/api/core`）：

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

import { createGhRealApi } from './gh'

beforeEach(() => invokeMock.mockReset())

describe('gh real api', () => {
  it('routes diagnoseAuth through gh auth status', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'gh_env_probe') return { token: null }
      if (command === 'gh_exec') {
        return {
          stdout: 'github.com\n  ✓ Logged in to github.com account alice (keyring)\n  - Active account: true\n  - Token scopes: \'project\', \'read:org\', \'repo\'\n',
          stderr: '',
          code: 0
        }
      }
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    await expect(api.diagnoseAuth()).resolves.toMatchObject({
      ghAvailable: true,
      activeAccount: { user: 'alice' }
    })
  })

  it('routes rateLimit through gh api rate_limit', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'gh_exec') {
        return {
          stdout: JSON.stringify({
            resources: {
              core: { limit: 5000, remaining: 4999, reset: 1700000000 },
              search: { limit: 30, remaining: 30, reset: 1700000000 },
              graphql: { limit: 5000, remaining: 4999, reset: 1700000000 }
            }
          }),
          stderr: '',
          code: 0
        }
      }
      throw new Error(`unexpected ${command}`)
    })
    const api = createGhRealApi()
    const result = await api.rateLimit()
    expect(result.ok).toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('gh_exec', expect.anything())
  })

  it('keeps unported gh methods rejecting as unimplemented', async () => {
    const api = createGhRealApi()
    await expect(api.mergePR({ repoPath: '/repo', prNumber: 1 })).rejects.toMatchObject({
      name: 'UnimplementedBridgeError'
    })
  })
})
```

`real/hosted-review.test.ts`：`forBranch` 走真实现（mock gh_exec 返回 PR JSON）返回 `HostedReviewInfo`；`create` 仍 reject unimplemented。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/bridge/real/gh.test.ts src/bridge/real/hosted-review.test.ts`

- [ ] **Step 3: 实现**

`real/gh.ts`：

```ts
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'
import { createGhExecClient, defaultGhExecutor } from '@/lib/github/gh-exec-client'
import { computeAuthDiagnostic, parseAuthStatus } from '@/lib/github/auth-diagnose'
import { createRepoIdentityResolver } from '@/lib/github/repo-identity'
import { createPRForBranchLookup } from '@/lib/github/pr-for-branch'
import { createPRChecksClient } from '@/lib/github/pr-checks'
import { createRateLimitClient } from '@/lib/github/rate-limit'

const noopUnsubscribe = (): void => {}

export function createGhRealApi(): PreloadApi['gh'] {
  const client = createGhExecClient(defaultGhExecutor())
  const readRemoteUrls = (worktreePath: string) =>
    invokeCommand<{ name: string; url: string }[]>('git_remote_urls', {
      args: { worktreePath }
    })
  const identity = createRepoIdentityResolver({ client, readRemoteUrls })
  const lookup = createPRForBranchLookup({ client, identity })
  const checks = createPRChecksClient({ client, identity })
  const rateLimit = createRateLimitClient({ client })

  return withMethodFallback<PreloadApi['gh']>('gh', {
    diagnoseAuth: async (args) => {
      const host = args?.host
      let accounts = []
      let ghAvailable = true
      try {
        const result = await client.run(['auth', 'status'])
        accounts = parseAuthStatus(`${result.stdout}\n${result.stderr}`)
      } catch {
        ghAvailable = false
      }
      const env = await invokeCommand<{ token: 'GH_TOKEN' | 'GITHUB_TOKEN' | null }>('gh_env_probe')
      return computeAuthDiagnostic({ accounts, ghAvailable, envTokenInProcess: env.token, requiredHost: host ?? null })
    },
    repoSlug: (args) => identity.getRepoSlug(args.repoPath),
    repoUpstream: (args) => identity.getRepoUpstream(args.repoPath),
    prForBranch: (args) => lookup.getPRForBranch(args),
    refreshPRNow: (args) => lookup.getPRForBranchOutcome(args.candidate),
    prChecks: (args) => checks.getPRChecks(args),
    prCheckDetails: (args) => checks.getPRCheckDetails(args),
    rateLimit: (args) => rateLimit.getRateLimit(args ?? undefined),
    onPRRefreshEvent: () => noopUnsubscribe
  })
}
```

（`refreshPRNow` 入参 `candidate` 的字段与 lookup 入参映射：`worktreePath: candidate.repoPath`、`branch: candidate.branch`、`linkedPRNumber: candidate.linkedPRNumber`、`fallbackPRNumber: candidate.fallbackPRNumber`、`currentHeadOid: candidate.currentHeadOid`——按 `GitHubPRRefreshCandidate` 实际字段接线。）

`real/hosted-review.ts`：`forBranch: (args) => hostedReviewClient.forBranch(args)`，其余 fallback；`hostedReview` 域同样 `withMethodFallback`。

`real/preflight.ts`：`check` 改为 `gh: await ghReadinessProbe()`（模块级 60s 缓存探针，用 `defaultGhExecutor`），git 不变。

`create-api.ts`：`RealDomains` 加 `'gh' | 'hostedReview'`；`createRealDomains()` 加 `gh: createGhRealApi(), hostedReview: createHostedReviewRealApi()`。

`parity.test.ts`：`surfaceCases` 加 gh/hostedReview 条目（gh explicit 8 个只读方法 + onPRRefreshEvent；hostedReview explicit `['forBranch']`，missing 其余）；`realApiFor` 加两个 switch 臂与 import。

`create-api.test.ts`：原 `hostedReview.forBranch resolves null` 断言改为断言真域路由（mock invoke 返回 `[]`/null 后解析 null）；新增 gh 域路由断言（`diagnoseAuth` → 触发 `gh_exec`/`gh_env_probe`）。

- [ ] **Step 4: 跑测试 + typecheck + 全量**

Run: `pnpm vitest run src/bridge && pnpm typecheck`
Run（提交前）: `pnpm test`（全量；性能类抖动单独复跑）
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src/bridge/real/gh.ts src/bridge/real/gh.test.ts \
  src/bridge/real/hosted-review.ts src/bridge/real/hosted-review.test.ts \
  src/bridge/real/preflight.ts src/bridge/real/preflight.test.ts \
  src/bridge/create-api.ts src/bridge/create-api.test.ts src/bridge/real/parity.test.ts
git commit -m "feat(bridge): gh/hostedReview 真实域接线与 preflight gh 探针

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: 门禁复跑 + 收尾记录

**Files:**
- Create: `docs/phase2d1-github-readonly-record.md`
- Modify: `docs/superpowers/plans/2026-10-08-phase2d1-github-readonly.md`（勾选复选框）

- [ ] **Step 1: 全量门禁**

Run: `cargo test --workspace`（workdir src-tauri）
Run: `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`
Run: `pnpm test`（性能类失败隔离复跑记 flake；其它失败 STOP + BLOCKED）

- [ ] **Step 2: 写收尾记录**

结构照 `docs/phase2c-diff-annotations-review-record.md`：§1 范围与验收、§2 提交清单、§3 门禁证据（命令 + 精确数字）、§4 手工验收（下列 5 项，标注待用户复核）、§5 偏差与边界备案（spec §7 十条 + 实现中发现的新偏差，特别记录：`gh_exec` 无 host 参数 → rate_limit 未固定 host；合并隐藏/stack/merge-queue 的裁剪范围）、§6 已知边界与后续（2D.2 创建 PR、后台协调器、日志尾、SSH 别名、Windows/WSL、速率熔断）。

手工验收清单（写入记录，供用户执行）：
1. Settings → Git & Source Control：速率预算面板显示 core/search/graphql 数值；
2. Settings → repository → GitHub avatar：解析出 slug 并可刷新头像；
3. 有开放 PR 的 worktree：卡片显示 PR pill；Checks 面板显示 checks；详情可展开（无日志尾）；
4. Landing/Onboarding：gh 就绪状态正确（已装/已登录 → ready）；
5. 无 PR 分支：无 pill、无报错 toast。

- [ ] **Step 3: 提交**

```bash
git add docs/phase2d1-github-readonly-record.md docs/superpowers/plans/2026-10-08-phase2d1-github-readonly.md
git commit -m "docs: Phase 2D.1 实施记录与门禁证据（GitHub 只读基础）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- §3.1 Rust 执行器 → Task 1（gh_exec/gh_env_probe/gate/path cache）、Task 2（git_remote_urls）。
- §3.2 桥接接线 → Task 8（real gh/hosted-review/preflight/create-api/parity）。
- §3.3 TS 编排 → Task 3（exec-client/auth/classification）、Task 4（identity）、Task 5（pr-for-branch）、Task 6（checks）、Task 7（rate-limit/hosted-review/preflight）。
- §4 数据流 → 各任务测试逐条覆盖；§5 错误处理 → Task 3 分类 + Task 5/6/7 分支测试。
- §6 测试与门禁 → 各任务 + Task 9；§7 风险披露 → Task 9 记录。

**Placeholder scan:** 无 TBD/TODO；Rust 全部代码在计划内；TS 逐字移植步骤附参照 file:line 与关键常量/argv/映射表；接口与测试断言具体。

**Type consistency:** `GhExecResult`/`GhExecutor`/`GhRunError`（Task 3）被 Task 4–8 一致引用；`GitHubRepoIdentity` 与 shared 的 `GitHubRepositoryIdentity` 字段一致（`{owner,repo,host?}`）；`PRInfo`/`PRRefreshOutcome`/`PRCheckDetail`/`PRCheckRunDetails`/`HostedReviewInfo`/`GetRateLimitResult`/`GhAuthDiagnostic` 均为端口已有类型；命令名 `gh_exec`/`gh_env_probe`/`git_remote_urls` 在 Task 1/2/8 一致。
