# 原地 Checkout 后端（增量 2A）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 ade 加一条 `git_checkout` tauri 命令，支持「在当前工作区原地检出一个本地分支」和「检出远程分支并建同名本地跟踪分支」，并把 git 的两类拒绝（分支被其它 worktree 占用、会覆盖本地改动）以**结构化软错误**回传前端。

**Architecture:** 新模块 `ade_git::branch_ops` 承载目标解析与前置复核，产出 `GitCheckoutOutcome { status, branch?, refusal?, message? }` 并以 `Ok(...)` 返回；`ade-bridge` 的 `git_checkout` 命令只做鉴权 + 阻塞调度 + 透传。前端在 2B 才消费它，本增量只交付到 `window.api.git.checkout` 与其 runtime 包装。

**Tech Stack:** Rust（`ade-git` / `ade-bridge` + tauri-specta 绑定再生）、TypeScript、Vitest、`cargo test`。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` §13.3（F1/F2/F3）、§13.4、§13.5、§3.5（7 面镜像）、§3.6、§3.7、§8.1（含 BridgeError 不可承载类型化拒绝的更正）。

## Global Constraints

- **拒绝走软错误，不走 `BridgeError`。** `BridgeError` 的 `Serialize`（`ade-bridge/src/errors.rs:30-36`）把所有变体压成 `{ message: string }`，类型信息全丢。仓库先例是 `GitBranchCompareSummary { status: String, error_message: Option<String> }` 以 `Ok(...)` 返回（`ade-git/src/compare.rs:49-63`）。本命令用 **serde enum**（比先例的 `String` 更强，TS 拿到可判别联合），但**位置**沿用软错误。
- **只接受真实存在的 ref。** 命令层自己解析 `refs/heads` / `refs/remotes`，**不接受**调用方拼装的任意 revision 字符串 —— 否则等于开一个 `git checkout <任意串>` 的注入面，也会踩 F2 那个「远程名进 detached」的坑。
- **本期不提供 tag / commit 的 detached checkout**（spec §13.5）。目标解析到非分支即 `InvalidTarget`。
- **Rust 侧不写 workspace 元数据。** `Worktree.branch` 的跟刷由既有链路负责：`git-status-refresh.ts:147-153` 已在做 `updateWorktreeGitIdentity(worktreeId, { head, branch: status.branch ?? null })`，其注释原话就是 "branch switches can happen inside a terminal"。本增量**不新增同步代码**。
- **只做本地路径。** `runtime` / `runtimeEnvironments` 无 real 实现（spec §3.6），故 `git_checkout` 只加 tauri 命令 + `window.api.git.checkout`，**不加** `callRuntimeRpc` 分支。
- 新命令必须过 §3.5 的 **7 面镜像**，否则 `specta_export::tests::bindings_are_fresh` 与 `command_inventory_and_bindings_are_fresh` 会红。
- 仓库**没有 pre-commit 钩子**，门禁全靠手动执行：`pnpm typecheck` + 定向 `pnpm vitest run` + `cargo test`。
- `git` 版本前提：本命令只用 `checkout` / `worktree list --porcelain` / `rev-parse --symbolic-full-name`，**不依赖** `%(HEAD)` 那类新 atom，故不受 spec §3.7 的 git ≥ 2.24 约束影响。

## 已实测的行为前提（不是推测）

| # | 事实 | 实测 |
|---|---|---|
| F1 | 分支被**其它 worktree** 检出时 `git checkout feat` → `fatal: 'feat' is already used by worktree at '<path>'`，非零退出 | 建 worktree 后 checkout 该分支 |
| F2 | `git checkout origin/main` → `Note: switching to 'origin/main'. You are in 'detached HEAD' state`，**不**建本地跟踪分支 | 实跑，`branch --show-current` 为空 |
| F3 | 脏工作区 checkout 到不冲突分支 → `Switched to branch 'other'` + `M f1`，改动被带过去 | 实跑 |

F1/F2 是本命令必须自己处理的两件事；F3 交给 git（前端在 2B 先拦确认）。

## 文件结构

**新建**

| 文件 | 职责 |
|---|---|
| `src-tauri/crates/ade-git/src/branch_ops.rs` | `GitCheckoutTarget` / `GitCheckoutStatus` / `GitCheckoutRefusal` / `GitCheckoutOutcome` + `resolve_checkout_target()` + `checkout()` |
| `src-tauri/crates/ade-git/tests/branch_checkout.rs` | 真 git 仓集成测试（F1、F2、正常检出、非法目标、detached 拒绝） |

**修改**

| 文件 | 改动 |
|---|---|
| `src-tauri/crates/ade-git/src/lib.rs` | `pub mod branch_ops;` + `pub use` 四个类型 |
| `src-tauri/crates/ade-bridge/src/commands/git.rs` | `GitCheckoutArgs` + `git_checkout_impl` + `#[tauri::command] git_checkout` |
| `src-tauri/crates/ade-bridge/src/specta_export.rs` | `collect_commands!` 注册 `commands::git::git_checkout` |
| `src/bridge/real/generated/tauri-bindings.ts` | `cargo run -p ade-bridge --bin export-bindings` 再生 |
| `src/bridge/real/git.ts` | real 映射加 `checkout: (args) => invokeCommand('git_checkout', { args })` |
| `src/shared/preload-api/api/git-operation-api.ts` | `checkout: (args) => Promise<GitCheckoutOutcome>` 类型 |
| `src/shared/git-checkout-types.ts` | `GitCheckoutOutcome` 等的 TS 手抄镜像（对齐 `git-ref-types.ts` 的做法） |
| `src/renderer/src/runtime/runtime-git-client.ts` + 新 `runtime-git-checkout-client.ts` | `checkoutRuntimeGit(context, target)`：只走本地 `window.api.git.checkout` |
| `src-tauri/crates/ade-bridge/tests/git_checkout.rs` | bridge 层契约测试 |

> `src/bridge/mock/git-api.ts` **不加** `checkout`：`withMethodFallback` 会兜成 unimplemented，与 `refs`/`pull` 的既有处理一致（spec §3.5 第 7 面）。渲染测试自行桩。

---

## Task 1: `ade_git::branch_ops` —— 目标解析、F1 复核、软错误结果

**Files:**
- Create: `src-tauri/crates/ade-git/src/branch_ops.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`
- Test: `src-tauri/crates/ade-git/tests/branch_checkout.rs`

**Interfaces:**
- Consumes: `crate::command::{run, run_text}`（`run_text(worktree_path: &str, args: &[&str]) -> Result<String, CoreError>`，`pub(crate)`）；`crate::worktree_list(path) -> Result<Vec<GitWorktreeEntry>, CoreError>`，`GitWorktreeEntry { path, head, branch: Option<String>, is_bare, is_main_worktree }`；`ade_core::errors::CoreError::{InvalidInput, GitCommandFailed}`
- Produces:
  - `GitCheckoutTarget`（Deserialize + `specta::Type`，serde camelCase）：`{ kind: 'local' | 'remote', name: String }`
  - `GitCheckoutStatus`（Serialize，serde `rename_all = "kebab-case"`）：`Switched | AlreadyOnBranch | Refused { .. } | InvalidTarget`
  - `GitCheckoutRefusal`（Serialize，kebab-case）：`CheckedOutElsewhere`
  - `GitCheckoutOutcome { status: GitCheckoutStatus, branch: Option<String>, worktree_path: Option<String>, message: Option<String> }`（camelCase，Option 全带 `skip_serializing_if`）
  - `ade_git::branch_ops::checkout(worktree_path: &str, target: &GitCheckoutTarget) -> Result<GitCheckoutOutcome, CoreError>`

- [ ] **Step 1: 写失败测试 —— 正常检出本地分支**

Create `src-tauri/crates/ade-git/tests/branch_checkout.rs`。文件头部的 `git()` / `commit()` / `repo()` helper 逐字照抄 `src-tauri/crates/ade-git/tests/history_query.rs` 的同名 helper（那套已验证可用：`GIT_CONFIG_GLOBAL=/dev/null`、`-c commit.gpgsign=false -c core.hooksPath= -c init.templateDir=`、`repo()` 建 `main` + `feature` 且当前在 `main`），只把 import 换成本模块的：

```rust
use ade_git::branch_ops::{checkout, GitCheckoutStatus, GitCheckoutTarget};

// 正常路径：本地分支存在且未被别的 worktree 占用 → Switched，HEAD 真的动了。
#[test]
fn checkout_local_branch_switches_head() {
    let d = repo();
    let p = d.path().to_str().unwrap();

    let outcome = checkout(p, &GitCheckoutTarget { kind: "local".into(), name: "feature".into() }).unwrap();
    assert!(matches!(outcome.status, GitCheckoutStatus::Switched), "{:?}", outcome.status);
    assert_eq!(outcome.branch.as_deref(), Some("feature"));

    let current = String::from_utf8(
        Command::new("git").arg("-C").arg(p).args(["branch", "--show-current"]).output().unwrap().stdout,
    )
    .unwrap();
    assert_eq!(current.trim(), "feature");
}
```

- [ ] **Step 2: 跑测试确认编译失败**

Run: `cd src-tauri && cargo test -p ade-git --test branch_checkout 2>&1 | tail -12`
Expected: `error[E0432]: unresolved import \`ade_git::branch_ops\``

- [ ] **Step 3: 写 `branch_ops.rs`**

```rust
//! 原地检出分支（spec §13.5）。
//!
//! 与「跳到已检出该分支的 workspace」（增量 1）是两条不同路径：本模块真的改
//! 当前 worktree 的 HEAD。因此有两个 git 层面的硬约束必须先处理：
//!   F1 分支已被**别的 worktree** 检出时 `git checkout` 直接 fatal —— 必须先复核，
//!      并把拒绝作为**结构化软错误**回传（不是 Err：前端要按 kind 出不同文案）。
//!   F2 `git checkout origin/main` 进的是 detached HEAD 而非建本地跟踪分支 ——
//!      远程目标必须显式 `checkout -b <短名> --track <remote>/<短名>`。

use serde::{Deserialize, Serialize};

use ade_core::errors::CoreError;

use crate::command::{run, run_text};
use crate::GitWorktreeEntry;

/// 调用方能表达的目标。**只有这两种** —— tag / commit 的 detached checkout 本期
/// 不提供（spec §13.5）：ade 的 workspace 模型对 detached HEAD 没有好位置放，
/// `Worktree.branch` 会被刷成 null、头部退化成 DetachedHeadBadge。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitCheckoutTarget {
    /// `"local"` | `"remote"`。字符串而非 enum：前端从 `GitRefEntry.category` 直接映射，
    /// 少一层转换；非法值在 `resolve` 里统一报 `InvalidTarget`。
    pub kind: String,
    /// 短名：本地为 `feature`，远程为 `origin/feature`。
    pub name: String,
}

/// 拒绝原因。经 `GitCheckoutOutcome.status` 以 `Ok(...)` 透出，**不走** `BridgeError`
/// （后者 Serialize 成 `{message}`，类型信息全丢）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitCheckoutRefusal {
    /// F1：目标分支已被另一个 worktree 检出。
    CheckedOutElsewhere,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitCheckoutStatus {
    Switched,
    /// 目标就是当前分支：不执行任何写操作，直接回。
    AlreadyOnBranch,
    Refused(GitCheckoutRefusal),
    /// 目标解析不到真实 ref，或 kind 非法。
    InvalidTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitCheckoutOutcome {
    pub status: GitCheckoutStatus,
    /// 成功或 AlreadyOnBranch 时是新分支短名；`CheckedOutElsewhere` 时是占用方路径。
    /// 复用同一字段会让 TS 侧拿到 `string | null` 而分不清语义，故拆两个字段。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 规范化后的目标：一个本地分支短名 + 可选的「从哪个远程 ref 建并跟踪」。
struct Resolved {
    local_branch: String,
    track: Option<String>,
}

fn invalid_target() -> GitCheckoutOutcome {
    GitCheckoutOutcome {
        status: GitCheckoutStatus::InvalidTarget,
        branch: None,
        worktree_path: None,
        message: Some("目标不是可检出的分支".to_string()),
    }
}

/// `git rev-parse --verify --quiet <ref>` 是否解析成功。
fn ref_exists(worktree_path: &str, reference: &str) -> bool {
    matches!(
        run_text(worktree_path, &["rev-parse", "--verify", "--quiet", reference]),
        Ok(out) if !out.trim().is_empty()
    )
}

/// 把调用方给的 `(kind, name)` 折成一个**真实存在**的本地分支目标。
///
/// 关键：绝不把外部字符串当 revision 直接丢给 `git checkout`。远程目标必须剥出
/// 短名并显式 `--track`，否则按 F2 会进 detached HEAD。
fn resolve(worktree_path: &str, target: &GitCheckoutTarget) -> Option<Resolved> {
    let name = target.name.trim();
    if name.is_empty() || name.starts_with('-') {
        return None;
    }
    match target.kind.as_str() {
        "local" => {
            let full = format!("refs/heads/{name}");
            ref_exists(worktree_path, &full).then(|| Resolved { local_branch: name.to_string(), track: None })
        }
        "remote" => {
            // name 形如 `origin/feature`；remote 名不含 `/`（git 的硬性规则），
            // 所以 split_once('/') 的第一段一定是 remote。
            let (remote, short) = name.split_once('/')?;
            let full = format!("refs/remotes/{remote}/{short}");
            if !ref_exists(worktree_path, &full) {
                return None;
            }
            // 本地已有同名分支 → 直接检出它（它可能就是 upstream 对应的那个数），
            // 不再 `-b`，否则 git 会因 "already exists" 失败。
            if ref_exists(worktree_path, &format!("refs/heads/{short}")) {
                return Some(Resolved { local_branch: short.to_string(), track: None });
            }
            Some(Resolved { local_branch: short.to_string(), track: Some(full) })
        }
        _ => None,
    }
}

/// 找出把 `branch` 检出在**其它** worktree 的那一个（F1）。
///
/// `worktree_list` 的 `branch` 是完整 ref（`refs/heads/x`），所以要比完整形态。
fn occupant(worktrees: &[GitWorktreeEntry], here: &str, branch: &str) -> Option<String> {
    let full = format!("refs/heads/{branch}");
    worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some(full.as_str()) && w.path != here)
        .map(|w| w.path.clone())
}

fn already_checked_out(path: &str, branch: &str) -> bool {
    matches!(
        run_text(path, &["branch", "--show-current"]),
        Ok(current) if current.trim() == branch
    )
}

/// 在当前 worktree 原地检出 `target`。
///
/// `Err(CoreError)` 只留给「环境级失败」（git 不可用、路径不允许、git 非零退出且
/// 不是我们能分类的拒绝）。业务级拒绝一律 `Ok(GitCheckoutOutcome { status: ... })`。
pub fn checkout(worktree_path: &str, target: &GitCheckoutTarget) -> Result<GitCheckoutOutcome, CoreError> {
    let Some(resolved) = resolve(worktree_path, target) else {
        return Ok(invalid_target());
    };

    if already_checked_out(worktree_path, &resolved.local_branch) {
        return Ok(GitCheckoutOutcome {
            status: GitCheckoutStatus::AlreadyOnBranch,
            branch: Some(resolved.local_branch),
            worktree_path: None,
            message: None,
        });
    }
    let worktrees = crate::worktree_list(worktree_path)?;
    if let Some(path) = occupant(&worktrees, worktree_path, &resolved.local_branch) {
        return Ok(GitCheckoutOutcome {
            status: GitCheckoutStatus::Refused(GitCheckoutRefusal::CheckedOutElsewhere),
            branch: Some(resolved.local_branch),
            worktree_path: Some(path),
            message: Some(F1_MESSAGE.to_string()),
        });
    }

    // 先算完整 argv 再统一执行，不拆成两条带 borrow  gymnastics 的分支。
    let argv: Vec<&str> = match &resolved.track {
        // F2：远程目标必须显式建并跟踪，否则进 detached HEAD。
        // `--track -b <短名> <远程 ref>` 由 git 自己建立 upstream。
        Some(full) => vec!["checkout", "--track", "-b", resolved.local_branch.as_str(), full.as_str()],
        None => vec!["checkout", resolved.local_branch.as_str()],
    };
    run_checkout(worktree_path, &argv, resolved.local_branch)
}

const F1_MESSAGE: &str = "该分支已被另一个工作区检出";

fn run_checkout(
    worktree_path: &str,
    args: &[&str],
    branch: String,
) -> Result<GitCheckoutOutcome, CoreError> {
    // `command::run` 已经把非零退出折成 `CoreError::GitCommandFailed`，所以这里**没有**
    // `output.status` 可查。走到 `Err` 的就是「我们无法分类的失败」，最典型是 F3
    // （checkout 会覆盖本地改动）。
    //
    // 为什么 F3 不当软错误回传：要把它从其他失败里分出来只能去匹配 git 的 stderr 文本，
    // 而 spec §3.8 点名批评的正是这个做法（`git-branch-delete-refusal.ts` 靠
    // `text.contains("is checked out")` 猜，git 版本/locale 一改就失效）。
    // F3 的处理止于「把 git 自己那句人话展示给用户」，不需要分支判定，
    // 所以 `Err(BridgeError)` → `{message}` 恰好够用。前端在 2B 会先查 status 拦一道确认。
    run(worktree_path, args)?;
    Ok(GitCheckoutOutcome {
        status: GitCheckoutStatus::Switched,
        branch: Some(branch),
        worktree_path: None,
        message: None,
    })
}
```

> 另：`command::run(worktree_path, args: &[&str]) -> Result<Vec<u8>, CoreError>`，
> **非零退出已在 `run` 内部折成 `CoreError::GitCommandFailed`**（`command.rs:15-24`）。
> 所以 `run_checkout` 拿不到 `Output`，也不要试图自己再包一层退出码判断。

- [ ] **Step 4: 注册模块**

`src-tauri/crates/ade-git/src/lib.rs` 的模块声明区按字母序插入（`base_ref` 之后、`branch.rs` 之前）：

```rust
pub mod branch_ops;
```

`pub use` 区加：

```rust
pub use branch_ops::{GitCheckoutOutcome, GitCheckoutRefusal, GitCheckoutStatus, GitCheckoutTarget};
```

- [ ] **Step 5: 跑测试**

Run: `cd src-tauri && cargo test -p ade-git --test branch_checkout 2>&1 | tail -20`
Expected: `checkout_local_branch_switches_head ... ok`。

- [ ] **Step 6: 加 F1 拒绝的失败测试**

追加到 `branch_checkout.rs`：

```rust
// F1：分支被别的 worktree 检出时 git 会 fatal。我们必须先复核并回结构化拒绝，
// 而不是把 git 的 stderr 当环境级错误抛给前端。
#[test]
fn checkout_refuses_branch_already_checked_out_in_another_worktree() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let other = d.path().join("wt-feature");
    assert!(git(p, &[], &["worktree", "add", other.to_str().unwrap(), "feature"]));

    let outcome = checkout(p, &GitCheckoutTarget { kind: "local".into(), name: "feature".into() }).unwrap();
    assert_eq!(
        outcome.status,
        ade_git::GitCheckoutStatus::Refused(ade_git::GitCheckoutRefusal::CheckedOutElsewhere)
    );
    assert_eq!(outcome.branch.as_deref(), Some("feature"));
    // 占用方路径必须回出来，前端要拿它做「跳到那个 workspace」的跳转。
    assert!(outcome.worktree_path.as_deref().unwrap().ends_with("wt-feature"));
    // 且 HEAD 不能被移动 —— 拒绝必须是**无副作用**的。
    let current = String::from_utf8(
        Command::new("git").arg("-C").arg(p).args(["branch", "--show-current"]).output().unwrap().stdout,
    ).unwrap();
    assert_eq!(current.trim(), "main");
}

// F2：远程目标必须建**本地跟踪分支**，不能把用户推进 detached HEAD。
#[test]
fn checkout_remote_creates_local_tracking_branch() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/feature", "feature"]));

    let outcome = checkout(
        p,
        &GitCheckoutTarget { kind: "remote".into(), name: "origin/feature".into() },
    ).unwrap();
    assert!(matches!(outcome.status, GitCheckoutStatus::Switched), "{:?}", outcome.status);

    let show = Command::new("git").arg("-C").arg(p)
        .args(["for-each-ref", "--format=%(refname:short)|up=%(upstream:short)", "refs/heads/feature"])
        .output().unwrap();
    let line = String::from_utf8_lossy(&show.stdout);
    // 短名必须是 feature（不是 detached），且 upstream 真指向 origin/feature。
    assert_eq!(line.trim(), "feature|up=origin/feature", "actual: {line}");
    let current = String::from_utf8(
        Command::new("git").arg("-C").arg(p).args(["branch", "--show-current"]).output().unwrap().stdout,
    ).unwrap();
    assert_eq!(current.trim(), "feature", "绝不能是 detached");
}

// 已在目标分支上是幂等的：不执行 checkout，不报错。
#[test]
fn checkout_current_branch_is_noop() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = checkout(p, &GitCheckoutTarget { kind: "local".into(), name: "main".into() }).unwrap();
    assert_eq!(outcome.status, ade_git::GitCheckoutStatus::AlreadyOnBranch);
}

// 不存在的 ref / 非法 kind 一律 InvalidTarget，**不是** Err —— 否则前端把它显示成
// 一条环境级错误而不是一句「这个目标不能检出」。
#[test]
fn unknown_targets_are_invalid_target_not_error() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    for target in [
        GitCheckoutTarget { kind: "local".into(), name: "nope".into() },
        GitCheckoutTarget { kind: "remote".into(), name: "origin/nope".into() },
        GitCheckoutTarget { kind: "tag".into(), name: "v1".into() },
        GitCheckoutTarget { kind: "local".into(), name: "".into() },
        GitCheckoutTarget { kind: "local".into(), name: "--help".into() },
    ] {
        let outcome = checkout(p, &target).unwrap();
        assert_eq!(outcome.status, ade_git::GitCheckoutStatus::InvalidTarget, "{:?}", target);
    }
}

// 远程目标但本地已有同名分支 → 检出已有本地分支，不再 `-b`（否则 git 报 already exists）。
#[test]
fn remote_target_with_existing_local_branch_checks_out_local() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/main", "main"]));
    // refs/heads/main 已存在（就是当前分支），所以走 AlreadyOnBranch 而不是 -b 失败。
    let outcome = checkout(p, &GitCheckoutTarget { kind: "remote".into(), name: "origin/main".into() }).unwrap();
    assert_eq!(outcome.status, ade_git::GitCheckoutStatus::AlreadyOnBranch);
}
```

Run: `cd src-tauri && cargo test -p ade-git --test branch_checkout 2>&1 | tail -25`
Expected: 6 passed。若 `checkout_remote_creates_local_tracking_branch` 因 git 版本对 `--track -b` 的参数顺序敏感而失败，把 `argv` 改成 `["checkout", "-b", <短名>, "--track", <远程ref>]` 再试，并把结论写进 commit message。

- [ ] **Step 7: 跑 ade-git 全量确认没踩坏既有**

Run: `cd src-tauri && cargo test -p ade-git 2>&1 | grep -E "^test result|^error" | tail -12`
Expected: 全绿，0 failed。

- [ ] **Step 8: Commit**

```bash
git add src-tauri/crates/ade-git/
git commit -m "feat(ade-git): branch_ops::checkout（原地检出 + F1 复核 + F2 建跟踪分支）"
```

---

## Task 2: `git_checkout` tauri 命令 + 7 面镜像 + bindings 再生

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/git.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Modify: `src/bridge/real/generated/tauri-bindings.ts`（生成）
- Modify: `src/bridge/real/git.ts`
- Modify: `src/shared/preload-api/api/git-operation-api.ts`
- Create: `src/shared/git-checkout-types.ts`
- Create: `src-tauri/crates/ade-bridge/tests/git_checkout.rs`

**Interfaces:**
- Consumes: Task 1 的 `ade_git::{GitCheckoutTarget, GitCheckoutOutcome, GitCheckoutStatus, GitCheckoutRefusal}`；`require_authorized_worktree(&state.fs, path)`、`run_blocking(closure)`（两者均在 `commands/git.rs` 内已存在）
- Produces:
  - tauri 命令 `git_checkout(args: GitCheckoutArgs) -> Result<GitCheckoutOutcome, BridgeError>`
  - `window.api.git.checkout({ worktreePath, target })` → `Promise<GitCheckoutOutcome>`

- [ ] **Step 1: 写 bridge 层失败测试**

Create `src-tauri/crates/ade-bridge/tests/git_checkout.rs`（`git()` / `repo()` helper 照抄 `tests/git_refs.rs` 的同名实现）：

```rust
use ade_bridge::commands::git::git_checkout_impl;
use ade_git::{GitCheckoutRefusal, GitCheckoutStatus, GitCheckoutTarget};
use std::process::Command;

fn current(dir: &str) -> String {
    String::from_utf8_lossy(
        &Command::new("git").arg("-C").arg(dir).args(["branch", "--show-current"]).output().unwrap().stdout,
    ).trim().to_string()
}

// bridge 层只测「透传不吞语义」：status 与 worktree_path 必须原样到调用方。
#[test]
fn git_checkout_impl_switches_and_reports_new_branch() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_checkout_impl(p, &GitCheckoutTarget { kind: "local".into(), name: "feature".into() }).unwrap();
    assert_eq!(outcome.status, GitCheckoutStatus::Switched);
    assert_eq!(outcome.branch.as_deref(), Some("feature"));
    assert_eq!(current(p), "feature");
}

#[test]
fn git_checkout_impl_surfaces_checked_out_elsewhere_with_path() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let other = d.path().join("wt-feature");
    assert!(git(p, &[], &["worktree", "add", other.to_str().unwrap(), "feature"]));

    let outcome = git_checkout_impl(p, &GitCheckoutTarget { kind: "local".into(), name: "feature".into() }).unwrap();
    assert_eq!(
        outcome.status,
        GitCheckoutStatus::Refused(GitCheckoutRefusal::CheckedOutElsewhere),
        "拒绝必须是 Ok 而不是 Err —— BridgeError 会丢掉 status"
    );
    assert!(outcome.worktree_path.is_some());
}
```

Run: `cd src-tauri && cargo test -p ade-bridge --test git_checkout 2>&1 | tail -12`
Expected: 编译失败 —— `git_checkout_impl` 不存在

- [ ] **Step 2: 加命令**

`src-tauri/crates/ade-bridge/src/commands/git.rs`，紧接 `git_refs` 之后（保持 git 写命令聚在 `git_push` 附近亦可）：

```rust
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCheckoutArgs {
    pub worktree_path: String,
    pub target: ade_git::GitCheckoutTarget,
}

/// 原地检出分支。业务级拒绝（被别的 worktree 占用、目标非法）以 `Ok(GitCheckoutOutcome)`
/// 的 `status` 透出，**不**经 `BridgeError` —— 后者的 Serialize 只留 `{message}`。
pub fn git_checkout_impl(
    worktree_path: &str,
    target: &ade_git::GitCheckoutTarget,
) -> Result<ade_git::GitCheckoutOutcome, BridgeError> {
    Ok(ade_git::branch_ops::checkout(worktree_path, target)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_checkout(
    state: State<'_, AppState>,
    args: GitCheckoutArgs,
) -> Result<ade_git::GitCheckoutOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let target = args.target;
    run_blocking(move || git_checkout_impl(&args.worktree_path, &target)).await
}
```

> 若 `args` 被 move 进闭包后访问 `worktree_path` 报借用错，改成
> `let path = args.worktree_path; run_blocking(move || git_checkout_impl(&path, &target))`。

- [ ] **Step 3: 注册进 `collect_commands!`**

`src-tauri/crates/ade-bridge/src/specta_export.rs` 的 git 区段（`commands::git::git_refs,` 之后）加：

```rust
                commands::git::git_checkout,
```

Run: `cd src-tauri && cargo test -p ade-bridge --test git_checkout 2>&1 | tail -20`
Expected: 2 passed

- [ ] **Step 4: 再生 bindings 并核对形状**

```bash
cd src-tauri && cargo run -q -p ade-bridge --bin export-bindings
cargo test -p ade-bridge --lib specta_export 2>&1 | grep -E "^test |^test result"
```
Expected: `bindings_are_fresh ... ok`、`command_inventory_and_bindings_are_fresh ... ok`

```bash
grep -n "GitCheckout" -A 10 ../src/bridge/real/generated/tauri-bindings.ts | head -40
```
Expected: `GitCheckoutStatus = "switched" | "already-on-branch" | { refused: GitCheckoutRefusal } | "invalid-target"`、`GitCheckoutRefusal = "checked-out-elsewhere"`、`GitCheckoutOutcome_Serialize = { status: GitCheckoutStatus; branch?: string | null; worktreePath?: string | null; message?: string | null }`。
若 `status` 生成为 `string` 而非联合，说明 serde enum 的表示形式与 specta 不合 —— **停下来改 Rust 侧表示**（例如把 `Refused` 拆成 `status: GitCheckoutStatusKind` + `refusal?: GitCheckoutRefusal` 两个字段），不要在 TS 手抄镜像里编一个更宽的类型蒙过去。

- [ ] **Step 5: 写 TS 手抄镜像**

Create `src/shared/git-checkout-types.ts`：

```ts
/**
 * `ade_git::branch_ops` 的 TS 镜像。改 Rust 侧必须同步这里 ——
 * `bindings_are_fresh` 只锁生成文件，不锁这份手抄镜像（同 `git-ref-types.ts` 的约定）。
 *
 * 刻意**不**从生成的 `tauri-bindings.ts` 引类型：渲染层与 preload-api 都只依赖
 * `src/shared/**` 的手抄契约，混用会让 `| null` 与 `?:` 两种形状在边界处打架。
 */
export type GitCheckoutTargetKind = 'local' | 'remote'

export type GitCheckoutTarget = {
  kind: GitCheckoutTargetKind
  /** 本地为短名 `feature`；远程为 `origin/feature`。 */
  name: string
}

export type GitCheckoutRefusal = 'checked-out-elsewhere'

export type GitCheckoutOutcome = {
  status:
    | 'switched'
    | 'already-on-branch'
    | 'invalid-target'
    | { refused: GitCheckoutRefusal }
  /**
   * 成功/幂等时是新分支短名。
   * 与生成绑定的字面差异（`?: T` vs `?: T | null`）同 `git-ref-types.ts`：
   * Rust 侧 `skip_serializing_if` 会整键省略，运行时只会是 `undefined`。本文件
   * 多一个未声明风险：`status` 是带内联对象的可判别联合，改动 Rust 侧 enum 表示
   * 会连带破这里，Step 4 的形状核对必须逐字对。
   */
  branch?: string
  /** `checked-out-elsewhere` 时是占用方 worktree 路径，前端据此给跳转。 */
  worktreePath?: string
  message?: string
}

export function isCheckoutSwitched(outcome: GitCheckoutOutcome): boolean {
  return outcome.status === 'switched' || outcome.status === 'already-on-branch'
}
```

- [ ] **Step 6: preload-api + real 映射**

`src/shared/preload-api/api/git-operation-api.ts` 头部 import 区加：

```ts
import type { GitCheckoutOutcome, GitCheckoutTarget } from '../../git-checkout-types'
```

`GitOperationApi` 类型里（`fastForward` 之后）加：

```ts
  /**
   * 原地检出分支。业务级拒绝在 resolve 里（`outcome.status`），**不** reject ——
   * `BridgeError` 会丢掉 status。只有环境级失败（git 不可用、会覆盖本地改动）才 reject。
   */
  checkout: (args: { worktreePath: string; target: GitCheckoutTarget }) => Promise<GitCheckoutOutcome>
```

`src/bridge/real/git.ts` 的 real 映射里（`refs:` 之后）加：

```ts
    checkout: (args) => invokeCommand('git_checkout', { args }),
```

并更新该文件头部注释里「留在 rejecting 面上的方法」清单，把 checkout 移出去。

- [ ] **Step 7: typecheck + commit**

Run: `pnpm typecheck 2>&1 | tail -10`
Expected: exit 0。若报 `checkout` 在 `PreloadApi['git']` 上不存在，说明 `git-operation-api.ts` 没被 `api-types.ts` 的 git 交叉类型收进来 —— 去 `src/shared/preload-api/api-types.ts` 确认 `GitOperationApi` 的合并方式，别在 real 侧加 `as any`。

```bash
git add src-tauri/crates/ade-bridge/ src/bridge/real/ src/shared/
git commit -m "feat(ade-bridge): git_checkout 命令 + 7 面镜像 + TS 契约"
```

---

## Task 3: runtime 客户端包装 + 契约测试

**Files:**
- Create: `src/renderer/src/runtime/runtime-git-checkout-client.ts`
- Test: `src/renderer/src/runtime/runtime-git-checkout-client.test.ts`
- Modify: `src/renderer/src/runtime/runtime-git-client.ts`

**Interfaces:**
- Consumes: `RuntimeGitContext`（`runtime-git-client-context.ts`）、`window.api.git.checkout`
- Produces: `checkoutRuntimeGit(context: RuntimeGitContext, target: GitCheckoutTarget): Promise<GitCheckoutOutcome>`

- [ ] **Step 1: 写失败测试**

Create `src/renderer/src/runtime/runtime-git-checkout-client.test.ts`：

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitCheckoutOutcome } from '../../../../shared/git-checkout-types'
import { checkoutRuntimeGit } from './runtime-git-checkout-client'

const checkout = vi.fn()
;(globalThis as unknown as { window: unknown }).window = { api: { git: { checkout } } }

const context = {
  settings: null,
  worktreeId: 'r::/a',
  worktreePath: '/a',
  connectionId: undefined,
} as never

const refused: GitCheckoutOutcome = {
  status: { refused: 'checked-out-elsewhere' },
  branch: 'feature',
  worktreePath: '/other',
}

beforeEach(() => checkout.mockReset())

describe('checkoutRuntimeGit', () => {
  it('把 worktreePath 与 target 原样下发', async () => {
    checkout.mockResolvedValue({ status: 'switched', branch: 'feature' } satisfies GitCheckoutOutcome)
    const outcome = await checkoutRuntimeGit(context, { kind: 'local', name: 'feature' })
    expect(checkout).toHaveBeenCalledWith({
      worktreePath: '/a',
      target: { kind: 'local', name: 'feature' },
    })
    expect(outcome.status).toBe('switched')
  })

  it('业务级拒绝**不**被改写成异常（前端要按 status 分支出不同文案与跳转）', async () => {
    checkout.mockResolvedValue(refused)
    const outcome = await checkoutRuntimeGit(context, { kind: 'local', name: 'feature' })
    expect(outcome).toEqual(refused)
  })

  it('环境级失败原样上抛，不吞', async () => {
    checkout.mockRejectedValue(new Error('would be overwritten by checkout'))
    await expect(checkoutRuntimeGit(context, { kind: 'local', name: 'feature' })).rejects.toThrow(
      /overwritten by checkout/,
    )
  })

  it('空 worktreePath 时本地就拒，不把空路径下发给鉴权层', async () => {
    await expect(
      checkoutRuntimeGit({ settings: null, worktreeId: null, worktreePath: '' } as never, {
        kind: 'local',
        name: 'x',
      })
    ).rejects.toThrow(/worktree path/)
    expect(checkout).not.toHaveBeenCalled()
  })
})
```

Run: `pnpm vitest run src/renderer/src/runtime/runtime-git-checkout-client.test.ts 2>&1 | tail -12`
Expected: FAIL —— 找不到模块

- [ ] **Step 2: 实现**

Create `src/renderer/src/runtime/runtime-git-checkout-client.ts`：

```ts
import type { GitCheckoutOutcome, GitCheckoutTarget } from '../../../../shared/git-checkout-types'
import { resolveLocalWorktreePath, type RuntimeGitContext } from './runtime-git-client-context'

/**
 * 原地检出分支的 runtime 包装。
 *
 * **只有本地一条路**：`runtime` / `runtimeEnvironments` 在本仓库没有 real 实现
 * （`create-api.ts` 的 real 域清单里没有它们），远程分支的 `callRuntimeRpc`
 * 照 push/pull 的样子加也只会得到一个 unimplemented。故此处刻意不开 RPC 分支，
 * 等远程 runtime 面真落地时再一起补。
 *
 * 为什么自己挡空路径：`resolveLocalWorktreePath` **不抛错**（它回落到
 * `context.worktreePath`，可能是空串），而 checkout 是写操作 ——
 * 把空路径下发给 `require_authorized_worktree` 去报错，不如在这层给出可读的失败。
 */
export async function checkoutRuntimeGit(
  context: RuntimeGitContext,
  target: GitCheckoutTarget
): Promise<GitCheckoutOutcome> {
  const worktreePath = resolveLocalWorktreePath(context)
  if (!worktreePath) {
    throw new Error('checkoutRuntimeGit: missing worktree path')
  }
  return window.api.git.checkout({ worktreePath, target })
}
```

Run: `pnpm vitest run src/renderer/src/runtime/runtime-git-checkout-client.test.ts 2>&1 | tail -15`
Expected: 4 passed

- [ ] **Step 3: 从 `runtime-git-client.ts` 导出**

`src/renderer/src/runtime/runtime-git-client.ts` 的 import 区加：

```ts
import {
  checkoutRuntimeGit as checkoutRuntimeGitImplementation
} from './runtime-git-checkout-client'
```

导出区（`export const fastForwardRuntimeGit = ...` 附近）加：

```ts
export const checkoutRuntimeGit = checkoutRuntimeGitImplementation
```

- [ ] **Step 4: 门禁 + commit**

```bash
pnpm typecheck 2>&1 | tail -5
pnpm vitest run src/renderer/src/runtime 2>&1 | grep -E "Test Files|Tests "
cd src-tauri && cargo test -p ade-git -p ade-bridge 2>&1 | grep -E "^test result: FAILED|test result: ok" | tail -14
```
Expected: typecheck 0；runtime 测试全绿；两个 crate 0 failed。

```bash
git add src/renderer/src/runtime/
git commit -m "feat(runtime): checkoutRuntimeGit（仅本地路径，拒绝不改写成异常）"
```

---

## Task 4: 2A 收尾门禁

- [ ] **Step 1: 全量 Rust**

```bash
cd src-tauri && cargo build -q -p orcinus-cli --bin orcinus && cargo test --workspace 2>&1 | tail -20
```
Expected: exit 0。`cli_basics` 若报 5 项失败是先 `cargo build` 没生效（过期产物，仓库既有环境问题）。

- [ ] **Step 2: 全量 TS**

Run: `pnpm test 2>&1 | tail -20`
Expected: 除 `src/shared/startup-shell-portability.live-shell.test.ts`（live-shell，与 `git diff main..HEAD --name-only` 证实本分支未碰，且已在增量 1 记录为待用户机器确认）外全绿。

- [ ] **Step 3: 手工验一下命令真的能改工作区（后端没有 UI，只能这么验）**

```bash
cd /tmp && rm -rf sdd-co && mkdir sdd-co && cd sdd-co && git init -qb main . \
  && echo a > f && git add f && git -c user.name=T -c user.email=t@e.com commit -qm one \
  && git branch dev && git worktree add ../sdd-co-dev dev \
  && git checkout dev 2>&1 | head -2
```
Expected: `fatal: 'dev' is already used by worktree at '.../sdd-co-dev'` —— 这就是 F1，
`branch_ops::checkout` 的 `occupant()` 复核必须**先于**这次 git 调用发生，
所以 `checkout_refuses_branch_already_checked_out_in_another_worktree` 里
「HEAD 仍在 main」那条断言是本任务最重要的守卫。

- [ ] **Step 4: CodeReview 门禁**

派 CodeReview 子代理审 `git diff 4ad50f95..HEAD`，要求覆盖：
- 软错误 vs `BridgeError` 的分界是否只在「环境级失败」上 reject（F3 归环境级是否合理）
- `resolve()` 是否存在把外部字符串当 revision 下传的漏口（尤其 `kind` 非法、`name` 含 `..` 或 `@\{`）
- `occupant()` 比较完整 ref 是否正确（`GitWorktreeEntry.branch` 是完整 ref）
- 新命令的 7 面镜像是否一面不漏（`bindings_are_fresh` 只锁生成文件，锁不到 `git-checkout-types.ts` 手抄镜像与 `git-operation-api.ts`）
- 每条新测试是否可失败

- [ ] **Step 5: 修完审查发现后确认每个 commit 独立可编译**

```bash
git log --oneline 4ad50f95..HEAD
```
每个 commit 都要能过 `pnpm typecheck`（Task 2 的 Step 6/7 顺序保证了 real 映射与 preload-api 类型同 commit）。
