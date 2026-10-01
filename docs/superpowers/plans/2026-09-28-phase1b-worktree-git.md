# Phase 1 子项目 B（worktree + git）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Git 面板（status/diff/stage/discard/commit/upstream、branch compare、history）与 worktree 生命周期（创建/删除/遗忘/强删分支/元数据）从 mock 推进到真实 Rust 后端，`pnpm dev` 下跑通「创建 worktree → 编辑 → status/diff → stage → commit → 删除」并重启恢复。

**Architecture:** 扩展 `ade-git`（容忍非零退出的 git runner、porcelain v2 解析、diff blob 引擎、staging/commit、worktree add/remove、compare/history）；`ade-store` 新增 worktree 元数据存储；`ade-bridge` 新增 `commands/git.rs`、扩展 worktrees/repos 命令与 specta 登记；TS 侧新增 `src/bridge/real/git.ts` 并扩展 worktrees/repos 适配。

**Tech Stack:** Rust（tauri 2、serde、specta 2.0.0-rc.25、thiserror）、TypeScript 7、Vite、React 19、Vitest 4、pnpm 12。

**Spec:** `docs/superpowers/specs/2026-09-28-phase1b-worktree-git-design.md`（执行者需通读；本计划是其落地）

## Global Constraints

- 分支 `phase1b-worktree-git`（Task 0 创建），base `main`（含 spec 提交 `a69caad`）；不 push；完成后由 finishing-a-development-branch 交用户决定。
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（只读）；语义基准文件在各任务中逐条给出；对照时只读，不得修改。
- 门禁：每个 Rust 任务 `cargo test --manifest-path src-tauri/Cargo.toml --workspace` 相关包全绿；每个 TS 任务 `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` exit 0；最终 `pnpm test`（既有 3847 文件）全绿。
- 命令名契约 `<域>_<方法 snake_case>`；Rust 结构体 `#[serde(rename_all = "camelCase")]` 与 `src/shared/preload-api/api/*.ts` 及 `src/shared/*.ts` 类型逐字对齐；未接真方法保持响亮 `UnimplementedBridgeError`。
- `ade-git` 新增 `serde` + optional `specta` feature（模式照抄 `ade-core/Cargo.toml:6-14`）；`ade-bridge` 依赖改 `ade-git = { path = "../ade-git", features = ["specta"] }`。
- 事件：不新增事件名；create/remove 等复用 `worktrees:changed`/`repos:changed`（`src-tauri/crates/ade-bridge/src/events.rs`）。
- `pnpm test` 覆盖渲染层既有 3847 文件；新增桥接测试不得改变既有 mock 期望以外行为。
- Windows 不在本计划验证范围；`.worktreeinclude`/`orca.yaml` hooks/symlinkPaths/远端（fetch/push/pull/PR/SSH）不在本计划（spec §2.2/§2.3）。
- 一次性脚本放 `.superpowers/sdd/2026-09-28-phase1b-worktree-git/tools/`（gitignored）；生成物（specta bindings）必须 checked in。
- 每个任务结束时提交，提交信息用 conventional commit（`feat(git):`/`feat(worktree):`/`feat(store):`/`feat(bridge):`/`feat(renderer):`）。

## 文件结构（结束时）

```
src-tauri/crates/
├── ade-core/src/errors.rs                     # + GitCommandFailed / GitCommandCancelled
├── ade-git/
│   ├── Cargo.toml                             # + serde / specta(feature)
│   ├── src/lib.rs                             # mod 声明与 re-export
│   ├── src/runner.rs                          # 进程执行（超时/取消/保留 stderr）
│   ├── src/status.rs                          # 类型 + porcelain v2 -z 增量解析器
│   ├── src/status_read.rs                     # status 命令执行/限流/行统计/冲突操作
│   ├── src/diff.rs                            # blob diff 引擎（staged/unstaged/compare）
│   ├── src/staging.rs                         # stage/unstage/discard/commit/upstream
│   ├── src/branch.rs                          # base ref 解析/分支命名/前缀/清洗
│   ├── src/worktree_create.rs                 # 目录计算 + worktree add + 配置写入
│   ├── src/worktree_remove.rs                 # 预检/remove/prune/分支删除/CAS 强删
│   ├── src/compare.rs                         # branchCompare/commitCompare/branchDiff/commitDiff
│   ├── src/history.rs                         # git log 历史 + refs 解析
│   └── tests/{status.rs,diff.rs,staging.rs,worktree.rs,compare_history.rs}
├── ade-store/src/worktree_meta_store.rs       # worktrees.json
└── ade-bridge/src/
    ├── commands/git.rs                        # 17 命令
    ├── commands/worktrees.rs                  # +create/remove/forget/forceDelete/updateMeta/persistSortOrder
    ├── commands/repos.rs                      # +create/getBaseRefDefault/searchBaseRefs/searchBaseRefDetails
    ├── commands/mod.rs                        # + pub mod git;
    ├── state.rs                               # + WorktreeMetaStore、CancelRegistry
    └── specta_export.rs                       # 命令登记 + bindings 新鲜度
src/bridge/
├── real/git.ts                                # 新增 Git 域真实适配
├── real/git.test.ts
├── real/worktrees.ts / worktrees.test.ts      # 扩展
├── real/repos.ts / repos.test.ts              # 扩展
├── real/parity.test.ts                        # 迁移清单
└── create-api.ts                              # git 加入 real 域
docs/phase1b-worktree-git-record.md            # Task 15 产出
```

---

## Task 0: 分支、基线、SDD 工作区

**Files:**
- Create: `.superpowers/sdd/2026-09-28-phase1b-worktree-git/`（gitignored）

- [ ] **Step 1: 建分支**

```bash
git checkout main && git checkout -b phase1b-worktree-git
```

- [ ] **Step 2: 建 SDD 工作区**

```bash
mkdir -p .superpowers/sdd/2026-09-28-phase1b-worktree-git/tools
```

- [ ] **Step 3: 记录基线**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace 2>&1 | tail -5
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
git status -sb
```

预期：cargo 351 通过 / 0 failed；门禁 exit 0；工作树干净。将结果写入 `.superpowers/sdd/2026-09-28-phase1b-worktree-git/progress.md`（含 spec 与 plan 路径、基线提交）。

---

## Task 1: ade-core 错误扩展 + ade-git runner

**Files:**
- Modify: `src-tauri/crates/ade-core/src/errors.rs`
- Create: `src-tauri/crates/ade-git/src/runner.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`（`mod runner;` + `pub use runner::{...}`）
- Modify: `src-tauri/crates/ade-git/Cargo.toml`

**Interfaces:**
- Produces:
  - `CoreError::GitCommandFailed { command: String, stderr: String, exit_code: Option<i32> }`，Display：`git {command} failed (exit code {exit_code:?}): {stderr}`
  - `CoreError::GitCommandCancelled { command: String }`，Display：`git {command} was cancelled`
  - `ade_git::runner::GitOutput { status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8> }`
  - `ade_git::runner::CancelToken`（`new/cancel/is_cancelled`，`Clone`）
  - `ade_git::runner::run_git_in(cwd: &str, args: &[&str], timeout: Duration, cancel: Option<&CancelToken>) -> Result<GitOutput, CoreError>`（非零退出返回 `Ok`）
- Consumes：既有 `output_with_timeout` 语义（`ade-git/src/lib.rs:50-108`）

- [ ] **Step 1: 写失败测试**

`ade-core/src/errors.rs` 测试：

```rust
#[test]
fn git_command_failed_display_includes_command_stderr_and_code() {
    let error = CoreError::GitCommandFailed {
        command: "commit -m x".to_string(),
        stderr: "hook declined".to_string(),
        exit_code: Some(1),
    };
    assert_eq!(
        error.to_string(),
        "git commit -m x failed (exit code Some(1)): hook declined"
    );
}

#[test]
fn git_command_cancelled_display() {
    let error = CoreError::GitCommandCancelled { command: "status --porcelain=v2".to_string() };
    assert_eq!(error.to_string(), "git status --porcelain=v2 was cancelled");
}
```

`ade-git/src/runner.rs` 测试（新模块内 `#[cfg(test)]`，unix 限定）：

```rust
#[cfg(unix)]
#[test]
fn run_git_in_keeps_nonzero_exit_as_ok() {
    let dir = std::env::temp_dir();
    let output = run_git_in(
        dir.to_str().unwrap(),
        &["rev-parse", "--is-inside-work-tree"],
        Duration::from_secs(5),
        None,
    )
    .expect("process ran");
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn cancel_token_kills_running_process() {
    let token = CancelToken::new();
    let canceller = token.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        canceller.cancel();
    });
    let mut command = Command::new("sleep");
    command.arg("30");
    let started = Instant::now();
    let error = run_process(&mut command, Duration::from_secs(10), Some(&token)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
#[test]
fn timeout_still_wins_over_no_cancel() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let error = run_process(&mut command, Duration::from_millis(200), None).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-core -p ade-git
```

预期：编译失败（`CoreError::GitCommandFailed`、`run_git_in`、`run_process` 不存在）。

- [ ] **Step 3: 实现**

`Cargo.toml`：

```toml
[features]
specta = ["dep:specta"]

[dependencies]
ade-core = { path = "../ade-core" }
serde = { version = "1", features = ["derive"] }
specta = { version = "=2.0.0-rc.25", features = ["derive"], optional = true }
```

`runner.rs` 核心：

```rust
use crate::CancelToken; // 或原地定义后 re-export
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self { Self::default() }
    pub fn cancel(&self) { self.flag.store(true, Ordering::SeqCst) }
    pub fn is_cancelled(&self) -> bool { self.flag.load(Ordering::SeqCst) }
}

pub fn run_git_in(
    cwd: &str,
    args: &[&str],
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> Result<GitOutput, ade_core::errors::CoreError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args(args);
    match run_process(&mut command, timeout, cancel) {
        Ok(output) => Ok(GitOutput { status: output.status, stdout: output.stdout, stderr: output.stderr }),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
            Err(ade_core::errors::CoreError::GitCommandCancelled {
                command: format!("git {}", args.join(" ")),
            })
        }
        Err(error) => Err(error.into()),
    }
}

/// Spawn, capture both streams on reader threads, poll with a 10ms tick for
/// timeout/cancel, kill on either. Mirrors the existing lib.rs machinery.
pub(crate) fn run_process(
    command: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> std::io::Result<Output> { /* 见 lib.rs:50-108 的读线程 + try_wait 循环，循环内先查 cancel 再查 deadline */ }
```

注意：`run_process` 内取消命中时 `reap` 后返回 `io::Error::new(ErrorKind::Interrupted, "git command cancelled")`；超时保持 `TimedOut`（既有测试不得回归）。`lib.rs` 既有 `output_with_timeout` 改为委托 `runner::run_process(command, timeout, None)`，删除重复的 `wait_with_timeout/reap/read_all` 私有实现。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-core -p ade-git
```

预期：全绿（既有 `output_with_timeout_*` 2 测试 + 新增 4 测试）。

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): 容忍非零退出的 git runner 与取消/超时机制"
```

---

## Task 2: ade-git status 类型与 porcelain v2 增量解析器

**Files:**
- Create: `src-tauri/crates/ade-git/src/status.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`（`pub mod status;`）

**Interfaces:**
- Produces（全部 `#[derive(Debug, Clone, PartialEq, Serialize)]` + `#[cfg_attr(feature = "specta", derive(specta::Type))]`，字段 `camelCase`）：
  - `GitFileStatus { Modified, Added, Deleted, Renamed, Untracked, Copied }`（`#[serde(rename_all = "lowercase")]`）
  - `GitStagingArea { Staged, Unstaged, Untracked }`（lowercase）
  - `GitConflictKind { BothModified, BothAdded, BothDeleted, AddedByUs, AddedByThem, DeletedByUs, DeletedByThem }`（`rename_all = "snake_case"`）
  - `GitConflictOperation { Merge, Rebase, CherryPick, Unknown }`（`rename_all = "kebab-case"`）
  - `GitConflictResolutionStatus { Unresolved, ResolvedLocally }`（snake_case）
  - `GitConflictStatusSource { Git, Session }`（lowercase）
  - `GitSubmoduleStatus { commit_changed, tracked_changes, untracked_changes }`（camelCase）
  - `GitStatusEntry { path, status, area, old_path?, conflict_kind?, conflict_status?, conflict_status_source?, submodule?, submodule_root?, added?, removed? }`（`skip_serializing_if = "Option::is_none"`）
  - `GitUpstreamStatus { has_upstream, upstream_name?, ahead, behind, has_configured_push_target?, behind_commits_are_patch_equivalent? }`
  - `GitBranchLineTotal { added, removed, merge_base, test?, generated? }`（`LineStat { added, removed }`）
  - `GitStatusResult { entries, conflict_operation, head?, branch?, upstream_status?, ignored_paths?, did_hit_limit?, status_length?, branch_line_total? }`
  - `StatusParser { update(&mut self, chunk: &[u8], limit: usize) -> bool; finish(&mut self); into_parsed(self) -> ParsedStatus }`
  - `ParsedStatus { entries: Vec<GitStatusEntry>, ignored_paths: Vec<String>, unmerged_lines: Vec<String>, head: Option<String>, branch: Option<String>, upstream_name: Option<String>, ahead_behind: Option<(u64, u64)>, changed_count: u64 }`
- 语义依据：`orca:src/shared/git-status-porcelain-parser.ts`（逐字对照）、`orca:src/shared/git-status-types.ts`、`orca:src/shared/git-status-conflict-entries.ts`（冲突行解析在 Task 3）

- [ ] **Step 1: 写失败测试（fixtures 驱动）**

在 `status.rs` 内写：

```rust
fn feed(records: &[&str], limit: usize) -> (StatusParser, bool) {
    let mut parser = StatusParser::new();
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(record.as_bytes());
        bytes.push(0);
    }
    let stopped = parser.update(&bytes, limit);
    parser.finish();
    (parser, stopped)
}

#[test]
fn parses_branch_headers_and_entries() {
    let (parser, stopped) = feed(
        &[
            "# branch.oid efbccd00b747859625ba07b4b6d4322cbe07b37",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 M. N... 100644 100644 100644 61780798228d17af2d34fce4cfbdf35556832472 61780798228d17af2d34fce4cfbdf35556832472 src/app.ts",
            "1 .M N... 100644 100644 100644 1111111111111111111111111111111111111111 1111111111111111111111111111111111111111 src/other.ts",
            "? notes.txt",
        ],
        1000,
    );
    assert!(!stopped);
    let parsed = parser.into_parsed();
    assert_eq!(parsed.head.as_deref(), Some("efbccd00b747859625ba07b4b6d4322cbe07b37"));
    assert_eq!(parsed.branch.as_deref(), Some("refs/heads/main"));
    assert_eq!(parsed.upstream_name.as_deref(), Some("origin/main"));
    assert_eq!(parsed.ahead_behind, Some((2, 1)));
    assert_eq!(parsed.entries.len(), 3);
    assert_eq!(parsed.entries[0].area, GitStagingArea::Staged);
    assert_eq!(parsed.entries[0].status, GitFileStatus::Modified);
    assert_eq!(parsed.entries[1].area, GitStagingArea::Unstaged);
    assert_eq!(parsed.entries[2].status, GitFileStatus::Untracked);
}

#[test]
fn detached_head_yields_no_branch() {
    let (parser, _) = feed(&["# branch.oid abc", "# branch.head (detached)"], 1000);
    assert_eq!(parser.into_parsed().branch, None);
}

#[test]
fn type2_rename_z_takes_orig_path_from_next_chunk() {
    // `-z` 形态：type-2 记录的旧路径是紧随其后的独立 NUL 分片（见计划 Task 2 Step 3）
    let (parser, _) = feed(
        &[
            "2 R. N... 100644 100644 100644 61780798228d17af2d34fce4cfbdf35556832472 61780798228d17af2d34fce4cfbdf35556832472 R100 new name.txt",
            "has space.txt",
        ],
        1000,
    );
    let parsed = parser.into_parsed();
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].path, "new name.txt");
    assert_eq!(parsed.entries[0].old_path.as_deref(), Some("has space.txt"));
}

#[test]
fn stops_when_changed_count_exceeds_limit() {
    let (parser, stopped) = feed(
        &["1 M. N... 100644 100644 100644 a a one", "1 M. N... 100644 100644 100644 a a two", "1 M. N... 100644 100644 100644 a a three"],
        2,
    );
    assert!(stopped);
    let parsed = parser.into_parsed();
    assert_eq!(parsed.changed_count, 3); // statusLength 含越限计数
    assert_eq!(parsed.entries.len(), 3);
}

#[test]
fn collects_ignored_and_unmerged_without_parsing() {
    let (parser, _) = feed(&["! dist/", "u UU N... 100644 100644 100644 100644 a b c conflict.txt"], 1000);
    let parsed = parser.into_parsed();
    assert_eq!(parsed.ignored_paths, vec!["dist/".to_string()]);
    assert_eq!(parsed.unmerged_lines.len(), 1);
    assert_eq!(parsed.changed_count, 1);
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

预期：编译失败（parser/类型不存在）。

- [ ] **Step 3: 实现解析器**

要点（逐条对照 oracle parser）：
- 记录以 `0u8` 分隔；`carry` 保存不完整尾部；`update` 逐条 `parse_record`，`limit != 0 && changed_count > limit` 时立即返回 `true`（调用方 kill git；**不清空 carry，直接保留已解析结果**）。
- `# branch.oid ` → `head`；`# branch.head ` → 非空且非 `(detached)` 时 `refs/heads/<name>`；`# branch.upstream ` → `upstream_name`；`# branch.ab +N -M` 正则解析 → `ahead_behind`。
- `1 `：按空格 split，`parts[1]` 为 XY，`status_char(index)`/`status_char(worktree)` 非 `.` 各推一条（staged/unstaged），path 为 `parts[8..].join(" ")`；submodule 字段 `parts[2]` 以 `S` 开头 → `{commit_changed: f[1]=='C' || (f=="S..." && ch=='M'), tracked_changes: f[2]=='M', untracked_changes: f[3]=='U'}`。
- `2 `：拆分后 path=`parts[9..].join(" ")`；**旧路径来自下一条 NUL 分片**（设置 `pending_rename = true`，下一条记录直接作为 `old_path`，不解析其前缀）；index/worktree 非 `.` 各推一条（均带 `old_path`）。
- `? ` → untracked 条目；`! ` → `ignored_paths`；`u ` → `changed_count += 1` 且原样进 `unmerged_lines`（Task 3 解析）。
- 未知前缀忽略（对齐 oracle 的静默跳过）。
- `changed_count` 统计所有 changed 记录（含越过 limit 的）与 unmerged 行。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

预期：新增 5 测试 + 既有 13 测试全绿。

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): porcelain v2 状态类型与增量解析器"
```

---

## Task 3: ade-git status 执行、冲突解析、行统计

**Files:**
- Create: `src-tauri/crates/ade-git/src/status_read.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`（`pub mod status_read;`）
- Test: `src-tauri/crates/ade-git/tests/status.rs`（tempdir + 真实 git）

**Interfaces:**
- Produces（`status_read.rs`）：
  - `pub const DEFAULT_GIT_STATUS_LIMIT: usize = 1000;`
  - `pub struct StatusOptions { pub limit: Option<i64>, pub include_ignored: bool, pub include_line_stats: bool, pub branch_line_total_merge_base: Option<String> }`（Default：limit None→1000）
  - `pub fn resolve_status_limit(limit: Option<i64>) -> usize`（None/负数/非法 → 1000；`0` → 0 关闭）
  - `pub fn status(worktree_path: &str, options: &StatusOptions, cancel: Option<&CancelToken>) -> Result<GitStatusResult, CoreError>`
  - `pub fn conflict_operation(worktree_path: &str) -> Result<GitConflictOperation, CoreError>`
  - `pub fn parse_unmerged_entry(worktree_path: &str, line: &str) -> Option<GitStatusEntry>`
  - `pub fn attach_line_stats(worktree_path: &str, entries: &mut [GitStatusEntry]) -> Result<(), CoreError>`
- Consumes：Task 2 的 `StatusParser`/类型、Task 1 的 `run_git_in`
- 语义依据：`orca:src/main/git/source-control/status-read.ts:121-289`、`status-line-stats.ts`、`git-conflict-operation.ts`、`orca:src/shared/git-status-conflict-entries.ts`（逐字）

- [ ] **Step 1: 写失败集成测试**

`tests/status.rs`（复用 `tests/cli_integration.rs` 的 TempDir/git helper 模式，env 隔离 `GIT_CONFIG_NOSYSTEM=1`、`GIT_CONFIG_GLOBAL=/dev/null`）：

```rust
#[test]
fn status_reports_branch_staged_unstaged_and_untracked() {
    let dir = TempDir::new("status-basic");
    let repo = init_repo_with_commit(&dir); // main 分支，含 README.md 一次提交
    std::fs::write(repo.join("README.md"), "changed\n").unwrap();
    std::fs::write(repo.join("new.txt"), "new\n").unwrap();
    let result = status(repo.to_str().unwrap(), &StatusOptions::default(), None).unwrap();
    assert_eq!(result.branch.as_deref(), Some("refs/heads/main"));
    assert_eq!(result.conflict_operation, GitConflictOperation::Unknown);
    let paths: Vec<&str> = result.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"README.md"));
    assert!(paths.contains(&"new.txt"));
    let readme = result.entries.iter().find(|e| e.path == "README.md").unwrap();
    assert_eq!(readme.area, GitStagingArea::Unstaged);
    assert_eq!(readme.status, GitFileStatus::Modified);
}

#[test]
fn status_limit_truncates_and_flags() {
    let dir = TempDir::new("status-limit");
    let repo = init_repo_with_commit(&dir);
    for i in 0..5 { std::fs::write(repo.join(format!("f{i}.txt")), "x").unwrap(); }
    let options = StatusOptions { limit: Some(2), ..StatusOptions::default() };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();
    assert_eq!(result.entries.len(), 2);
    assert_eq!(result.did_hit_limit, Some(true));
    assert_eq!(result.status_length, Some(5));
}

#[test]
fn status_on_non_repo_is_unsigned_empty_and_conflict_unknown() {
    let dir = TempDir::new("status-nonrepo");
    let result = status(dir.path_str(), &StatusOptions::default(), None).unwrap();
    assert!(result.entries.is_empty());
    assert!(result.branch.is_none());
    assert_eq!(result.conflict_operation, GitConflictOperation::Unknown);
}

#[test]
fn unmerged_entry_maps_conflict_kind_and_compatibility_status() {
    let dir = TempDir::new("status-conflict");
    let repo = init_conflicted_repo(&dir); // main 与 feature 同时修改同一文件并 merge 产生 UU
    let result = status(repo.to_str().unwrap(), &StatusOptions::default(), None).unwrap();
    let entry = result.entries.iter().find(|e| e.path == "conflict.txt").unwrap();
    assert_eq!(entry.conflict_kind, Some(GitConflictKind::BothModified));
    assert_eq!(entry.conflict_status, Some(GitConflictResolutionStatus::Unresolved));
    assert_eq!(entry.status, GitFileStatus::Modified);
    assert_eq!(result.conflict_operation, GitConflictOperation::Merge);
}

#[test]
fn line_stats_attach_numbers_for_modified_files() {
    let dir = TempDir::new("status-stats");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "line1\nline2\nline3\n").unwrap();
    let options = StatusOptions { include_line_stats: true, ..StatusOptions::default() };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();
    let entry = result.entries.iter().find(|e| e.path == "README.md").unwrap();
    assert_eq!(entry.added, Some(2));
    assert_eq!(entry.removed, Some(0));
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git --test status
```

预期：编译失败。

- [ ] **Step 3: 实现**

- `status`：`run_git_in(worktree_path, &["-c","core.quotePath=false","status","--porcelain=v2","--branch","--untracked-files=all","-z", ...include_ignored? "--ignored=matching"], 120s, cancel)`。**非取消的 git 失败 → 返回空 entries + `conflict_operation: Unknown` 的成功结果**（对齐全 oracle `status-read.ts:179-185`）；取消（`GitCommandCancelled`）→ 上抛。解析：`StatusParser::update` 逐段喂入（每读到 `limit` 越界即 kill 子进程——实现为 `run_process` 带 `on_chunk` 回调变体或读出后解析后截断；**若实现行级流式 kill 成本高，可采用「全量读入 + 解析到 limit 截断」并记录偏差**，但必须在 `did_hit_limit`/`status_length` 上保持语义）。
- unmerged 解析（照抄 `git-status-conflict-entries.ts:49-102`）：`UU→both_modified`、`AA→both_added`、`DD→both_deleted`、`AU→added_by_us`、`UA→added_by_them`、`DU→deleted_by_us`、`UD→deleted_by_them`；任一 stage mode `160000` → 丢弃；`status` 兼容值为 both_modified/both_added→modified、both_deleted→deleted、其余按 `mW`（`000000`→deleted，否则 modified；非 6 位八进制回退 fs exists 探测）；`area: unstaged`、`conflict_status: unresolved`。按 `statusRecords` 输出序插入并遵循 limit 截断（对齐 `status-read.ts:194-206`）。
- `upstream_status`：构建于 branch 头；`has_upstream = upstream_name.is_some()`，`ahead/behind` 缺省 0。`upstreamStatus` 仅在 `has_upstream` 或状态成功时填充（对齐 oracle：branch.ab 缺省则无 upstreamStatus？——以 oracle 最终对象为准，实现时读取 `status-read.ts:260-289` 的组装）。
- `conflict_operation`：照抄 `orca:src/main/git/source-control/git-conflict-operation.ts`（MERGE_HEAD / rebase-merge / rebase-apply / CHERRY_PICK_HEAD 探测，worktree `.git` 文件解析）。
- `attach_line_stats`：`git diff -z --numstat -M`（unstaged）与 `--cached`（staged）解析 `added\tremoved\tpath`（`-` 表示二进制 → 不设置字段）；untracked 文件：大小 ≤ 2 MiB 且前 8192 字节无 NUL → `added = 行数`，`removed = 0`；其余不设置。参照 `orca:src/main/git/source-control/status-line-stats.ts`。
- `branch_line_total`：仅当 `branch_line_total_merge_base` 提供时，`git diff --numstat -M <mergeBase>` 汇总 `{added, removed, merge_base}`（`test`/`generated` 桶不实现，记录偏差）。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): status 执行（限流/忽略/冲突/行统计）"
```

---

## Task 4: ade-git diff 引擎

**Files:**
- Create: `src-tauri/crates/ade-git/src/diff.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`
- Test: `src-tauri/crates/ade-git/tests/diff.rs`

**Interfaces:**
- Produces：
  - `GitDiffResult`（serde `tag = "kind"`）：`Text { original_content, modified_content, original_is_binary: false, modified_is_binary: false, large_diff_render_limit? }` / `Binary { original_content, modified_content, is_image?, mime_type?, modified_deleted?, original_is_binary, modified_is_binary }`；字段 camelCase；binary 两布尔至少一 true
  - `pub const MAX_GIT_SHOW_BYTES: u64 = 10 * 1024 * 1024;`
  - `pub fn diff(worktree_path: &str, file_path: &str, staged: bool, compare_against_head: bool, timeout: Duration) -> Result<GitDiffResult, CoreError>`
  - `pub fn diff_refs(worktree_path: &str, left_ref: &DiffSide, right_ref: &DiffSide, file_path: &str, old_path: Option<&str>) -> Result<GitDiffResult, CoreError>`，其中 `pub enum DiffSide { Rev { rev: String, path: String }, Worktree { path: String }, Empty }`（供 branchDiff/commitDiff 复用）
- Consumes：Task 1 runner
- 语义依据：`orca:src/main/git/source-control/file-diff.ts:32-211`、`git-blob-read.ts`、`previewable-binary-mime-types.ts`、`git-show-max-bytes.ts`、`orca:src/shared/binary-buffer.ts`、`orca:src/shared/large-diff-render-limit.ts`、`orca:src/main/git/source-control/diff-result.ts`

- [ ] **Step 1: 写失败集成测试**

```rust
#[test]
fn unstaged_diff_returns_index_and_worktree_contents() {
    // init repo + commit README("a\n") → 写 README("b\n")
    let result = diff(repo_str, "README.md", false, false, Duration::from_secs(30)).unwrap();
    match result {
        GitDiffResult::Text { original_content, modified_content, .. } => {
            assert_eq!(original_content, "a\n");
            assert_eq!(modified_content, "b\n");
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn staged_diff_returns_head_and_index_contents() {
    // commit("a\n") → 写 "b\n" → git add
    let result = diff(repo_str, "README.md", true, false, ...).unwrap();
    // original = HEAD "a\n"，modified = index "b\n"
}

#[test]
fn compare_against_head_uses_head_as_original_for_unstaged() {
    // commit("a\n") → 写 "b\n" → git add → 写 "c\n"（index=b, worktree=c）
    // compare_against_head=true → original "a\n", modified "c\n"
}

#[test]
fn binary_file_returns_binary_kind_with_empty_contents() {
    // 写入含 NUL 的 .bin（非预览白名单）
    // kind=binary, original_is_binary||modified_is_binary, contents 均为 ""
}

#[test]
fn png_within_cap_returns_base64_and_mime_type() {
    // 写入最小 PNG 字节并 git add/commit 后修改？——直接 untracked? diff 需要 index 有内容：
    // 提交 v1（PNG 字节）→ 改写 v2 → 断言 {"kind":"binary","mimeType":"image/png","isImage":true} 且 originalContent 为 base64
}

#[test]
fn oversized_blob_is_treated_as_binary_not_error() { /* >10MiB 文件：断言 kind=binary、无错误 */ }
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git --test diff
```

- [ ] **Step 3: 实现**

- `read_blob(worktree, rev, path)`：`run_git_in(worktree, ["show", &format!("{rev}:{path}")], 120s)`；`rev` 用 `HEAD` 或空（index 用 `git show :<path>` 语法 → 参数 `format!(":{path}")`）；blob 不存在（新文件）→ 返回 `Ok(None)` 视为空内容；单侧 > `MAX_GIT_SHOW_BYTES` → 按二进制标记（不报错）。
- 工作区侧：`std::fs::read` 相对 worktree 的文件；缺失时若为「证明删除」场景 → `modified_is_binary: true` + `modified_content: ""` + `modified_deleted: true`；否则按读取失败处理（对齐 file-diff.ts 的 `modifiedDeleted` 判定）。
- 二进制：`binary_buffer` 语义——前 8192 字节含 NUL（`orca:src/shared/binary-buffer.ts`）。
- MIME 白名单：`.png image/png`、`.jpg/.jpeg image/jpeg`、`.gif image/gif`、`.svg image/svg+xml`、`.webp image/webp`、`.bmp image/bmp`、`.ico image/x-icon`、`.pdf application/pdf`；命中 → base64 两侧内容 + `is_image: true`（PDF 也 true）；未命中 → 内容空串。
- 结果归属：渲染级截断由渲染端处理（`largeDiffRenderLimit` 字段不实现→缺省省略；记录偏差）。若实现时确认 oracle 在主进程施加 120k 行/6M 字符上限（`diff-result.ts:28-39` 的调用点），则改为在 `diff` 内施加并填充 `largeDiffRenderLimit`。
- `diff()` 组装（`file-diff.ts:162-211`）：staged 左 `HEAD:<path>` 右 `:<path>`；unstaged 左 index（失败回退 HEAD）右工作区；`compare_against_head` 时 unstaged 左 HEAD；`old_path` 用于 rename 左侧路径（`diff_refs` 供 Task 8 使用）。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): blob 语义 diff 引擎（二进制/上限/compareAgainstHead）"
```

---

## Task 5: ade-git staging / discard / commit / upstream

**Files:**
- Create: `src-tauri/crates/ade-git/src/staging.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`
- Test: `src-tauri/crates/ade-git/tests/staging.rs`

**Interfaces:**
- Produces：
  - `pub const BULK_PATHSPEC_CHUNK: usize = 100;`
  - `pub fn literal_pathspec(path: &str) -> String` → `format!(":(literal){path}")`
  - `pub fn stage(worktree_path: &str, file_path: &str) -> Result<(), CoreError>`（`git add -- :(literal)<path>`）
  - `pub fn unstage(worktree_path: &str, file_path: &str) -> Result<(), CoreError>`（`git restore --staged -- :(literal)<path>`）
  - `pub fn bulk_stage(worktree_path: &str, file_paths: &[String]) -> Result<(), CoreError>`（每 100 条调用一次 `add`；空列表直接 Ok）
  - `pub fn bulk_unstage(worktree_path: &str, file_paths: &[String]) -> Result<(), CoreError>`（同上 `restore --staged`）
  - `pub fn discard(worktree_path: &str, file_path: &str) -> Result<(), CoreError>`：先 `git ls-files --error-unmatch -- :(literal)<p>` 判 tracked；tracked → `git restore --worktree --source=HEAD -- :(literal)<p>`；untracked → `git clean -ffdx -- :(literal)<p>`（路径必须先做 worktree 包含性校验，越界 → `CoreError::PathNotAllowed`）
  - `pub fn bulk_discard(worktree_path: &str, file_paths: &[String]) -> Result<(), CoreError>`
  - `pub struct CommitOutcome { pub success: bool, pub error: Option<String> }`
  - `pub fn commit(worktree_path: &str, message: &str) -> Result<CommitOutcome, CoreError>`（message trim 空 → `CoreError::InvalidInput("Commit message is required")`）
  - `pub fn upstream_status(worktree_path: &str) -> Result<GitUpstreamStatus, CoreError>`
- 语义依据：`orca:src/main/git/source-control/staging.ts`、`discard-changes.ts:15-159`、`git-pathspec.ts`（`literalPathspec`/`BULK_CHUNK_SIZE=100`）、`commit-changes.ts:6-34`、`orca:src/shared/git-discard-path-safety.ts`

- [ ] **Step 1: 写失败集成测试**

```rust
#[test]
fn stage_and_unstage_round_trip() {
    // commit README("a\n") → 改 "b\n" → stage → status 中 README 在 staged 区 → unstage → 回 unstaged 区
}

#[test]
fn bulk_stage_empty_list_is_noop_and_handles_many_paths() {
    // 生成 250 个文件（验证 100/批分块）→ bulk_stage → status 全部 staged
}

#[test]
fn discard_restores_tracked_file_and_removes_untracked_file() {
    // 改 README → discard → 内容回 "a\n"；新建 untracked.txt → discard → 文件不存在
}

#[test]
fn discard_rejects_path_outside_worktree() {
    // assert!(matches!(discard(repo, "../escape.txt"), Err(CoreError::PathNotAllowed(_))))
}

#[test]
fn commit_returns_success_and_commit_visible_in_log() {
    // stage README → commit("feat: x") → success && git log -1 --format=%s == "feat: x"
}

#[test]
fn commit_reports_hook_failure_via_success_false() {
    // .git/hooks/pre-commit 写 `#!/bin/sh\nexit 1` + chmod +x → commit("x") → success=false && error 含 hook stderr
}

#[test]
fn commit_requires_message() {
    // assert!(matches!(commit(repo, "   "), Err(CoreError::InvalidInput(_))))
}

#[test]
fn upstream_status_without_upstream_reports_false() {
    // init_repo_with_commit（无 remote）→ {has_upstream:false, ahead:0, behind:0}
}

#[test]
fn upstream_status_counts_ahead_behind_when_upstream_configured() {
    // clone --bare 作为 origin + clone 工作仓库 + 两次本地 commit + push 一次：
    // 断言 ahead=1, behind=0；再 reset 到远程并本地落后 → behind 递增（或用第二个 clone 推一提交后 fetch）
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git --test staging
```

- [ ] **Step 3: 实现**

- 全部命令经 `run_git_in(..., COMMAND_TIMEOUT=10s, None)`；非零退出 → `CoreError::GitCommandFailed`（含 stderr 与 exit code）。
- `commit`：`git commit -m <message>`；失败时按 stderr → stdout → `"Commit failed"` 顺序取首段非空文本（trim），返回 `CommitOutcome{success:false,error:Some(text)}`；成功 `{success:true,error:None}`。**不实现** `--no-verify`/author 参数。
- `upstream_status`：`git rev-parse --abbrev-ref --symbolic-full-name @{upstream}` 取 upstreamName（失败 → has_upstream:false）；成功则 `git rev-list --left-right --count <upstream>...HEAD` 解析 `behind\tahead` 两数；`has_configured_push_target: false`、`behind_commits_are_patch_equivalent: false` 恒缺省（记录偏差）。
- 路径包含性：`Path::new(worktree).join(file_path)` canonicalize 后须以 worktree canonical 前缀 + 段边界开头；不存在的路径按父目录 canonicalize（对齐 oracle `isWithinWorktree` 的 `relative` 判定语义，越界/`..` → 拒绝）。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): staging/discard/commit/upstream 本地操作"
```

---

## Task 6: ade-git worktree 创建支撑（命名/路径/base/前缀，纯函数）

**Files:**
- Create: `src-tauri/crates/ade-git/src/branch.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`
- Test: `branch.rs` 内 `#[cfg(test)]` + `tests/worktree.rs`（git 依赖部分）

**Interfaces:**
- Produces：
  - `pub fn sanitize_worktree_name(name: &str) -> Result<String, CoreError>`（空 → `CoreError::InvalidInput("Invalid worktree name")`）
  - `pub fn normalize_branch_prefix(raw: &str) -> String`
  - `pub fn select_branch_prefix_input(strategy: &str, custom: Option<&str>, git_username: Option<&str>) -> Option<String>`（`git-username|custom|none`）
  - `pub fn build_branch_name(prefix: Option<&str>, name: &str) -> String`
  - `pub fn resolve_git_username(repo_path: &str) -> Option<String>`
  - `pub const DEFAULT_BASE_REF_CANDIDATES: &[&str] = &["refs/remotes/origin/HEAD","refs/remotes/origin/main","origin/main","refs/remotes/origin/master","origin/master","refs/heads/main","main","refs/heads/master","master"];`
  - `pub fn resolve_default_base_ref(repo_path: &str) -> Result<String, CoreError>`
  - `pub fn resolve_create_base(repo_path: &str, explicit: Option<&str>, repo_base_ref: Option<&str>) -> Result<String, CoreError>`
  - `pub fn compute_worktree_path(root: &str, repo_basename: &str, nest: bool, name: &str) -> Result<String, CoreError>`
- 语义依据：`orca:src/main/ipc/worktree-logic.ts:36-62`（sanitize）、`:103-131`（computeWorktreePath/ensurePathWithinWorkspace）、`orca:src/shared/branch-prefix.ts`、`orca:src/main/git/git-username.ts:303-336`、`orca:src/main/git/repo-default-base-ref.ts:21-61`、`orca:src/main/worktree-create-base.ts:8-29`
- Consumes：Task 1 `run_git_in`

- [ ] **Step 1: 写失败测试（纯函数表驱动）**

```rust
#[test]
fn sanitize_keeps_unicode_word_chars_and_collapses_others() {
    assert_eq!(sanitize_worktree_name("feature/login").unwrap(), "feature-login");
    assert_eq!(sanitize_worktree_name(" 我的 工作..区 ").unwrap(), "我的-工作.区");
    assert!(sanitize_worktree_name("   ").is_ok()); // 空→"workspace"（以 oracle ipc/worktree-logic.ts 为准）
    assert!(sanitize_worktree_name("***").is_ok());
}

#[test]
fn normalize_branch_prefix_strips_and_collapses_slashes() {
    assert_eq!(normalize_branch_prefix(" team//frontend/ "), "team/frontend");
    assert_eq!(normalize_branch_prefix("///"), "");
}

#[test]
fn build_branch_name_joins_with_single_slash() {
    assert_eq!(build_branch_name(Some("alice"), "fix-auth"), "alice/fix-auth");
    assert_eq!(build_branch_name(None, "fix-auth"), "fix-auth");
    assert_eq!(build_branch_name(Some(""), "fix-auth"), "fix-auth");
}

#[test]
fn compute_worktree_path_nests_under_repo_basename() {
    assert_eq!(compute_worktree_path("/ws", "my-repo", true, "fix").unwrap(), "/ws/my-repo/fix");
    assert_eq!(compute_worktree_path("/ws", "my-repo", false, "fix").unwrap(), "/ws/fix");
    assert!(compute_worktree_path("/ws", "my-repo", false, "../escape").is_err());
}

#[test]
fn resolve_default_base_ref_prefers_verified_candidates_in_order() {
    // init repo（main 分支）→ 无 remote → default = "main"
    // 再 git update-ref refs/remotes/origin/master HEAD → default 变为 "refs/remotes/origin/master"？——
    // 候选顺序以 oracle repo-default-base-ref.ts:21-26 为准：origin/main 优先于 origin/master，refs/remotes 优先于本地
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

- [ ] **Step 3: 实现**

- `sanitize_worktree_name` 逐字照抄 oracle：按 code point 保留 `is_alphanumeric() || "._-"`，其余连续字符折叠为 `-`，`..`→`.`，trim 首尾 `[.-]`，空 → `"workspace"`（以 oracle 为准；若 oracle 抛 `Invalid worktree name` 则改为抛错——实现时读源文件确认并让测试断言与 oracle 一致）。
- `resolve_git_username`：`git config --get remote.origin.url` 命中 github.com 才启用；`git config --get github.user` → `git config --get user.username`；gh CLI 不调用。
- `resolve_default_base_ref`：磁盘 `git rev-parse --verify --quiet <candidate>^{commit}` 依次验证；`refs/remotes/origin/HEAD` 验证通过直接返回 `refs/remotes/origin/HEAD`（oracle 行为：优先验证过的 origin/HEAD）；全部失败 → `CoreError::InvalidInput("Could not resolve a default base ref. ...")`（准确文案读 `orca:src/main/ipc/worktree-remote.ts:2374-2379`）。
- `resolve_create_base`：explicit → repo_base_ref（验证通过才用）→ default。
- `compute_worktree_path`：root 规范化后必须为绝对路径；目标路径 `normalize` 后须在 root 前缀内（段边界）否则 `CoreError::InvalidInput`。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): worktree 命名/路径/base ref 纯函数（oracle 对齐）"
```

---

## Task 7: ade-git worktree add / remove / prune / 分支删除

**Files:**
- Create: `src-tauri/crates/ade-git/src/worktree_create.rs`、`src-tauri/crates/ade-git/src/worktree_remove.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`
- Test: `src-tauri/crates/ade-git/tests/worktree.rs`

**Interfaces:**
- Produces（`worktree_create.rs`）：
  - `pub struct AddWorktreeRequest { pub repo_path: String, pub worktree_path: String, pub branch: String, pub base_ref: String }`
  - `pub fn worktree_add(request: &AddWorktreeRequest) -> Result<(), CoreError>`（`git worktree add --no-track -b <branch> <path> <base_ref>`，超时 180s）
  - `pub fn configure_branch_base(worktree_path: &str, branch: &str, base_ref: &str) -> Result<(), CoreError>`（`config --local --replace-all branch.<branch>.base <base_ref>`；失败 → `config --local --remove-section branch.<branch>` 清理后返回 Err）
  - `pub fn ensure_push_auto_setup_remote(worktree_path: &str) -> Result<(), CoreError>`（先 `git config --get push.autoSetupRemote`（含 include/global）与 `--local` 检查；都未设置才写 `--local push.autoSetupRemote true`）
  - `pub fn sanitize_created_name(...)`（如需独立函数）
- Produces（`worktree_remove.rs`）：
  - `pub const REMOVAL_PREFLIGHT_TIMEOUT: Duration = 30s`
  - `pub fn assert_worktree_removable(repo_path: &str, worktree_path: &str, force: bool) -> Result<(), CoreError>`（列表重列校验存在 + locked 拒绝 + 非 force 脏检查 `git status --porcelain -z --untracked-files=all`）
  - `pub fn worktree_remove(repo_path: &str, worktree_path: &str, force: bool) -> Result<(), CoreError>`（`git worktree remove [--force] <path>`；失败 → `git worktree prune` 后重试一次）
  - `pub enum BranchDeleteOutcome { Deleted, Preserved { branch_name: String, head: Option<String> }, Skipped }`
  - `pub fn delete_branch(repo_path: &str, branch_ref: &str, force: bool) -> Result<BranchDeleteOutcome, CoreError>`（默认 `git branch -d -- <short>`，`force` → `-D`；`-d` 失败 → `Preserved`，`head` 取 `git rev-parse <branch_ref>`）
  - `pub fn force_delete_branch(repo_path: &str, branch_name: &str, expected_head: &str) -> Result<(), CoreError>`（`git update-ref -d refs/heads/<b> <expected>`；CAS 失败 → Err；随后 `git config --remove-section branch.<b>` 失败容忍）
- 语义依据：`orca:src/main/git/worktree-add.ts:150-244`、`orca:src/main/git/worktree-removal.ts:32-107`、`worktree-removal-preflight.ts:8-44`、`worktree-branch-removal.ts:54-179`、`orca:src/main/git/worktree-operation-options.ts:45-53`

- [ ] **Step 1: 写失败集成测试**

```rust
#[test]
fn worktree_add_creates_linked_worktree_on_new_branch() {
    // repo(commit on main) → worktree_add{path: sibling, branch: "feature/x", base_ref: "main"}
    // → worktree_list(repo) 含两项；linked .git 文件存在；branch = refs/heads/feature/x
    // → git -C <linked> config --local --get branch.feature/x.base == "main"
}

#[test]
fn ensure_push_auto_setup_remote_sets_once() {
    // worktree_add → ensure_push_auto_setup_remote → git -C <linked> config --local --get push.autoSetupRemote == "true"
}

#[test]
fn assert_worktree_removable_rejects_dirty_without_force() {
    // worktree_add → 写入未提交文件 → assert(force=false) → Err 文案 "Worktree has uncommitted or untracked changes."
    // → assert(force=true) → Ok
}

#[test]
fn worktree_remove_deletes_directory_and_registration() {
    // worktree_add → worktree_remove(force=false) → 路径不存在 && worktree_list 只余主工作树
}

#[test]
fn delete_branch_preserves_unmerged_branch_and_deletes_merged_one() {
    // linked worktree 上提交（未合并）→ 切回主工作树 → delete_branch(-d) → Preserved{branch_name, head}
    // 主分支上 fast-forward 合并后 → delete_branch → Deleted
}

#[test]
fn force_delete_branch_enforces_cas() {
    // preserved 分支 → force_delete_branch(错误 expected_head) → Err；正确 head → Ok 且分支不存在
}

#[test]
fn remove_locked_worktree_is_rejected() {
    // git worktree lock <path> → assert_worktree_removable → Err（含 "git worktree unlock"）
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git --test worktree
```

- [ ] **Step 3: 实现**

按 Interfaces 逐条实现；关键点：
- `worktree_add` 前先 `worktree_list(repo)` 校验路径未被占用（存在 → `CoreError::InvalidInput` 可识别冲突文案，供 oRust 内重试后缀；重试循环放 bridge Task 11，不在此层）。
- `assert_worktree_removable`：重列列表找不到目标 → Err（文案 `Worktree registration changed during deletion: ...`）；`locked` 块（porcelain 有 `locked` 行）→ Err 含 `git worktree unlock`；脏检查用 `run_git_in` 并容忍非零退出（git status 在主分支 linked 场景应成功；失败视为不可判定 → Err）。
- `delete_branch`：`branch_ref` 形如 `refs/heads/x`；short = 去前缀；非 `refs/heads/` 前缀或为空 → `Skipped`。`git branch -d` 失败时 stderr 里 `not fully merged` 才 `Preserved`；其它失败照常 Err（对齐 oracle 判定，实现时读源）。
- `worktree_remove`：删除目标目录不存在（已被手动删）→ 仍尝试 `worktree prune` 并视为成功（对齐 oracle 的 `forgetLocal` 语义由 bridge 层选择命令，这里足够）。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): worktree add/remove/prune 与分支保留/强删"
```

---

## Task 8: ade-git compare / history

**Files:**
- Create: `src-tauri/crates/ade-git/src/compare.rs`、`src-tauri/crates/ade-git/src/history.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`
- Test: `src-tauri/crates/ade-git/tests/compare_history.rs`

**Interfaces:**
- Produces（`compare.rs`，类型逐个对照 `src/shared/git-diff-compare-types.ts` 全字段 camelCase）：
  - `GitBranchChangeEntry { path, status, old_path?, added?, removed? }`
  - `GitBranchCompareSummary { base_ref, base_oid: Option<String>, compare_ref, head_oid: Option<String>, merge_base: Option<String>, changed_files: u64, commits_ahead?, commits_behind?, status: String, error_message? }`（status 取 `'ready'|'invalid-base'|'unborn-head'|'no-merge-base'|'error'`）
  - `GitBranchCompareResult { summary, entries }`
  - `GitCommitCompareSummary { commit_oid, parent_oid: Option<String>, compare_ref, base_ref, changed_files, status: 'ready'|'invalid-commit'|'error', error_message? }`
  - `GitCommitCompareResult { summary, entries }`
  - `pub fn branch_compare(worktree_path: &str, base_ref: &str) -> Result<GitBranchCompareResult, CoreError>`
  - `pub fn commit_compare(worktree_path: &str, commit_id: &str) -> Result<GitCommitCompareResult, CoreError>`
- Produces（`history.rs`，逐个对照 `src/shared/git-history-types.ts`）：
  - `pub const GIT_HISTORY_DEFAULT_LIMIT: u32 = 50; pub const GIT_HISTORY_MAX_LIMIT: u32 = 200;`
  - `pub fn history(worktree_path: &str, limit: Option<u32>, base_ref: Option<&str>) -> Result<GitHistoryResult, CoreError>`
- Produces（`diff.rs` 复用，必要时把 `diff_refs` 调整为 `pub`）：
  - `pub fn branch_diff(worktree_path, base_oid, head_oid, file_path, old_path) -> Result<GitDiffResult, CoreError>`
  - `pub fn commit_diff(worktree_path, commit_oid, parent_oid: Option<&str>, file_path, old_path) -> Result<GitDiffResult, CoreError>`
- 语义依据：`orca:src/main/git/source-control/branch-compare.ts`、`commit-compare.ts`、`branch-diff.ts`、`commit-diff.ts`、`compare-ref-oids.ts`、`orca:src/shared/git-history.ts:1-240`（命令序列、`GIT_HISTORY_COMMIT_FORMAT`、`parseGitHistoryLog`、ref 解析与 mergeBase）

- [ ] **Step 1: 写失败集成测试**

```rust
#[test]
fn branch_compare_lists_changed_files_and_summary() {
    // main 上 commit A → 分支 feature commit B（改 README + 新增 file.txt）
    // branch_compare(repo, "main") → summary.status=="ready"、changed_files==2、
    // entries 含 {path:"README.md",status:Modified} 与 {path:"file.txt",status:Added}，均带 added/removed
}

#[test]
fn branch_compare_invalid_base_reports_status() {
    // branch_compare(repo, "does-not-exist") → summary.status=="invalid-base" 且 entries 空（以 oracle 状态机为准）
}

#[test]
fn commit_compare_lists_single_commit_files() {
    // commit_compare(repo, <B oid>) → entries 数量与改动一致、parent_oid 为 A
}

#[test]
fn branch_diff_and_commit_diff_return_whole_file_contents() {
    // branch_diff(base_oid=A, head_oid=B, "README.md") → original=A 版本内容 modified=B 版本内容
    // commit_diff(commit_oid=B, parent_oid=Some(A), "README.md") 同上
}

#[test]
fn history_returns_items_with_refs_and_limit() {
    // 5 次提交 → history(limit=Some(2)) → items.len()==2、limit==2、has_more==true
    // → currentRef.revision==HEAD；无 remote 时 hasIncomingChanges/hasOutgoingChanges==false
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git --test compare_history
```

- [ ] **Step 3: 实现**

- `branch_compare`：解析 base（`rev-parse --verify --quiet <base>^{commit}`）→ invalid-base；HEAD unborn → unborn-head；`merge-base <base> HEAD` 失败 → no-merge-base（空 mergeBase）；否则 `git diff --name-status -z -M <mergeBase>`（改 rename/copy 状态解析：`R<score>`/`C<score>` + 两段路径）得 entries 骨架 → `git diff --numstat -z -M <mergeBase>` 附 added/removed；`commitsAhead/Behind` 用 `git rev-list --count <mergeBase>..HEAD` / `<base>..HEAD`（以 oracle 实现为准）；`changed_files = entries.len()`。逐行对照 `orca:src/main/git/source-control/branch-compare.ts`。
- `commit_compare`：`rev-parse --verify <commit>^{commit}` → invalid-commit；`git diff --name-status -z -M <parent?>`（首提交用空树 `4b825dc642cb6eb9a060e54bf8d69288fbee4904`，对齐 oracle compare-ref-oids.ts）；`compare_ref=commit_id`、`base_ref=parent_oid ?? ""`（以 oracle 为准）。
- `branch_diff`/`commit_diff`：复用 Task 4 的 blob 读取（左侧 rev=base_oid/父 OID、path=old_path ?? file_path；右侧 rev=head_oid/commit_oid）。
- `history`：逐步复刻 `orca:src/shared/git-history.ts` 的序列（resolveCommit('HEAD') → resolveCurrentRef → resolveUpstreamRef（`for-each-ref`）→ resolveNamedRef(baseRef) → mergeBase → `git log --format=... -z --topo-order --decorate=full -n<limit+1> <headOid>` → parse），格式常量与解析器从 oracle 逐字复制为 Rust 实现；空仓库返回 `{items:[],hasIncomingChanges:false,hasOutgoingChanges:false,hasMore:false,limit}`。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-git
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(git): branch/commit compare 与 history"
```

---

## Task 9: ade-store worktree 元数据存储

**Files:**
- Create: `src-tauri/crates/ade-store/src/worktree_meta_store.rs`
- Modify: `src-tauri/crates/ade-store/src/lib.rs`（`pub mod worktree_meta_store; pub use ...`）
- Test: `worktree_meta_store.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces：
  - `pub struct WorktreeMetaStore { file: JsonFile }`
  - `impl WorktreeMetaStore { pub fn load(path: impl Into<PathBuf>) -> Self; pub fn items(&self) -> Map<String, Value>; pub fn get(&self, worktree_id: &str) -> Option<Value>; pub fn merge(&self, worktree_id: &str, updates: &Value) -> Result<Value, StoreError>; pub fn remove(&self, worktree_id: &str) -> Result<bool, StoreError>; pub fn persist_sort_order(&self, ordered_ids: &[String]) -> Result<(), StoreError>; }`
  - `pub const WORKTREE_META_FIELDS: &[&str] = &[...]`（白名单= `src/shared/worktree/meta-types.ts` 的 `WorktreeMeta` 键，实现时逐字抄录）
- 内部实现：`WorktreeMetaStore { path: PathBuf, inner: Mutex<Value> }`——全部方法 `&self`（内部锁保护内存态），bridge 侧以 `Arc<WorktreeMetaStore>` 共享并可直接移入 `run_blocking` 闭包；`JsonFile` 在每次 `persist()` 时按 `path` 构造（复用原子写/轮换/损坏回退）
- 持久化形状：`{ "schemaVersion": 1, "items": { "<worktreeId>": { ... } } }`
- Consumes：`JsonFile`（`ade-store/src/json_file.rs`）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn merge_persists_whitelisted_fields_and_returns_merged_value() {
    let store = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let merged = store.merge("r::/p", &json!({"displayName":"fix","isPinned":true,"bogus":1})).unwrap();
    assert_eq!(merged["displayName"], "fix");
    assert_eq!(merged["isPinned"], true);
    assert!(merged.get("bogus").is_none());
    assert_eq!(WorktreeMetaStore::load(dir.file("worktrees.json")).get("r::/p").unwrap()["displayName"], "fix");
}

#[test]
fn merge_with_null_or_empty_display_name_clears_the_key() {
    store.merge("r::/p", &json!({"displayName":"fix"})).unwrap();
    let merged = store.merge("r::/p", &json!({"displayName":""})).unwrap();
    assert!(merged.get("displayName").is_none());
}

#[test]
fn remove_deletes_entry_and_reports_presence() {
    store.merge("r::/p", &json!({"isUnread":true})).unwrap();
    assert!(store.remove("r::/p").unwrap());
    assert!(store.get("r::/p").is_none());
    assert!(!store.remove("r::/p").unwrap());
}

#[test]
fn persist_sort_order_assigns_indexes() {
    store.persist_sort_order(&["a".into(), "b".into()]).unwrap();
    assert_eq!(store.get("a").unwrap()["sortOrder"], 0);
    assert_eq!(store.get("b").unwrap()["sortOrder"], 1);
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-store
```

- [ ] **Step 3: 实现**

内部持有内存 `Value`（load 一次）；`persist()` 写回。`merge`：只接受白名单键；`Value::Null` 或 `displayName:""` → 移除该键；其余原样写入；返回该 worktree 的完整 meta 值。`items` 返回克隆。损坏/缺失 → `{schemaVersion:1,items:{}}`。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-store
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(store): worktree 元数据持久化（worktrees.json）"
```

---

## Task 10: ade-bridge git 命令、取消注册表与 specta 登记

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/git.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`、`src-tauri/crates/ade-bridge/src/state.rs`、`src-tauri/crates/ade-bridge/src/specta_export.rs`
- Modify: `src-tauri/crates/ade-bridge/Cargo.toml`（`ade-git` 加 `features = ["specta"]`）
- Create（生成物）：`src/bridge/real/generated/tauri-bindings.ts`（重生成）
- Test: `src-tauri/crates/ade-bridge/src/commands/git.rs` 单测 + `src-tauri/crates/ade-bridge/tests/git_commands.rs`

**Interfaces:**
- Produces（命令，全部 `#[tauri::command] #[specta::specta]`，参数 `{ args: XxxArgs }`）：
  - `git_status(args: GitStatusArgs) -> Result<GitStatusResult, BridgeError>`（`request_token` 注册取消）
  - `git_cancel_status(args: GitCancelStatusArgs{request_token}) -> Result<(), BridgeError>`
  - `git_diff(args) -> Result<GitDiffResult, BridgeError>`
  - `git_stage/git_bulk_stage/git_unstage/git_bulk_unstage/git_discard/git_bulk_discard`
  - `git_commit(args) -> Result<GitCommitOutcome, BridgeError>`
  - `git_upstream_status(args) -> Result<GitUpstreamStatus, BridgeError>`
  - `git_conflict_operation(args) -> Result<GitConflictOperation, BridgeError>`
  - `git_branch_compare/git_commit_compare/git_branch_diff/git_commit_diff/git_history`
- Produces（state）：
  - `pub struct GitCancelRegistry { ... }`：`register(&self, token: &str) -> CancelToken`、`finish(&self, token: &str)`、`cancel(&self, token: &str) -> bool`（`AppState` 新字段 `git_cancels: GitCancelRegistry`）
- Produces（参数映射）：
  - `impl GitStatusArgs { pub fn to_status_options(&self) -> ade_git::status_read::StatusOptions }`（`limit` 由 `BranchLineTotalMergeBase`/`IncludeLineStats` 等映射；测试可直接 `serde_json::from_value::<GitStatusArgs>(json!({...}))`）
- 阻塞执行：所有 git 子进程经既有 `run_blocking` 包装（模式照抄 `commands/worktrees.rs:187-206`：锁内取数据、锁外执行）
- 语义：命令薄包装，参数逐字对照 TS 调用点（Task 13 的契约测试会锁定）；错误经 `From<CoreError>` 变 `{message}`

- [ ] **Step 1: 写失败测试**

`tests/git_commands.rs`（可直接调用纯函数路径：为每个命令抽出同文件 `pub fn xxx_impl(...)` 便于测试，命令体只做 state 解包）：

```rust
#[test]
fn status_args_map_to_status_options() {
    let args: GitStatusArgs = serde_json::from_value(json!({"worktreePath":"/tmp/x"})).unwrap();
    let options = args.to_status_options();
    assert_eq!(options.limit, None); // None 由 resolve_status_limit 落 1000
    assert!(!options.include_ignored);
}

#[test]
fn cancel_registry_cancels_registered_token_once() {
    let registry = GitCancelRegistry::new();
    let token = registry.register("t1");
    assert!(!token.is_cancelled());
    assert!(registry.cancel("t1"));
    assert!(token.is_cancelled());
    assert!(!registry.cancel("t1")); // 已移除
}

#[test]
fn status_impl_returns_empty_entries_for_non_repo() {
    let dir = tempdir();
    let result = ade_git::status_read::status(dir.path().to_str().unwrap(), &StatusOptions::default(), None).unwrap();
    assert!(result.entries.is_empty());
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-bridge --test git_commands
```

- [ ] **Step 3: 实现**

- `state.rs`：`AppState` 加 `pub git_cancels: GitCancelRegistry`；`initialize` 构造；`GitCancelRegistry` 用 `Mutex<HashMap<String, CancelToken>>`。
- `git_status`：`request_token`（或由 bridge 生成 uuid）→ `register` → `run_blocking(move || ade_git::status_read::status(&path, &options, Some(&token)))` → `finish`。
- 其余命令直接映射 `ade_git` 调用；`commit` 返回 `CommitOutcome`（域错误不 reject）。
- `specta_export.rs`：`collect_commands!` 追加 17 个命令；`export_lists_every_command` 数组追加命令名；`cargo run -p ade-bridge --bin export-bindings` 重新生成并检查 diff。
- 注意：`git_diff` 返回的 `GitDiffResult` 带 `kind` 判别字段，specta 导出为 tagged union；与 TS `GitDiffResult` 对齐（生成物仅参考，契约以既有 TS 为准）。

- [ ] **Step 4: 跑测试确认通过 + bindings 新鲜度**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-bridge
cargo run -p ade-bridge --bin export-bindings
git diff --stat src/bridge/real/generated/tauri-bindings.ts
```

预期：bindings diff 仅包含新增命令与类型；新鲜度测试绿。

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(bridge): git 命令面、取消注册表与 bindings 登记"
```

---

## Task 11: ade-bridge worktrees 生命周期（create/remove/forget/forceDelete/updateMeta）与元数据投影

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/worktrees.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/project_groups.rs`（`revoke_root_if_unused` 提为 `pub(crate)`）
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（`pub worktree_meta: Arc<WorktreeMetaStore>`，data_dir 下 `worktrees.json`；访问器 `worktree_meta_store()`）
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`（6 新命令 + 必要 `.typ`）
- Test: `src-tauri/crates/ade-bridge/tests/worktree_lifecycle.rs`

**Interfaces:**
- Produces（投影合并）：
  - `pub fn apply_worktree_meta(worktree: &mut Worktree, meta: Option<&Value>)`：白名单键合并（`displayName→displayName + displayNameMode="custom"`、`comment`、`linkedIssue`、`linkedPR`、`linkedLinearIssue`、`isArchived`、`isUnread`、`isPinned`、`sortOrder`、`lastActivityAt`、`workspaceStatus`）
  - `list_worktrees(repo, folder_workspaces, meta_items: &Map<String, Value>, fs)`（签名扩展；`worktrees_list/list_all/updateMeta` 调用点与既有测试同步更新）
  - `git_worktree(repo, entry, meta: Option<&Value>) -> Worktree`
- Produces（命令与参数，`{ args }` 包裹、camelCase）：
  - `worktrees_create(args: WorktreesCreateArgs) -> Result<WorktreesCreateResult, BridgeError>`：参数子集 `repoId, name, displayName?, baseBranch?, branchNameOverride?, workspaceStatus?, manualOrder?, createdWithAgent?`（其余 TS 字段被 serde 忽略）；结果 `{ worktree, warnings? }`
  - `worktrees_remove(args: WorktreesRemoveArgs{worktreeId, hostId?, force?, allowUnverifiedPtyStop?, skipArchive?, snapshotPruneBatchId?}) -> Result<WorktreesRemoveResult{preservedBranch?: {branchName, head?}}, BridgeError>`
  - `worktrees_forget_local(args: {worktreeId, hostId?, snapshotPruneBatchId?}) -> Result<WorktreesRemoveResult, BridgeError>`
  - `worktrees_force_delete_preserved_branch(args: {worktreeId, branchName, expectedHead, hostId?}) -> Result<{deleted: bool}, BridgeError>`
  - `worktrees_update_meta(args: {worktreeId, executionHostId?, updates}) -> Result<Worktree, BridgeError>`
  - `worktrees_persist_sort_order(args: {orderedIds: Vec<String>}) -> Result<(), BridgeError>`
- 可测编排函数（命令体只做 state 解包 + `run_blocking`）：
  - `pub fn create_worktree_impl(repo: &Value, settings: &Value, meta: &WorktreeMetaStore, fs: &FsService, args: &WorktreesCreateArgs) -> Result<Worktree, BridgeError>`
  - `pub fn remove_worktree_impl(repo: &Value, meta: &WorktreeMetaStore, fs: &FsService, worktree_id: &str, force: bool) -> Result<WorktreesRemoveResult, BridgeError>`
- 语义依据：`orca:src/main/ipc/worktree-remote.ts:2301-3025`（create 主流程）、`orca:src/main/ipc/worktree-logic.ts`、`orca:src/main/git/worktree-add.ts`、`orca:src/main/git/worktree-removal*.ts`；A 既有模式：`worktrees.rs:154-220`（锁外 run_blocking）、`repos.rs:448-451`（事件尾调）、`project_groups.rs:305-322`（revoke 判定）

- [ ] **Step 1: 写失败测试**

`tests/worktree_lifecycle.rs`（tempdir + `init_git_repo` helper；settings 用 `json!({"workspaceDir": <temp>/ws, "nestWorkspaces": true, "branchPrefix": "none"})`，meta store 指向 temp 文件）：

```rust
#[test]
fn create_worktree_adds_git_worktree_writes_meta_and_authorizes() {
    let repo = init_git_repo(&dir, "my-repo"); // main
    let settings = json!({"workspaceDir": dir.path.join("ws"), "nestWorkspaces": true, "branchPrefix": "none"});
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let args = create_args(&repo_value, "fix-auth");
    let worktree = create_worktree_impl(&repo_value, &settings, &meta, &fs, &args).unwrap();
    assert_eq!(worktree.path, format!("{}/ws/my-repo/fix-auth", dir.path));
    assert_eq!(worktree.branch, "refs/heads/fix-auth");
    assert_eq!(worktree.display_name, "fix-auth");
    assert_eq!(worktree.workspace_status, "in-progress");
    assert!(fs.resolve(&worktree.path).is_ok()); // 已授权
    assert_eq!(meta.get(&worktree.id).unwrap()["displayName"], "fix-auth");
}

#[test]
fn create_worktree_retries_name_suffix_on_conflict() {
    // 先创建 fix-auth；再次同 name → 路径/分支冲突 → 自动 fix-auth-2
}

#[test]
fn create_worktree_uses_default_base_ref_when_absent() {
    // 无 baseBranch、无 remote → base=main（resolve_default_base_ref），创建成功
}

#[test]
fn remove_worktree_deletes_and_preserves_unmerged_branch() {
    // create → linked 上提交（未合并）→ remove(force=false) →
    // 目录消失、注册消失、result.preservedBranch.branchName == "fix-auth"、meta 清理
}

#[test]
fn remove_worktree_dirty_requires_force() {
    // create → 写未提交文件 → remove(force=false) → Err 含 "Worktree has uncommitted or untracked changes."
    // → remove(force=true) → Ok
}

#[test]
fn forget_local_keeps_directory_and_drops_meta() {
    // create → forget → 目录仍在、git worktree 注册仍在、meta 被清、fs.resolve 变为拒绝
}

#[test]
fn force_delete_preserved_branch_removes_branch() {
    // remove 保留分支后 → force_delete(branchName, expectedHead) → {deleted:true} 且 rev-parse 失败
}

#[test]
fn update_meta_merges_whitelist_and_projection_reflects() {
    // create → update_meta({displayName:"renamed", isPinned:true, isUnread:true}) →
    // 返回 Worktree.displayName=="renamed"（displayNameMode=="custom"）、isPinned、isUnread
    // list_worktrees 再读仍带这些字段
}

#[test]
fn persist_sort_order_sets_indexes_and_projection_order() { /* orderedIds [b,a] → sortOrder b=0,a=1 */ }
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-bridge --test worktree_lifecycle
```

- [ ] **Step 3: 实现**

- 命令锁模式照抄 `worktrees.rs:187-206`：projects/store 锁内取 clone（repo 行、settings 快照、meta 快照、home），锁外 `run_blocking` 调 `*_impl`。
- `create_worktree_impl` 流程：
  1. `repo_kind` 非 git → `BridgeError::message("Worktrees can only be created for git repositories")`（文案以 oracle 渲染端可达文案为准，实现时查 `orca:src/main/ipc/worktree-remote.ts` 对应分支）。
  2. `name = sanitize_worktree_name(&args.name)`；`prefix = select_branch_prefix_input(settings.branchPrefix, settings.branchPrefixCustom, resolve_git_username(repo.path))`；`branch_leaf = branch_name_override.unwrap_or(name)`；`branch = build_branch_name(prefix, leaf)`。
  3. `base = resolve_create_base(repo_path, args.base_branch, repo.worktreeBaseRef)`。
  4. `root = repo.worktreeBasePath || settings.workspaceDir`；`path = compute_worktree_path(root, basename(repo.path), settings.nestWorkspaces != false, name)`。
  5. 重试循环 0..100：尝试 `worktree_add`；错误命中「路径已存在/分支已存在/already exists」→ 后缀 `-{n}` 同时改 path 与 branch，继续；否则上抛。超限 → `BridgeError::message("Worktree creation failed: name conflict could not be resolved")`。
  6. `configure_branch_base` / `ensure_push_auto_setup_remote` 失败 → 收集进 `warnings`（不失败）。
  7. meta 合并 `{displayName: args.display_name.unwrap_or(name), workspaceStatus: args.workspace_status.unwrap_or("in-progress"), createdWithAgent?, sortOrder/manualOrder 若提供}`；`fs.authorize_root(path)`。
  8. 返回 `list_worktrees(repo, &[], meta.items(), fs)` 中匹配 `worktree_id(repo_id, path)` 的行。
- `remove_worktree_impl`：解析 `worktree_id`（`split_once("::")`）→ 列表定位条目（拿 branch/head；找不到 → `BridgeError`）→ `assert_worktree_removable` → `worktree_remove` → `delete_branch(branch, force=false)`（除非 meta `preserveBranchOnDelete`）→ `meta.remove` → `revoke_root_if_unused(store, fs, path)`（在命令层调用）→ `emit_worktrees_changed`。
- `forget_local`：仅 meta.remove + revoke + 事件；目录/注册不动。
- `update_meta`：`meta.merge` → 读 repo + folderWorkspaces + meta → 投影返回。（若 worktree 不在了：仍更新 meta 并返回合成行？以 oracle 为准——实现时确认；最少返回更新后的投影，找不到git条目 → `BridgeError`。）
- `worktrees_list/list_all` 调用点适配新签名。
- specta：登记 6 命令；重生成 bindings。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-bridge
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(bridge): worktree 创建/删除/遗忘/强删/元数据命令"
```

---

## Task 12: ade-bridge repos.create 与 base ref 查询

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/repos.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Test: `src-tauri/crates/ade-bridge/tests/repos.rs`（追加用例）

**Interfaces:**
- Produces：
  - `pub struct ReposCreateArgs { parent_path: String, name: String, kind: Option<RepoKind> }`
  - `pub fn create_repo(store: &mut ProjectsStore, fs: &FsService, args: &ReposCreateArgs, added_at_ms: u64) -> Value`：返回 `json!({"repo": repo})` 或 `json!({"error": message})`（不 reject，对齐 `repos_add` 的错误契约特例 `repos.rs:470-476`）
  - `repos_create(args) -> Result<Json, BridgeError>`（成功/域错误都返回 Json；成功后 `emit_repos_changed` + `emit_worktrees_changed`）
  - `pub struct GetBaseRefDefaultArgs { repo_id, host_id: Option<String> }`；`repos_get_base_ref_default(args) -> Result<Json, BridgeError>` → `{defaultBaseRef: string|null, remoteCount: number}`（folder repo/找不到 → `{null, 0}`）
  - `repos_search_base_refs(args: {repoId, query, limit?, hostId?}) -> Result<Json, BridgeError>` → `string[]`
  - `repos_search_base_ref_details(args: 同上) -> Result<Json, BridgeError>` → `[{refName, localBranchName}]`
  - ade-git 新增（Task 6 可复用 + 本轮补齐）：`pub fn remote_count(repo_path) -> u32`、`pub fn search_base_refs(repo_path, query, limit) -> Vec<BaseRefSearchResult>`
- 语义依据：`orca:src/main/ipc/repos/repo-creation-handlers.ts:132-296`（create 逐字：name/parent 校验、空目录判定、`git init` + `--allow-empty -m "Initial commit"`、identity 错误文案、失败清理）、`orca:src/main/ipc/repos/base-ref-query-handlers.ts:27-108`（folder → null/0；`getRemoteCount` = `git remote` 行数）、`orca:src/main/git/repo-default-base-ref.ts:77`、`orca:src/main/git/repo-base-ref-search.ts:164-214`（limit 校验/clamp 常量以 oracle 为准）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn create_git_repo_makes_directory_initial_commit_and_registers() {
    // parent=dir/parents；create({name:"demo", kind:"git"}) →
    // {repo.path == parent/demo}、git log -1 --format=%s == "Initial commit"、
    // store.repos 含该行、fs.resolve(repo.path) 已授权
}

#[test]
fn create_repo_rejects_name_with_slash_and_empty_name() {
    // {error:"Name cannot contain slashes or be \".\" / \"..\""} / {error:"Name cannot be empty"}
}

#[test]
fn create_repo_rejects_non_empty_existing_directory() {
    // 预建目录写一个文件 → {error:"\"demo\" already exists at this location and is not empty."}
}

#[test]
fn create_repo_reuses_existing_empty_directory() {
    // 预建空目录 → 成功且不报错
}

#[test]
fn create_repo_identity_failure_reports_setup_hint() {
    // env GIT_CONFIG_GLOBAL=/dev/null 且无 user.name/email → {error 含 "Git author identity is not configured"}
    // 且创建的目录被清理（若为新建），或 .git 被清理（预存在目录）
}

#[test]
fn base_ref_default_and_remote_count() {
    // repo 无 remote → {defaultBaseRef:"main", remoteCount:0}
    // folder repo → {defaultBaseRef:null, remoteCount:0}
}

#[test]
fn search_base_refs_filters_and_limits() {
    // 建 local 分支 feature-x、远端 ref refs/remotes/origin/main 等 → query "origin" 命中、limit 截断
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-bridge --test repos
```

- [ ] **Step 3: 实现**

- `create_repo`：校验顺序与文案逐字照 oracle；`parent_path` 必须绝对（相对 → `{error:"Parent directory must be an absolute path"}`）；mkdir parent；target 存在则 read_dir 判空；不存在 mkdir；git kind → `ade_git::run_git_in(target, ["init"], 10s)`（失败 → 清理并 `{error:"Failed to initialize git repository: <stderr>"}`）+ `["commit","--allow-empty","-m","Initial commit"]`（失败：identity 正则 `Please tell me who you are|user\.name|user\.email` → 逐字 hint 文案；否则 `{error:"Failed to create initial commit: <stderr>"}`；清理规则：自建目录 rm -rf、预存在目录 rm -rf .git）；成功后复用 `add_repo(store, fs, target, RepoKind::Git, Some(name), added_at_ms)`。
- `create_repo` 去重：目标路径已注册 → 直接返回既有 repo（`{repo}`）。
- base refs：folder kind → `{null,0}`/`[]`；local git kind → `getBaseRefDefault`（收紧 Task 6 `resolve_default_base_ref` 或新函数，返回 Option）、`remote_count`（`git remote` 非空行数，失败日志 + 0）、`search_base_refs`（`for-each-ref --format=%(refname:short)` `refs/heads` + `refs/remotes`，排除 `*/HEAD`，query 去空白后子串匹配，limit 语义与常量照 oracle `repo-base-ref-search.ts`）。
- specta 登记 4 命令；重生成 bindings。

- [ ] **Step 4: 跑测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace -p ade-bridge
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(bridge): repos.create 与 base ref 查询命令"
```

---

## Task 13: TS git 真实适配层与契约测试

**Files:**
- Create: `src/bridge/real/git.ts`、`src/bridge/real/git.test.ts`
- Modify: `src/bridge/create-api.ts`（`git` 加入 `RealDomains`/`createRealDomains`）
- Modify: `src/bridge/real/parity.test.ts`、`src/bridge/create-api.test.ts`（如断言受影响）
- Modify: `src/bridge/mock/git-api.ts`（保持 mock 现状，仅必要时对齐）

**Interfaces:**
- Produces：`export function createGitRealApi(): Merged<GitInspectionApi & GitOperationApi>`
- 方法映射（未列出的保持 `withMethodFallback` 未实现）：
  - `status: (args) => invokeCommand('git_status', { args })`
  - `cancelStatus: (args) => invokeCommand('git_cancel_status', { args })`
  - `diff: (args) => invokeCommand('git_diff', { args })`
  - `stage/bulkStage/unstage/bulkUnstage/discard/bulkDiscard: (args) => invokeCommand('git_<snake>', { args })`
  - `commit: (args) => invokeCommand('git_commit', { args })`
  - `upstreamStatus: (args) => invokeCommand('git_upstream_status', { args })`
  - `conflictOperation: (args) => invokeCommand('git_conflict_operation', { args })`
  - `branchCompare/commitCompare/branchDiff/commitDiff/history: (args) => invokeCommand('git_<snake>', { args })`
- Consumes：`invokeCommand`/`subscribeToEvent`（`src/bridge/real/invoke.ts`）；mock 参考 `src/bridge/mock/git-api.ts`

- [ ] **Step 1: 写失败契约测试**

```ts
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }))

it('maps git.status to git_status with the args envelope', async () => {
  await createGitRealApi().status({ worktreePath: '/tmp/repo' })
  expect(invoke).toHaveBeenCalledWith('git_status', { args: { worktreePath: '/tmp/repo' } })
})

it.each([
  ['stage', 'git_stage'], ['bulkStage', 'git_bulk_stage'], ['unstage', 'git_unstage'],
  ['bulkUnstage', 'git_bulk_unstage'], ['discard', 'git_discard'], ['bulkDiscard', 'git_bulk_discard'],
  ['commit', 'git_commit'], ['diff', 'git_diff'], ['cancelStatus', 'git_cancel_status'],
  ['upstreamStatus', 'git_upstream_status'], ['conflictOperation', 'git_conflict_operation'],
  ['branchCompare', 'git_branch_compare'], ['commitCompare', 'git_commit_compare'],
  ['branchDiff', 'git_branch_diff'], ['commitDiff', 'git_commit_diff'], ['history', 'git_history']
])('maps git.%s to %s', async (method, command) => {
  const api = createGitRealApi() as Record<string, (args: unknown) => Promise<unknown>>
  await api[method]({ worktreePath: '/tmp/repo' })
  expect(invoke).toHaveBeenCalledWith(command, { args: { worktreePath: '/tmp/repo' } })
})

it('propagates rejected command errors as Error(message)', async () => {
  vi.mocked(invoke).mockRejectedValueOnce({ message: 'boom' })
  await expect(createGitRealApi().status({ worktreePath: '/tmp/x' })).rejects.toThrow('boom')
})

it('keeps unimplemented surface loud', async () => {
  await expect(createGitRealApi().generateCommitMessage({ worktreePath: '/x' } as never)).rejects.toThrow(
    'git.generateCommitMessage'
  )
})
```

- [ ] **Step 2: 跑测试确认失败**

```bash
pnpm vitest run src/bridge/real/git.test.ts
```

- [ ] **Step 3: 实现**

`src/bridge/real/git.ts` 按 Interfaces 实现（模式照抄 `repos.ts`）；`create-api.ts`：import + `RealDomains` 加 `'git'` + `createRealDomains()` 加 `git: createGitRealApi()`。

- [ ] **Step 4: 跑测试确认通过**

```bash
pnpm vitest run src/bridge/real/git.test.ts src/bridge
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(bridge): git 域真实适配层与 create-api 接线"
```

---

## Task 14: TS worktrees/repos 扩展与 parity 更新

**Files:**
- Modify: `src/bridge/real/worktrees.ts`、`src/bridge/real/repos.ts`
- Modify: `src/bridge/real/worktrees.test.ts`、`src/bridge/real/repos.test.ts`
- Modify: `src/bridge/real/parity.test.ts`（缺失清单迁移）
- Test: 上述测试文件

**Interfaces:**
- Produces（worktrees 新增映射）：
  - `create: (args) => invokeCommand('worktrees_create', { args })`
  - `remove: (args) => invokeCommand('worktrees_remove', { args })`
  - `forgetLocal: (args) => invokeCommand('worktrees_forget_local', { args })`
  - `forceDeletePreservedBranch: (args) => invokeCommand('worktrees_force_delete_preserved_branch', { args })`
  - `updateMeta: (args) => invokeCommand('worktrees_update_meta', { args })`
  - `persistSortOrder: (args) => invokeCommand('worktrees_persist_sort_order', { args })`
- Produces（repos 新增映射）：
  - `create: (args) => invokeCommand('repos_create', { args })`
  - `getBaseRefDefault: (args) => invokeCommand('repos_get_base_ref_default', { args })`
  - `searchBaseRefs: (args) => invokeCommand('repos_search_base_refs', { args })`
  - `searchBaseRefDetails: (args) => invokeCommand('repos_search_base_ref_details', { args })`
- 其余远端/克隆/host 方法保持 `withMethodFallback`。

- [ ] **Step 1: 写失败契约测试**

在每个域测试文件中加入命令名/参数包裹断言（模式同 Task 13），并覆盖形状：例如 `worktrees.create` 返回 `{worktree}` 透传、`remove` 返回 `{preservedBranch}` 透传、`updateMeta` 返回完整 `Worktree` 透传、`repos.create` 的 `{error}` 结果不被转成 reject。

- [ ] **Step 2: 跑测试确认失败**

```bash
pnpm vitest run src/bridge/real/worktrees.test.ts src/bridge/real/repos.test.ts
```

- [ ] **Step 3: 实现 + parity 清单迁移**

- 适配器补齐映射。
- `parity.test.ts`：把已实现方法从「real 缺失/reject」清单移入「已实现」清单，并保留未实现清单（`repos.create` 在 A 的清单里等）；确保 parity 对 mock/real 的未实现方法仍断言同拒。
- 检查 `create-api.test.ts` 对 `git` 的既有断言（此前 git 在 mock 域）→ 更新为 real 模式走 real（可用 `vi.stubEnv('VITE_ADE_BRIDGE','mock')` 的回退断言保留）。

- [ ] **Step 4: 跑测试确认通过**

```bash
pnpm vitest run src/bridge
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(bridge): worktrees/repos 真实适配扩展与 parity 更新"
```

---

## Task 15: 全量门禁、手工验收与收尾

**Files:**
- Create: `docs/phase1b-worktree-git-record.md`
- Modify: `docs/phase1a-open-project-record.md`（如需要：把「B 对接需知」标注为已处理/延后项引用）

- [ ] **Step 1: 全量门禁**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
pnpm test
```

预期：cargo 全绿（新增 crate 测试）；`pnpm test` 0 failed（既有 3847 文件 + 新增）。

- [ ] **Step 2: 手工验收（`pnpm dev`，逐项记录证据到 SDD 目录）**

1. 添加本地 git 仓库 → 侧栏创建 worktree（默认 base、nest 目录）→ 侧栏出现并可切换
2. 在新 worktree 编辑文件 → Git 面板出现 status（modified/untracked）
3. stage/unstage/bulk → 行状态流转；diff 打开显示两侧内容
4. commit → `git log` 可见；工作区变干净
5. 删除 worktree：干净直删；脏目录 force；未合并分支保留 → 强删
6. 重命名/pin/已读 → 重启应用后恢复（`worktrees.json`）
7. 新建项目（repos.create）：空目录 → git init + Initial commit；已用 `settings.json`/`ui-state.json` 不受影响
8. `VITE_ADE_BRIDGE=mock pnpm dev` 回退正常
9. 启动冒烟：日志 0 error / 0 panic

- [ ] **Step 3: 写记录**

`docs/phase1b-worktree-git-record.md`：提交序、crate/命令增量清单、oracle 对齐语义与偏差（spec §8）、测试证据（cargo/pnpm 计数）、手工验收结果、延后项（远端/AI/hooks/Windows）。

- [ ] **Step 4: 提交并交付**

```bash
git add -A && git commit -m "docs: Phase 1 子项目 B 收尾记录"
```

调用 superpowers:finishing-a-development-branch：确认 base `main`、全绿证据，向用户给出合并/PR/保留选项（本计划任务内不自动合并）。

---

## 自检记录

- **Spec 覆盖**：spec §2.1 方法表 → Task 2/3（status）/4（diff）/5（staging/commit/upstream）/6/7（worktree create/remove）/8（compare/history）/9/11（meta）/12（repos）；§2.2 未实现项在各任务 Interfaces 中显式排除；§3 架构 → 文件结构；§4 语义 → 各任务「语义依据」指向 oracle 逐字文件；§5 错误/取消 → Task 1/10；§6 测试 → 各任务测试步骤 + Task 15。
- **占位符扫描**：无 TBD/TODO；不确定处均给出 oracle `file:line` 作为判定源，且要求实现时读源确认（非臆测）。
- **类型一致性**：`GitStatusResult`/`GitDiffResult`/`Worktree`/`WorktreesCreateResult` 类型在 Task 2/4/11 定义与 Task 13/14 契约测试断言一致；`run_git_in`/`CancelToken` 在 Task 1 定义、Task 3/5/6/7/8 消费；`WorktreeMetaStore` 在 Task 9 定义、Task 11 消费；命令名在 Task 10-12 定义、Task 13-14 断言。
- **已知执行风险**：Task 3 的「流式 kill git」若实现成本高允许降级为全量读入（已在步骤中显式允许并要求保持语义）；Task 8 history 格式解析以 oracle 源逐字复制为准；Task 11 create 重试错误分类需与渲染端 `isRetryableWorktreeCreateConflict` 对齐（实现时读 `src/renderer/src/store/slices/worktrees/create/create-worktree.ts:187-227`）。
