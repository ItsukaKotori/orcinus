# Phase 2D.2 创建 PR 全链路 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 打通 GitHub.com「push 分支 → 创建 PR」闭环：`git_read`/`git_push`/`gh_exec` stdin + `hostedReview.getCreationEligibility/create` 接真。

**Architecture:** Rust 只加白名单只读 `git_read`、`git_push`（argv 执行）与 `gh_exec` 的 stdin；TS 在 `src/renderer/src/lib/github/hosted-review-create.ts` 编排 eligibility/create（复用 2D.1 客户端与已移植共享助手），`real/git.ts`/`real/hosted-review.ts` 薄接线。参照实现 `/Users/itsuka/CodeSpace/orca`（只读），`orca:` 前缀均为其内路径。

**Tech Stack:** Rust（ade-bridge）、TypeScript（Vitest、Tauri IPC）、gh CLI + git（运行时）。

**Spec:** `docs/superpowers/specs/2026-10-08-phase2d2-create-pr-design.md`（执行者须同时阅读）

## Global Constraints

- 不新增任何 npm 依赖；Rust 不新增 crate 依赖。
- bindings 唯一生成方式：`cargo run -p ade-bridge --bin export-bindings`（workdir `src-tauri`）；`bindings_are_fresh` 校验；新命令名进 `specta_export.rs` 的 `collect_commands!` 与 `export_lists_every_command`。
- 提交信息中文 conventional + `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- cargo 命令在 `src-tauri` 下执行；pnpm 在仓库根。
- 仅 macOS/POSIX；可选字段省略不物化 null。
- 创建用户文案与结果码逐字对齐参照（`hosted-review-creation-blocking.ts` / `create-pr-error-classification.ts`），禁止改写文案。
- 渲染层 TS 模块可注入依赖、纯函数优先，禁止模块顶层触碰 `window`（bridge real 域除外）。
- 明确不做：stacked、fetch/pull/fast-forward、fork 物化、GHES 创建、非 GitHub provider、Windows/WSL（spec §2.2）。

---

### Task 1: Rust `git_read`（白名单只读）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/git.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Test: `src-tauri/crates/ade-bridge/tests/git_commands.rs`
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: `ade_git::runner::run_git_in`（`ade-git/src/runner.rs:40`）、`require_authorized_worktree`、`run_blocking`、既有 `TestDir`/`git()` 测试辅助。
- Produces: 命令 `git_read({worktreePath: string, args: string[]}) -> {stdout: string, stderr: string, code: number|null}`；`pub fn is_allowed_git_read_args(args: &[String]) -> bool`（导出供测试）。

- [ ] **Step 1: 写失败测试**

`tests/git_commands.rs` 追加（沿用该文件既有 `TestDir`/`git()`/`init_git_repo`）：

```rust
#[test]
fn git_read_whitelist_accepts_only_read_forms() {
    let ok = |args: &[&str]| {
        is_allowed_git_read_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    assert!(ok(&["rev-parse", "--abbrev-ref", "HEAD"]));
    assert!(ok(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]));
    assert!(ok(&["show-ref", "--verify", "--quiet", "refs/heads/main"]));
    assert!(ok(&["check-ref-format", "--branch", "feature/x"]));
    assert!(ok(&["config", "--get", "branch.main.remote"]));
    assert!(ok(&["config", "--get-all", "remote.origin.fetch"]));
    assert!(ok(&["config", "--get-regexp", "^branch\\."]));
    assert!(ok(&["config", "--list"]));
    // 写形式一律拒绝
    assert!(!ok(&["config", "user.name", "x"]));
    assert!(!ok(&["config", "--unset", "branch.main.remote"]));
    assert!(!ok(&["config", "--add", "remote.origin.fetch", "+refs/x"]));
    assert!(!ok(&["config", "--replace-all", "a", "b"]));
    assert!(!ok(&["config", "--edit"]));
    assert!(!ok(&["config", "--rename-section", "a", "b"]));
    assert!(!ok(&["config", "--remove-section", "a"]));
    // 非白名单子命令
    assert!(!ok(&["fetch", "--prune"]));
    assert!(!ok(&["status", "--porcelain"]));
    assert!(!ok(&["push", "origin", "HEAD"]));
    assert!(!ok(&[]));
}

#[test]
fn git_read_runs_real_reads_and_passes_through_nonzero() {
    let dir = TestDir::new("git-read");
    let repo = init_git_repo(&dir, "repo");
    let path = repo.to_str().unwrap();

    let args: Vec<String> = ["rev-parse", "--abbrev-ref", "HEAD"].iter().map(|s| s.to_string()).collect();
    let branch = git_read_impl(path, &args).unwrap();
    assert_eq!(branch.code, Some(0));
    assert!(!branch.stdout.trim().is_empty());

    let args: Vec<String> = ["show-ref", "--verify", "--quiet", "refs/heads/nope"].iter().map(|s| s.to_string()).collect();
    let missing = git_read_impl(path, &args).unwrap();
    assert_ne!(missing.code, Some(0));
    assert_eq!(missing.stdout, "");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run（workdir src-tauri）: `cargo test -p ade-bridge --test git_commands git_read`
Expected: 编译失败（函数不存在）。

- [ ] **Step 3: 实现**

`git.rs` 追加：

```rust
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitReadArgs {
    pub worktree_path: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitReadResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

const GIT_READ_ALLOWED_SUBCOMMANDS: &[&str] =
    &["config", "rev-parse", "symbolic-ref", "show-ref", "check-ref-format"];
const GIT_CONFIG_READ_FLAGS: &[&str] =
    &["--get", "--get-all", "--get-regexp", "--list"];
const GIT_CONFIG_WRITE_FLAGS: &[&str] = &[
    "--unset", "--unset-all", "--add", "--replace-all", "--edit",
    "--rename-section", "--remove-section", "--set",
];

/// 只读 git 命令白名单：首参必须受支持；`config` 仅允许读形式（含 `--get*`/`--list`，
/// 且拒绝任何写标志与裸写位置参数）。
pub fn is_allowed_git_read_args(args: &[String]) -> bool {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return false;
    };
    if !GIT_READ_ALLOWED_SUBCOMMANDS.contains(&subcommand) {
        return false;
    }
    if subcommand != "config" {
        return true;
    }
    if args.iter().any(|arg| {
        GIT_CONFIG_WRITE_FLAGS
            .iter()
            .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
    }) {
        return false;
    }
    let has_read_flag = args
        .iter()
        .any(|arg| GIT_CONFIG_READ_FLAGS.contains(&arg.as_str()));
    // 裸写形式：`config <key> <value>`（≥2 个非选项位置参数）且无读标志。
    let positional = args[1..].iter().filter(|arg| !arg.starts_with('-')).count();
    has_read_flag && positional <= 2
}

pub fn git_read_impl(worktree_path: &str, args: &[String]) -> Result<GitReadResult, BridgeError> {
    if !is_allowed_git_read_args(args) {
        return Err(BridgeError::message(format!(
            "git read rejected: {}",
            args.first().cloned().unwrap_or_default()
        )));
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = ade_git::runner::run_git_in(
        worktree_path,
        &borrowed,
        std::time::Duration::from_secs(120),
        None,
    )?;
    Ok(GitReadResult {
        stdout: output.stdout,
        stderr: output.stderr,
        code: output.status.code(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn git_read(
    state: State<'_, AppState>,
    args: GitReadArgs,
) -> Result<GitReadResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_read_impl(&args.worktree_path, &args.args)).await
}
```

注意 `run_git_in` 返回的 `GitOutput.status` 为 `std::process::ExitStatus`；`code()` 在信号终止时为 None。`specta_export.rs` 注册 `commands::git::git_read` + 名称清单。

- [ ] **Step 4: 跑测试 + bindings**

Run: `cargo test -p ade-bridge --test git_commands git_read`
Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/ade-bridge/src/commands/git.rs \
  src-tauri/crates/ade-bridge/src/specta_export.rs \
  src-tauri/crates/ade-bridge/tests/git_commands.rs \
  src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): git_read 白名单只读命令

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Rust `gh_exec` stdin 扩展

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/gh.rs`
- Test: `src-tauri/crates/ade-bridge/tests/gh_exec.rs`
- Regenerate: bindings

**Interfaces:**
- Consumes: Task 1 无关；既有 `gh_exec_impl`/`GhExecArgs`/fake-gh 测试辅助。
- Produces: `GhExecArgs.stdin: Option<String>`；`gh_exec_impl` 签名增加 `stdin: Option<&str>`。

- [ ] **Step 1: 写失败测试**

`tests/gh_exec.rs` 追加（沿用既有 `TestDir`/`env_lock`）：

```rust
#[test]
fn gh_exec_pipes_stdin_to_child() {
    let _env = env_lock();
    let dir = TestDir::new("stdin");
    let gh = dir.write_executable("gh", "#!/bin/sh\ncat\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, Some("body text")).unwrap();
    assert_eq!(result.stdout, "body text");
    assert_eq!(result.code, Some(0));
}

#[test]
fn gh_exec_without_stdin_leaves_child_stdin_closed() {
    let _env = env_lock();
    let dir = TestDir::new("no-stdin");
    let gh = dir.write_executable("gh", "#!/bin/sh\ncat\n");
    let result = gh_exec_impl(&gh, &[], None, Duration::from_secs(5), 1024, None).unwrap();
    assert_eq!(result.stdout, "");
}
```

（既有 9 条测试的调用需同步加 `None` 参数。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p ade-bridge --test gh_exec stdin`
Expected: 编译失败。

- [ ] **Step 3: 实现**

`gh.rs`：

```rust
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
    #[serde(default)]
    pub stdin: Option<String>,
}
```

`gh_exec_impl` 增加末参 `stdin: Option<&str>`：spawn 时 `command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })`；spawn 后：

```rust
if let (Some(payload), Some(mut pipe)) = (stdin, child.stdin.take()) {
    std::thread::spawn(move || {
        use std::io::Write;
        let _ = pipe.write_all(payload.as_bytes());
        // pipe 落域即关闭 stdin，子进程 cat 正常退出
    });
}
```

命令包装传入 `args.stdin.as_deref()`。既有 9 条测试调用补 `None`。

- [ ] **Step 4: 跑测试 + bindings**

Run: `cargo test -p ade-bridge --test gh_exec`
Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/ade-bridge/src/commands/gh.rs \
  src-tauri/crates/ade-bridge/tests/gh_exec.rs \
  src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): gh_exec 支持 stdin（--body-file - 语义）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: Rust `git_push`

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/git.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Test: `src-tauri/crates/ade-bridge/tests/git_commands.rs`
- Regenerate: bindings

**Interfaces:**
- Produces: 命令 `git_push({worktreePath, remote?, refspec?, forceWithLease?}) -> null`；`pub fn is_safe_remote_name(name: &str) -> bool`、`pub fn push_args(remote: Option<&str>, refspec: Option<&str>, force_with_lease: bool) -> Vec<String>`（导出供测试）。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn push_args_builds_expected_argv() {
    assert_eq!(
        push_args(Some("origin"), Some("HEAD:feature"), false),
        vec!["push", "--set-upstream", "origin", "HEAD:feature"]
    );
    assert_eq!(
        push_args(Some("fork"), Some("HEAD:feature"), true),
        vec!["push", "--force-with-lease", "--set-upstream", "fork", "HEAD:feature"]
    );
    assert_eq!(
        push_args(None, None, false),
        vec!["push", "--set-upstream", "origin", "HEAD"]
    );
}

#[test]
fn push_rejects_unsafe_remote_and_refspec() {
    let dir = TestDir::new("push-validate");
    let repo = init_git_repo(&dir, "repo");
    let path = repo.to_str().unwrap();
    assert!(git_push_impl(path, Some("bad remote"), Some("HEAD:x"), false).is_err());
    assert!(git_push_impl(path, Some("origin"), Some("-danger"), false).is_err());
    assert!(git_push_impl(path, Some("origin"), Some(""), false).is_err());
}

#[test]
fn push_sets_upstream_on_local_bare_remote() {
    let dir = TestDir::new("push-real");
    let repo = init_git_repo(&dir, "repo");
    let bare = dir.file("origin.git");
    std::process::Command::new("git").args(["init", "--bare"]).arg(&bare).output().unwrap();
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    let path = repo.to_str().unwrap();
    git_push_impl(path, Some("origin"), Some("HEAD:feature"), false).unwrap();
    let upstream = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "--abbrev-ref", "HEAD@{upstream}"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&upstream.stdout).trim(), "origin/feature");
}
```

（`git()` 辅助按该文件现有签名使用；`init_git_repo` 需产生至少一个提交——按现有辅助行为调整。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p ade-bridge --test git_commands push`
Expected: 编译失败。

- [ ] **Step 3: 实现**

```rust
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitPushArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub refspec: Option<String>,
    #[serde(default)]
    pub force_with_lease: bool,
}

/// 安全 remote 名：1–100、按 `/` 分段每段非空且非 `.`/`..`、段首字母数字。
pub fn is_safe_remote_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 100 {
        return false;
    }
    name.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphanumeric())
            && segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    })
}

pub fn push_args(remote: Option<&str>, refspec: Option<&str>, force_with_lease: bool) -> Vec<String> {
    let mut args = vec!["push".to_string()];
    if force_with_lease {
        args.push("--force-with-lease".to_string());
    }
    args.push("--set-upstream".to_string());
    match (remote, refspec) {
        (Some(remote), Some(refspec)) => {
            args.push(remote.to_string());
            args.push(refspec.to_string());
        }
        _ => {
            args.push("origin".to_string());
            args.push("HEAD".to_string());
        }
    }
    args
}

pub fn git_push_impl(
    worktree_path: &str,
    remote: Option<&str>,
    refspec: Option<&str>,
    force_with_lease: bool,
) -> Result<(), BridgeError> {
    if let Some(remote) = remote {
        if !is_safe_remote_name(remote) {
            return Err(BridgeError::message("git push rejected: unsafe remote name"));
        }
    }
    if let Some(refspec) = refspec {
        if refspec.is_empty() || refspec.starts_with('-') {
            return Err(BridgeError::message("git push rejected: unsafe refspec"));
        }
    }
    let args = push_args(remote, refspec, force_with_lease);
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = ade_git::runner::run_git_in(
        worktree_path,
        &borrowed,
        std::time::Duration::from_secs(120),
        None,
    )?;
    if output.status.success() {
        return Ok(());
    }
    Err(BridgeError::message(output.stderr.trim().to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn git_push(state: State<'_, AppState>, args: GitPushArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || {
        git_push_impl(
            &args.worktree_path,
            args.remote.as_deref(),
            args.refspec.as_deref(),
            args.force_with_lease,
        )
    })
    .await
}
```

`specta_export.rs` 注册 + 名称清单。

- [ ] **Step 4: 跑测试 + bindings**

Run: `cargo test -p ade-bridge --test git_commands push`
Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/ade-bridge/src/commands/git.rs \
  src-tauri/crates/ade-bridge/src/specta_export.rs \
  src-tauri/crates/ade-bridge/tests/git_commands.rs \
  src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): git_push 命令（argv 执行 + 防御校验）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: TS `git-read-client` + `real/git.ts` push 接线

**Files:**
- Create: `src/renderer/src/lib/github/git-read-client.ts`
- Create: `src/renderer/src/lib/github/git-read-client.test.ts`
- Modify: `src/bridge/real/git.ts`
- Test: `src/bridge/real/git.test.ts`

**Interfaces:**
- Consumes: Task 1 `git_read`、Task 3 `git_push`；共享 `assertGitPushTargetShape`（`src/shared/git-push-target-validation.ts`）、`resolveConfiguredGitPushTarget`（`src/shared/git-push-target-resolution.ts`）；`invokeCommand`。
- Produces:
  - `createRunGit(executor: GitReadExecutor): (args: string[]) => Promise<{stdout: string}>`（非零抛 `GitReadError`）
  - `class GitReadError extends Error { code: number|null; stderr: string }`
  - `defaultGitReadExecutor(worktreePath: string): GitReadExecutor`

- [ ] **Step 1: 写失败测试**

`git-read-client.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import { createRunGit, GitReadError } from './git-read-client'

describe('git read client', () => {
  it('returns stdout on zero exit and throws GitReadError otherwise', async () => {
    const executor = vi.fn(async (args: string[]) =>
      args[0] === 'rev-parse'
        ? { stdout: 'feature\n', stderr: '', code: 0 }
        : { stdout: '', stderr: 'fatal: bad', code: 128 }
    )
    const runGit = createRunGit(executor)
    await expect(runGit(['rev-parse', '--abbrev-ref', 'HEAD'])).resolves.toEqual({
      stdout: 'feature\n'
    })
    await expect(runGit(['show-ref', 'x'])).rejects.toMatchObject({
      name: 'GitReadError',
      code: 128,
      stderr: 'fatal: bad'
    })
  })
})
```

`real/git.test.ts` 追加（模式照该文件既有 invoke mock）：

```ts
it('resolves a configured push target and calls git_push', async () => {
  invokeMock.mockImplementation(async (command: string, payload?: unknown) => {
    if (command === 'git_read') {
      const args = (payload as { args: { args: string[] } }).args.args
      if (args[0] === 'symbolic-ref') return { stdout: 'feature\n', stderr: '', code: 0 }
      if (args[0] === 'config') return { stdout: '', stderr: '', code: 1 }
      throw new Error(`unexpected git_read ${args.join(' ')}`)
    }
    if (command === 'git_push') return null
    throw new Error(`unexpected ${command}`)
  })
  const api = createGitRealApi()
  await api.push({ worktreePath: '/repo', publish: false } as never)
  expect(invokeMock).toHaveBeenCalledWith('git_push', {
    args: { worktreePath: '/repo', remote: 'origin', refspec: 'HEAD', forceWithLease: false }
  })
})

it('validates an explicit push target before pushing', async () => {
  invokeMock.mockImplementation(async (command: string) => {
    if (command === 'git_read') return { stdout: 'refs/heads/feature\n', stderr: '', code: 0 }
    if (command === 'git_push') return null
    throw new Error(`unexpected ${command}`)
  })
  const api = createGitRealApi()
  await api.push({
    worktreePath: '/repo',
    publish: false,
    pushTarget: { remoteName: 'fork', branchName: 'feature' }
  } as never)
  expect(invokeMock).toHaveBeenCalledWith('git_push', {
    args: { worktreePath: '/repo', remote: 'fork', refspec: 'HEAD:feature', forceWithLease: false }
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github/git-read-client.test.ts src/bridge/real/git.test.ts`

- [ ] **Step 3: 实现**

`git-read-client.ts`：

```ts
import { invokeCommand } from '../../../bridge/real/invoke'

export type GitReadResult = { stdout: string; stderr: string; code: number | null }
export type GitReadExecutor = (args: string[]) => Promise<GitReadResult>

export class GitReadError extends Error {
  readonly code: number | null
  readonly stderr: string
  constructor(result: GitReadResult) {
    super(result.stderr.trim() || `git read exited with code ${result.code ?? 'unknown'}`)
    this.name = 'GitReadError'
    this.code = result.code
    this.stderr = result.stderr
  }
}

export function createRunGit(executor: GitReadExecutor): (args: string[]) => Promise<{ stdout: string }> {
  return async (args) => {
    const result = await executor(args)
    if (result.code !== 0) {
      throw new GitReadError(result)
    }
    return { stdout: result.stdout }
  }
}

export function defaultGitReadExecutor(worktreePath: string): GitReadExecutor {
  return (args) => invokeCommand<GitReadResult>('git_read', { args: { worktreePath, args } })
}
```

`real/git.ts` `push` 实现（模块内构造依赖；`pushTarget` 类型见 `PreloadApi['git']['push']` 参数）：

```ts
push: async (args) => {
  const runGit = createRunGit(defaultGitReadExecutor(args.worktreePath))
  const forceWithLease = args.forceWithLease === true
  let remote: string | null = null
  let refspec: string | null = null
  if (args.pushTarget) {
    if (args.pushTarget.remoteUrl) {
      throw new Error('Push targets with a remote URL are not supported yet.')
    }
    assertGitPushTargetShape(args.pushTarget)
    await runGit(['check-ref-format', '--branch', args.pushTarget.branchName])
    remote = args.pushTarget.remoteName
    refspec = `HEAD:${args.pushTarget.branchName}`
  } else {
    const resolved = await resolveConfiguredGitPushTarget(runGit)
    if (resolved) {
      remote = resolved.remote
      refspec = resolved.refspec
    }
  }
  await invokeCommand('git_push', {
    args: { worktreePath: args.worktreePath, remote, refspec, forceWithLease }
  })
},
```

（`git_push` 参数缺省时 Rust 走 `origin HEAD`；TS 传 `remote/refspec` 为 `null` 时需省略或传 `null`——按 serde `Option` 语义传 `null` 可反序列化为 `None`，保持 `null` 即可。）

- [ ] **Step 4: 跑测试 + typecheck**

Run: `pnpm vitest run src/renderer/src/lib/github/git-read-client.test.ts src/bridge/real/git.test.ts && pnpm typecheck`

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/lib/github/git-read-client.ts \
  src/renderer/src/lib/github/git-read-client.test.ts \
  src/bridge/real/git.ts src/bridge/real/git.test.ts
git commit -m "feat(renderer): git_read 客户端与 git.push 目标解析接线

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: TS eligibility（`hosted-review-create.ts` 第一段）

**Files:**
- Create: `src/renderer/src/lib/github/hosted-review-create.ts`
- Create: `src/renderer/src/lib/github/hosted-review-create.test.ts`

**Interfaces:**
- Consumes: Task 4 `createRunGit`/`GitReadExecutor`；2D.1 `createHostedReviewClient`（forBranch 缓存查询）、`createRepoIdentityResolver`、`createGhExecClient`、`parseAuthStatus`；`git_creation_state` 等读取经 `runGit`；`git_status`/`git_upstream_status`（经注入的 `readStatus`/`readUpstream`）。
- Produces（本任务与 Task 6 共用工厂）：
  - `createHostedReviewCreation(deps): { getCreationEligibility(args): Promise<HostedReviewCreationEligibility>; create(args): Promise<CreateHostedReviewResult> }`
  - `deps = { client, identity, reviewLookup, makeRunGit: (worktreePath: string) => RunGit, readStatus, readUpstream, readTemplate?, now? }`（`makeRunGit` 每 worktree 构造一次；`RunGit = (args: string[]) => Promise<{stdout: string}>`）
  - `getDefaultBaseRef(runGit): Promise<string|null>`、`baseRefExistsOnRemote(runGit, base): Promise<boolean>`（导出供测试）

- [ ] **Step 1: 写失败测试**

覆盖（fake deps 注入；文案断言逐字）：

1. blockers 全序（12 条，逐条构造最小输入断言 `blockedReason/nextAction/canCreate`）：detached（branch `HEAD`）→ existing（reviewLookup 返回 PR）→ unsupported（identity null 或非默认 host）→ default_branch → dirty → no_upstream → hasUpstream undefined（`blockedReason:null, canCreate:false`）→ needs_sync（behind>0）→ auth_required（auth 查询失败）→ needs_push（ahead>0）→ base_not_on_remote（`enforceBaseOnRemote` + candidate 不在远端）→ 全通过。
2. `getDefaultBaseRef`：`symbolic-ref` 命中 → 该值；`symbolic-ref` 非零 + `refs/remotes/origin/main` 命中 → `origin/main`；全 miss → null。断言 argv 序列（`symbolic-ref --quiet refs/remotes/origin/HEAD`、`rev-parse --verify --quiet <ref>`）。
3. `baseRefExistsOnRemote`：归一化（`origin/main`→`main`）；不安全 ref → false 且不调 runGit；`show-ref --verify --quiet refs/remotes/origin/main` 命中 → true；全 miss 后后缀扫描 `show-ref -- main` 解析 `refs/remotes/origin/main` → true；runGit 抛非 GitReadError 异常 → true（fail-open）。
4. `defaultBaseRef` 语义：candidate 在远端 → 原样；不在 → default ?? candidate。
5. auth：`client.run(['auth','status','--hostname','github.com'])` 非零但 stdout 有 active → 通过；spawn 失败 → `auth_required`。
6. `reviewLookup` 抛错 → `reviewLookupOutcome:'unavailable'` 且 `canCreate:false`。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github/hosted-review-create.test.ts`

- [ ] **Step 3: 实现**

要点（决策序与文案逐字对齐参照 `orca:src/main/source-control/hosted-review-creation.ts:161-238` 与 `hosted-review-creation-blocking.ts`）：

- `getDefaultBaseRef`：移植 `orca:src/main/git/repo-default-base-ref.ts:21-26,95-115`——`symbolic-ref --quiet refs/remotes/origin/HEAD` 后用 `rev-parse --verify --quiet <value>` 校验；否则按序 `refs/remotes/origin/main`、`refs/remotes/origin/master`、`refs/heads/main`、`refs/heads/master` 探测；全 miss → null。
- `baseRefExistsOnRemote`：移植 `orca:src/main/source-control/hosted-review-creation-git-state.ts:185-249`——归一化（剥 `refs/heads/`、`refs/remotes/<r>/`、`origin/`、`upstream/`）；`isSafeGitRefName`（端口已有？若无则用现有 `shared/git-push-target-validation.ts` 风格的安全字符校验；执行者按参照 `isSafeGitRefName` 语义实现）不通过 → false；精确探测 `refs/remotes/<base>`（base 含 `/`）与 `refs/remotes/origin/<base>`、`refs/remotes/upstream/<base>`；后缀扫描 `show-ref -- <base>`，仅接受 `refs/remotes/<单段>/<base>`；`GitReadError`（确定无匹配）→ false，其它异常 → true。
- provider：`identity.getRepoSlug(worktreePath)`；null 或 `host` 非默认（非 `github.com`/空）→ `unsupported_provider`。
- 已有 review：`reviewLookup.forBranch({repoPath: worktreePath, branch, currentHeadOid: args.currentHeadOid ?? null, active: true, linkedGitHubPR: args.linkedGitHubPR ?? null, fallbackGitHubPR: args.fallbackGitHubPR ?? null})`；抛错 → `lookupFailed`；返回 null → `not_found`；有值 → `found`（review 取 `{number, url}`）。
- blockers 顺序与返回字段按 spec §3.3 逐条；`defaultBaseRef` 为保留原样的 candidate 或探测值。
- auth：`client.run(['auth','status','--hostname','github.com'])`（结果非零也解析 `stdout+stderr`）；`parseAuthStatus` 存在 active → 通过；spawn 类失败（`gh: command not found`）→ 不通过。

- [ ] **Step 4: 跑测试 + typecheck**

Run: `pnpm vitest run src/renderer/src/lib/github/hosted-review-create.test.ts && pnpm typecheck`

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/lib/github/hosted-review-create.ts \
  src/renderer/src/lib/github/hosted-review-create.test.ts
git commit -m "feat(renderer): PR 创建 eligibility（blockers/base/auth）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: TS create（preflight/模板/argv/回退/分类/缓存失效）

**Files:**
- Modify: `src/renderer/src/lib/github/hosted-review-create.ts`
- Modify: `src/renderer/src/lib/github/hosted-review-create.test.ts`
- Modify: `src/renderer/src/lib/github/hosted-review.ts`（导出 `invalidate`）

**Interfaces:**
- Consumes: Task 5 工厂；`gh_exec` stdin（Task 2）；模板经注入的 `readTemplate`（bridge 侧用 `fs_read_file`）。
- Produces: `create(args: CreateHostedReviewArgs): Promise<CreateHostedReviewResult>`；`hosted-review.ts` 客户端新增 `invalidate(repoPath: string): void`。

- [ ] **Step 1: 写失败测试**

覆盖：

1. preflight：`currentBranch !== head` → `validation` + 参照文案（"switch back to the selected branch…" 逐字）；`readStatus` 非空 entries → `dirty` 文案；`readUpstream` 无上游 → `no_upstream` 文案；eligibility `unavailable` → `validation`（"could not confirm whether this branch already has…" 逐字）。
2. argv：`gh pr create --repo org/repo --base main --title T --body-file -`；有 head → 追加 `--head feature`；draft → `--draft`；`stdin` = body。
3. 模板：`useTemplate && !body` → `readTemplate()` 命中路径内容进 stdin；`readTemplate` 返回 null → 空串。
4. 解析：stdout JSON → ok；stdout URL 文本 → ok（任意 host 正则）；不可解析 + `gh pr list` 恰好 1 条 → ok；0/2 条 → `unknown_completion` 文案逐字。
5. 失败分类表逐条（auth/`already exists`/timeout→`unknown_completion`/`validation failed`/其它→`unknown`）；`already_exists` 带 existingReview（来自回退查询）。
6. 成功 → `reviewLookup.invalidate` 被调用一次。
7. blockers 映射：blockedReason→结果码/文案逐字表（auth_required/unsupported_provider/dirty/detached_head/default_branch/no_upstream/needs_push/needs_sync/fork_head_unsupported/base_not_on_remote/空 blocker）。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/lib/github/hosted-review-create.test.ts`

- [ ] **Step 3: 实现**

- `create` 流程按 spec §3.3 与参照 `hosted-review-creation.ts:44-287`；错误分类移植 `orca:src/main/github/client/create/create-pr-error-classification.ts:3-74`（顺序与文案逐字；`already_exists`/`unknown_completion` 带 head 时执行回退查询）；blockers 映射移植 `hosted-review-creation-blocking.ts:9-102` 文案表。
- `gh pr list` 回退 argv 逐字：`['pr','list','--repo',`${owner}/${repo}`,'--head',head,'--base',base,'--state','open','--limit','2','--json','number,url']`（恰好 1 条才接受）。
- 成功路径：`reviewLookup.invalidate(args.repoPath)`（2D.1 客户端导出：清空该 repoPath 前缀的 TTL 条目）。
- 模板路径序（`readTemplate` 由 bridge 注入）：`.github/pull_request_template.md`、`.github/PULL_REQUEST_TEMPLATE.md`、`pull_request_template.md`、`PULL_REQUEST_TEMPLATE.md`、`docs/pull_request_template.md`、`docs/PULL_REQUEST_TEMPLATE.md`。

- [ ] **Step 4: 跑测试 + typecheck**

Run: `pnpm vitest run src/renderer/src/lib/github && pnpm typecheck`

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/lib/github/hosted-review-create.ts \
  src/renderer/src/lib/github/hosted-review-create.test.ts \
  src/renderer/src/lib/github/hosted-review.ts
git commit -m "feat(renderer): PR 创建执行（preflight/模板/回退/分类）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: `real/hosted-review.ts` creation 接线 + parity/create-api + 全量

**Files:**
- Modify: `src/bridge/real/hosted-review.ts`
- Modify: `src/bridge/real/hosted-review.test.ts`
- Modify: `src/bridge/real/parity.test.ts`
- Modify: `src/bridge/create-api.test.ts`

**Interfaces:**
- Consumes: Task 5/6 工厂、Task 4 `defaultGitReadExecutor`、2D.1 `createHostedReviewClient`（`forBranch` + 新 `invalidate`）、`createRepoIdentityResolver`、`defaultGhExecutor`、`invokeCommand('fs_read_file')`、`invokeCommand('git_status')`、`invokeCommand('git_upstream_status')`。
- Produces: `getCreationEligibility`/`create` 真实路由；`createStacked` 维持 fallback。

- [ ] **Step 1: 写失败测试**

`real/hosted-review.test.ts` 追加：mock invoke——
- `git_read`（symbolic-ref/config/check-ref-format/show-ref 分支）→ `getCreationEligibility` 返回 `canCreate:true`（clean、upstream、ahead 0）；
- `gh_exec`（auth status 有 active）→ 通过；无 → `auth_required`；
- `create`：`gh_exec` 返回 `{"number":7,"url":"https://github.com/o/r/pull/7"}` → `{ok:true,number:7,url}`；`createStacked` 仍 reject unimplemented。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/bridge/real/hosted-review.test.ts`

- [ ] **Step 3: 实现**

`real/hosted-review.ts` 构造：

```ts
const client = createGhExecClient(defaultGhExecutor())
const identity = createRepoIdentityResolver({ client, readRemoteUrls })
const reviewLookup = createHostedReviewClient({ identity, lookup }) // lookup = 2D.1 PR lookup（同 2D.1 装配）
const creation = createHostedReviewCreation({
  client,
  identity,
  reviewLookup,
  makeRunGit: (worktreePath) => createRunGit(defaultGitReadExecutor(worktreePath)),
  readStatus: (worktreePath) => invokeCommand('git_status', { args: { worktreePath } }),
  readUpstream: (worktreePath) => invokeCommand('git_upstream_status', { args: { worktreePath } }),
  readTemplate: (worktreePath, relativePath) => invokeCommand('fs_read_file', { args: { filePath: `${worktreePath}/${relativePath}` } })
})
```

`parity.test.ts`：hostedReview `explicit` 增加 `getCreationEligibility`、`create`（`forBranch` 已在），`missing` 仅剩 `createStacked`；`create-api.test.ts` 更新对应断言。

- [ ] **Step 4: 跑测试 + typecheck + 全量**

Run: `pnpm vitest run src/bridge && pnpm typecheck`
Run（提交前）: `pnpm test`（全量；性能类抖动隔离复跑；其它失败 STOP + BLOCKED）

- [ ] **Step 5: 提交**

```bash
git add src/bridge/real/hosted-review.ts src/bridge/real/hosted-review.test.ts \
  src/bridge/real/parity.test.ts src/bridge/create-api.test.ts
git commit -m "feat(bridge): hostedReview 创建面接线（eligibility/create）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: 门禁复跑 + 收尾记录

**Files:**
- Create: `docs/phase2d2-create-pr-record.md`
- Modify: `docs/superpowers/plans/2026-10-08-phase2d2-create-pr.md`（勾选复选框）

- [ ] **Step 1: 全量门禁**

Run: `cargo test --workspace`（workdir src-tauri）
Run: `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`
Run: `pnpm test`（性能类失败隔离复跑记 flake；其它失败 STOP + BLOCKED）

- [ ] **Step 2: 写收尾记录**

结构照 `docs/phase2d1-github-readonly-record.md`：§1 范围与验收 / §2 提交清单 / §3 门禁证据 / §4 手工验收（6 项，待用户复核）/ §5 偏差与边界备案（spec §7 九条 + 实现中发现的新偏差）/ §6 已知边界与后续（stacked 2D.2.1、fetch/pull/fast-forward、fork 物化、resolvePrBase、非 GitHub provider、PR 刷新协调器）。

手工验收清单（写入记录）：
1. 新分支未推送 → 显示 needs_push → 推送成功 → 创建 PR 成功（GitHub 可见）；
2. 草稿勾选 → PR 为 Draft；
3. useTemplate 且正文空 → 正文来自仓库模板；
4. 已有 PR 分支 → existing_review + 链接；
5. dirty → dirty blocker；主分支 → default_branch；
6. 无 gh 登录 → auth_required。

- [ ] **Step 3: 提交**

```bash
git add docs/phase2d2-create-pr-record.md docs/superpowers/plans/2026-10-08-phase2d2-create-pr.md
git commit -m "docs: Phase 2D.2 实施记录与门禁证据（创建 PR 全链路）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- §3.1 `git_read`/`git_push`/`gh_exec stdin` → Tasks 1–3。
- §3.2 TS git 面 → Task 4。
- §3.3 eligibility/create → Tasks 5–6。
- §2.1 桥接接线 → Tasks 4/7；§4 数据流 → 各任务测试 + Task 7 路由测试。
- §5 错误处理 → Task 6 分类/映射测试；§6 测试与门禁 → 各任务 + Task 8；§7 风险 → Task 8 记录。

**Placeholder scan:** Rust 全部代码在计划内；TS 逐字移植步骤附参照 file:line 与关键常量/argv/文案表；Task 7 的 `runGit` 工厂参数名留待实现时定名并已在步骤中说明（避免过度锁定内部形状）。

**Type consistency:** 命令名 `git_read`/`git_push` 在 Tasks 1/3/4/7 一致；`GitReadResult`/`GitReadError`/`createRunGit`/`defaultGitReadExecutor`（Task 4）被 Task 7 一致引用；`createHostedReviewCreation`（Tasks 5/6/7）一致；`HostedReviewCreationEligibility`/`CreateHostedReviewResult`/`CreateHostedReviewErrorCode` 均为端口已有类型；`pushTarget` 字段名与 `GitPushTarget` 一致。
