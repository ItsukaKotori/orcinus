# IDEA 风格分支树 + 分支切换（增量 1）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Source Control 面板头部提供 IDEA 风格的分支树以切换分支，并把 Git Log 的左栏分支面板合并进这同一棵树组件。

**Architecture:** 新建无业务语义的共享树组件 `components/git-branch/`（纯函数模型 + 纯呈现组件 + 动作声明表），由两个宿主以不同 `activate` 语义注入：Source Control 头部（切到已检出该分支的 workspace，未检出则从该分支新建 workspace）、Git Log 工具栏 branch chip（设查看 ref）。后端只扩 `list_refs` 的返回契约（新增 `GitRefEntry`，一次 `for-each-ref` 带出 ahead/behind/upstream/HEAD），**不新增任何写命令**。

**Tech Stack:** Rust（`ade-git` / `ade-bridge` + tauri-specta 绑定再生）、TypeScript、React 19、Tailwind + daisyUI 风格 `components/ui/*`、Vitest + happy-dom、`cargo test`。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` —— 本计划从 spec 论证，实施时两份一起读。

## Global Constraints

以下为全局约束，每个任务都隐含遵守（值逐字取自 spec）：

- **不引入 `git checkout` / 原地切分支**（spec §2.1）。切换 = `setActiveWorktree`，未检出 = `openModal('new-workspace-composer', { initialBaseBranch })`。
- **增量 1 不新增 tauri 写命令**，只扩 `git_refs` 的返回类型（spec §10 增量 1）。
- 新命名空间 `auto.components.git-branch` 的 locale key **只补 `en.json` + `zh.json`**；其余 4 个 locale 靠 `translate(key, fallback)` 的 `defaultValue` 兜底（spec §3.4，已核实 `auto.components.git-log` 同样只存在于 en/zh）。
- `translate()` **不得在模块顶层调用** —— `src/renderer/src/i18n/no-top-level-translate.test.ts` 是 AST 门禁。动作/分组 label 一律写成 `() => translate(…)`（spec §3.4）。
- `GitRefsResult.refs` **换元素类型，不做向后兼容**，不保留 `GitHistoryItemRef` 形状（spec §2.6）。
- 前端算出的「已检出」标记只是 advisory；权威判定留给后续增量的后端 `worktree_list` 复核（spec §6.4）。
- 每个任务结束必须过：`pnpm typecheck` + 该任务列出的定向测试全绿，然后 commit。仓库**没有 pre-commit 钩子**，门禁全靠手动执行。
- 受控 `<input>` 在 happy-dom 下必须用原生 value setter 触发 `onChange`（既有陷阱，见 Task 6 Step 2 的示例写法）。

## 与 spec 的一处偏差（实施前须知）

spec §10 增量 1 写了「动作集只放 Switch / New Workspace from Branch… / Show Diff with Working Tree / View in Git Log」。
本计划把 **`branch-menu.tsx`（右键 + `⋯` 菜单）整体推到增量 2**，因此增量 1 里四个动作的可达性是：

- `Switch` = 宿主 A 对**有 owner** 的行的 activate ✅
- `New Workspace from Branch…` = 宿主 A 对**无 owner** 的行的 activate ✅（正是 spec §2.1 的兜底语义）
- `View in Git Log` = 宿主 B 所有行的 activate ✅
- `Show Diff with Working Tree` = **增量 1 不可达**，随 `branch-menu` 在增量 2 落地 ⚠️

理由：增量 1 的用户可见目标是「能切分支」，`activate` 单通道已覆盖前三个动作；把菜单壳子塞进增量 1 会让它同时改 4 个宿主文件 + 引入 Radix ContextMenu 接线，破坏「每个任务都可独立审、可单独否掉」。Task 8 的 `branch-actions.ts` 仍按 spec 的接口建好，增量 2 只加 `branch-menu.tsx` 消费它，不动 actions 定义。

## 文件结构

**新建**

| 文件 | 职责 |
|---|---|
| `src/shared/git-ref-types.ts` | `GitRefEntry` / `GitRefCategory` 的 TS 镜像（供 preload-api 与组件共用） |
| `src-tauri/crates/ade-git/src/ref_entry.rs` | `GitRefEntry` / `GitRefCategory` / `parse_upstream_track` / `git_ref_entry`（从 `history.rs` 拆出，避免 `history.rs` 继续长到 900 行） |
| `src/renderer/src/components/git-branch/branch-tree-model.ts` | 纯函数：`GitRefEntry[]` → 可见行列表（折叠态已应用） |
| `src/renderer/src/components/git-branch/branch-tree-filter.ts` | 纯函数：`matchesBranchQuery` |
| `src/renderer/src/components/git-branch/branch-ref-tree.tsx` | 纯呈现：搜索框 + 行列表 + 键盘 + 行尾槽 |
| `src/renderer/src/components/git-branch/branch-actions.ts` | 动作声明表 + `primaryBranchAction` |
| `src/renderer/src/components/git-branch/use-branch-refs.ts` | 取 refs + `invalidate()` |
| `src/renderer/src/components/git-branch/source-control-branch-switcher.tsx` | Source Control 宿主：Popover 壳 + store 读取 + 动作执行 |
| 各文件同名 `.test.ts` / `.test.tsx` | 见各任务 |

**修改**

| 文件 | 改动 |
|---|---|
| `src-tauri/crates/ade-git/src/history.rs` | `GitRefsResult.refs` 换型；`list_refs_capped` 扩 format 与解析；re-export `ref_entry` |
| `src-tauri/crates/ade-git/src/lib.rs` | `mod ref_entry;` + `pub use` |
| `src-tauri/crates/ade-git/tests/history_query.rs` | 新增 track/HEAD 断言 |
| `src-tauri/crates/ade-bridge/tests/git_refs.rs` | 断言换型后的字段 |
| `src/bridge/real/generated/tauri-bindings.ts` | `cargo run -p ade-bridge --bin export-bindings` 再生 |
| `src/shared/preload-api/api/git-inspection-api.ts` | `GitRefsResult` 换型（**Task 10 做**，与删消费者同 commit） |
| `src/renderer/src/components/git-log/types.ts` | `GitRefs` 换型（**Task 10 做**，同上） |
| `src/renderer/src/components/git-log/GitLogView.tsx` | 删 `RefPanel` 左栏；refs 换型 |
| `src/renderer/src/components/git-log/Toolbar.tsx` | branch chip → `BranchRefTree` popover |
| `src/renderer/src/components/git-log/RefPanel.tsx` | **删除** |
| `src/renderer/src/components/git-log/git-log-view.test.tsx` | 删 `RefPanel` 用例、改 refs fixture 形状 |
| `src/renderer/src/components/right-sidebar/source-control/panel/branch-context-row.tsx` | `HeadIdentity` 可选接入 switcher |
| `src/renderer/src/components/right-sidebar/source-control/panel/header-toolbar.tsx` | 透传 `branchSwitcher` |
| `src/renderer/src/components/right-sidebar/source-control/panel/panel-ready.tsx` | 构造 `branchSwitcher` 上下文 |
| `src/renderer/src/i18n/locales/en.json` / `zh.json` | 新 `git-branch` 命名空间 |

---

## Task 1: Rust — `GitRefEntry` 与 `%(upstream:track)` 解析

**Files:**
- Create: `src-tauri/crates/ade-git/src/ref_entry.rs`
- Modify: `src-tauri/crates/ade-git/src/lib.rs`（加 `mod ref_entry;` 与 re-export）
- Modify: `src-tauri/crates/ade-git/src/history.rs:327-388`（`GitRefsResult` + `list_refs_capped`）
- Test: `src-tauri/crates/ade-git/tests/history_query.rs`

**Interfaces:**
- Consumes: `crate::command::run_text`（`pub(crate)`，签名 `run_text(worktree_path: &str, args: &[&str]) -> Result<String, CoreError>`）
- Produces:
  - `ade_git::GitRefCategory` = `Local | Remote | Tag`，serde `rename_all = "lowercase"`
  - `ade_git::GitRefEntry { name, full_name, category, remote: Option<String>, oid, is_head: bool, upstream: Option<String>, ahead: Option<u32>, behind: Option<u32> }`，serde `rename_all = "camelCase"`
  - `ade_git::history::GitRefsResult { refs: Vec<GitRefEntry>, truncated: bool }`（字段名不变，元素换型）
  - `ade_git::ref_entry::parse_upstream_track(&str) -> (Option<u32>, Option<u32>)`

- [ ] **Step 1: 写失败测试 —— track 解析矩阵**

在 `src-tauri/crates/ade-git/tests/history_query.rs` 末尾追加（该文件已 `use ade_git::history::{…}`，本步用完全限名以免动头部）：

```rust
// 增量 1：%(upstream:track) 解析矩阵。计数是装饰，不是列 refs 的前置条件，
// 所以畸形输入必须降级为 None 而不是报错。
#[test]
fn parse_upstream_track_matrix() {
    use ade_git::ref_entry::parse_upstream_track;
    assert_eq!(parse_upstream_track(""), (None, None));
    assert_eq!(parse_upstream_track("[ahead 14]"), (Some(14), None));
    assert_eq!(parse_upstream_track("[behind 3]"), (None, Some(3)));
    assert_eq!(parse_upstream_track("[ahead 2, behind 1]"), (Some(2), Some(1)));
    assert_eq!(parse_upstream_track("[behind 1, ahead 2]"), (Some(2), Some(1)));
    // 畸形/未知形状 → 降级，不 panic、不报错
    assert_eq!(parse_upstream_track("[gone]"), (None, None));
    assert_eq!(parse_upstream_track("[ahead x]"), (None, None));
    assert_eq!(parse_upstream_track("garbage"), (None, None));
}
```

- [ ] **Step 2: 跑测试确认编译失败**

Run: `cd src-tauri && cargo test -p ade-git --test history_query parse_upstream_track_matrix 2>&1 | tail -20`
Expected: `error[E0432]: unresolved import \`ade_git::ref_entry\``

- [ ] **Step 3: 写 `ref_entry.rs`**

Create `src-tauri/crates/ade-git/src/ref_entry.rs`：

```rust
//! `git for-each-ref` 的分支/标签条目模型（增量 1：IDEA 风格分支树的数据契约）。
//!
//! 与 `GitHistoryItemRef`（commit 图上的 decoration）刻意分开：那个类型服务
//! decoration 渲染，往它塞 `ahead/upstream/remote` 是语义污染（见 spec §6.2）。

use serde::Serialize;

/// ref 的大类。serde 用小写单词，TS 侧 `GitRefCategory` 逐字镜像。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum GitRefCategory {
    Local,
    Remote,
    Tag,
}

/// 一条分支/远程分支/标签，带分支树要显示的信息密度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitRefEntry {
    /// 短名：`main` / `origin/main` / `v1.2.0`。
    pub name: String,
    /// 完整 refname：`refs/heads/main`。
    pub full_name: String,
    pub category: GitRefCategory,
    /// 仅 `Remote` 有值（`origin`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// commit oid；annotated tag 已 peel 到 commit。
    pub oid: String,
    /// git 报告该 ref 就是当前 HEAD 分支。
    pub is_head: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
}

/// `git for-each-ref` 的 `%(upstream:track)` 载荷，形如 `[ahead 2, behind 1]`。
/// 空串（无 upstream 或已同步）→ `(None, None)`。未知形状一律降级为 `None`：
/// 计数是装饰，不该让整份 ref 列表失败。
pub fn parse_upstream_track(raw: &str) -> (Option<u32>, Option<u32>) {
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    let mut ahead = None;
    let mut behind = None;
    for part in inner.split(',') {
        match part.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["ahead", n] => ahead = n.parse().ok(),
            ["behind", n] => behind = n.parse().ok(),
            _ => {}
        }
    }
    (ahead, behind)
}

/// 从 `for-each-ref` 的一行六列构造条目。列序：
/// `refname / objectname / *objectname / HEAD / upstream:short / upstream:track`。
///
/// 后三列允许缺失（老 git 或 `--format` 被截断），缺失按空串处理。
/// 落在 `refs/{heads,remotes,tags}` 之外的 refname 返回 `None`（调用方跳过）。
pub fn git_ref_entry(columns: [&str; 6]) -> Option<GitRefEntry> {
    let [full_name, oid, peeled, head, upstream, track] = columns;
    if full_name.is_empty() || oid.is_empty() {
        return None;
    }
    let (category, name, remote) = if let Some(branch) = full_name.strip_prefix("refs/heads/") {
        (GitRefCategory::Local, branch.to_string(), None)
    } else if let Some(remote_branch) = full_name.strip_prefix("refs/remotes/") {
        let remote = remote_branch
            .split_once('/')
            .map(|(r, _)| r.to_string());
        (GitRefCategory::Remote, remote_branch.to_string(), remote)
    } else if let Some(tag) = full_name.strip_prefix("refs/tags/") {
        (GitRefCategory::Tag, tag.to_string(), None)
    } else {
        return None;
    };
    // annotated tag 的 %(objectname) 是 tag object，%(*objectname) 才是 commit。
    let revision = if peeled.is_empty() { oid } else { peeled };
    let (ahead, behind) = parse_upstream_track(track);
    Some(GitRefEntry {
        name,
        full_name: full_name.to_string(),
        category,
        remote,
        oid: revision.to_string(),
        is_head: head == "*",
        upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
        ahead,
        behind,
    })
}
```

- [ ] **Step 4: 在 `lib.rs` 注册模块**

`src-tauri/crates/ade-git/src/lib.rs` 里，紧接现有 `pub mod history;` 之类的模块声明区追加：

```rust
pub mod ref_entry;
```

并在该文件既有的 `pub use` 区（若无则新起一行）加：

```rust
pub use ref_entry::{GitRefCategory, GitRefEntry};
```

Run: `cd src-tauri && cargo test -p ade-git --test history_query parse_upstream_track_matrix 2>&1 | tail -15`
Expected: `test parse_upstream_track_matrix ... ok`

- [ ] **Step 5: 加 ref 构造的失败测试（`list_refs` 端到端）**

在 `history_query.rs` 末尾追加。复用该文件已有的 `git()` / `commit()` / `repo()`（`repo()` 建 `main` + `feature`，当前分支 `main`）：

```rust
// 增量 1：list_refs 必须带出 isHead / category / fullName / remote。
// 本机实测 git 2.54 输出：refs/heads/main → HEAD 列为 "*"，无 upstream 时 upstream 列为空。
#[test]
fn list_refs_carries_head_flag_and_categories() {
    let d = repo();
    let result = list_refs(d.path().to_str().unwrap()).unwrap();
    let by_name: std::collections::HashMap<&str, _> =
        result.refs.iter().map(|r| (r.name.as_str(), r)).collect();

    let main = by_name.get("main").expect("main listed");
    assert_eq!(main.full_name, "refs/heads/main");
    assert_eq!(main.category, ade_git::GitRefCategory::Local);
    assert!(main.is_head, "当前分支 main 必须 is_head");
    assert_eq!(main.upstream, None);

    let feature = by_name.get("feature").expect("feature listed");
    assert!(!feature.is_head);

    assert!(
        !result.refs.iter().any(|r| r.full_name.ends_with("/HEAD")),
        "refs/remotes/*/HEAD symref 必须继续被跳过"
    );
}
```

Run: `cd src-tauri && cargo test -p ade-git --test history_query list_refs_carries_head_flag 2>&1 | tail -25`
Expected: 编译失败 —— `GitRefsResult.refs` 仍是 `Vec<GitHistoryItemRef>`，无 `full_name` / `is_head` 字段

- [ ] **Step 6: 改 `history.rs` 的 `GitRefsResult` 与 `list_refs_capped`**

`src-tauri/crates/ade-git/src/history.rs`：把 `GitRefsResult`（327-333 行）的 `refs` 字段类型换掉，
并在 `list_refs_capped`（344 行起）替换 format 与解析。文件头部 `use` 区加：

```rust
use crate::ref_entry::{git_ref_entry, GitRefEntry};
```

`GitRefsResult` 改为：

```rust
/// [`list_refs`] 的结果：（截断后的）分支树条目 + 截断标志。
///
/// 元素是 `GitRefEntry` 而非 `GitHistoryItemRef`：分支树要 ahead/behind/upstream，
/// 那些字段对 commit decoration 无意义（spec §6.2）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitRefsResult {
    pub refs: Vec<GitRefEntry>,
    pub truncated: bool,
}
```

`list_refs_capped` 整体替换为：

```rust
/// [`list_refs`] with an explicit cap, so callers/tests can bound the payload.
pub fn list_refs_capped(
    worktree_path: &str,
    max: usize,
) -> Result<GitRefsResult, CoreError> {
    let stdout = run_text(
        worktree_path,
        &[
            "for-each-ref",
            &format!(
                "--format={}",
                [
                    "%(refname)",
                    "%(objectname)",
                    "%(*objectname)",
                    "%(HEAD)",
                    "%(upstream:short)",
                    "%(upstream:track)",
                ]
                .join("\t")
            ),
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    let mut refs = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        let (Some(full), Some(oid)) = (parts.next(), parts.next()) else {
            continue;
        };
        // 后四列允许缺失（git 版本差异），按空串处理：缺的信息只是不显示，不是失败。
        let peeled = parts.next().unwrap_or("");
        let head = parts.next().unwrap_or("");
        let upstream = parts.next().unwrap_or("");
        let track = parts.next().unwrap_or("");
        // Skip the remote HEAD symref (e.g. refs/remotes/origin/HEAD) — noise for a branch picker.
        if full.starts_with("refs/remotes/") && full.ends_with("/HEAD") {
            continue;
        }
        let Some(entry) = git_ref_entry([full, oid, peeled, head, upstream, track]) else {
            continue;
        };
        refs.push(entry);
        // Stop once we've seen one past the cap: enough to set `truncated`
        // without materializing every ref in a huge repository.
        if refs.len() > max {
            break;
        }
    }
    let truncated = refs.len() > max;
    if truncated {
        refs.truncate(max);
    }
    Ok(GitRefsResult { refs, truncated })
}
```

> 注意：format 串里的 `\t` 必须是**真实制表符**，用 `join("\t")` 构造而非手写 `%09`。
> `for-each-ref` 的 `%(HEAD)` 与 `%(upstream:track)` 在本机 git 2.54 实测可用（见 Step 5 注释）。

- [ ] **Step 7: 跑 ade-git 全量，确认没踩坏 decoration 路径**

Run: `cd src-tauri && cargo test -p ade-git 2>&1 | tail -25`
Expected: 全绿。若 `history_query.rs` 里既有的 `list_refs_capped` 截断测试引用了 `GitHistoryItemRef` 字段，
按新字段改断言（只改断言字段名，**不改测试语义**）。

- [ ] **Step 8: Commit**

```bash
git add src-tauri/crates/ade-git/src/ref_entry.rs src-tauri/crates/ade-git/src/lib.rs \
        src-tauri/crates/ade-git/src/history.rs src-tauri/crates/ade-git/tests/history_query.rs
git commit -m "feat(ade-git): list_refs 返回 GitRefEntry（带 ahead/behind/upstream/isHead）"
```

---

## Task 2: ade-bridge 契约 + bindings 再生

**Files:**
- Modify: `src-tauri/crates/ade-bridge/tests/git_refs.rs`
- Modify: `src/bridge/real/generated/tauri-bindings.ts`（由命令生成，不手改）
- Read-only check: `src-tauri/crates/ade-bridge/src/commands/git.rs:718-736`（`git_refs` 无需改动，类型随 `ade_git` 透传）

**Interfaces:**
- Consumes: Task 1 的 `ade_git::GitRefEntry` / `GitRefsResult`
- Produces: TS 侧 `GitRefEntry_Serialize` / `GitRefsResult_Serialize` 绑定类型；`window.api.git.refs()` 的新返回形状

- [ ] **Step 1: 改 `git_refs.rs` 断言到新形状**

`src-tauri/crates/ade-bridge/tests/git_refs.rs` 整体替换 `git_refs_lists_branches_not_truncated`，
并新增一条 upstream 断言。该文件的 `git()` / `repo()` helper 保留不动：

```rust
#[test]
fn git_refs_lists_branches_not_truncated() {
    let d = repo();
    let result = git_refs_impl(d.path().to_str().unwrap()).unwrap();
    let names: Vec<&str> = result.refs.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"main"));
    assert!(names.contains(&"feature"));
    assert!(!result.truncated);
    // 换型后仍要能认出当前分支：ade-bridge 侧的契约测试是 bindings 之外的第二道锁。
    assert_eq!(
        result.refs.iter().filter(|r| r.is_head).count(),
        1,
        "恰好一条 is_head"
    );
}

// 有 upstream 时 ahead 必须落进结构化字段，而不是留在 [ahead N] 文本里。
#[test]
fn git_refs_reports_ahead_against_upstream() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    // 造一个假 origin remote + upstream 配置，不联网：只写 ref 和 config。
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/main", "HEAD"]));
    assert!(git(p, &[], &["config", "branch.main.remote", "origin"]));
    assert!(git(p, &[], &["config", "branch.main.merge", "refs/heads/main"]));
    std::fs::write(d.path().join("b"), "2\n").unwrap();
    git(
        p,
        &["-c", "user.name=T", "-c", "user.email=t@example.com"],
        &["commit", "-am", "two"],
    );

    let result = git_refs_impl(p).unwrap();
    let main = result
        .refs
        .iter()
        .find(|r| r.name == "main")
        .expect("main listed");
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!(main.ahead, Some(1));
    assert_eq!(main.behind, None);
    // 远程条目自身归到 Remote 类，并带出 remote 名。
    let origin_main = result
        .refs
        .iter()
        .find(|r| r.full_name == "refs/remotes/origin/main")
        .expect("origin/main listed");
    assert_eq!(origin_main.category, ade_git::GitRefCategory::Remote);
    assert_eq!(origin_main.remote.as_deref(), Some("origin"));
}
```

文件头部那行 `use ade_bridge::commands::git::git_refs_impl;` **已有，不要改**；
`ade_git::GitRefCategory` 在断言里用完全限名，因为 `ade-git` 是 `ade-bridge` 的普通依赖（`git.rs` 已在用 `ade_git`），
无需加 import。

Run: `cd src-tauri && cargo test -p ade-bridge --test git_refs 2>&1 | tail -25`
Expected: `2 passed`

- [ ] **Step 2: 再生 TS bindings**

Run: `cd src-tauri && cargo run -p ade-bridge --bin export-bindings`
Expected: 退出 0，`src/bridge/real/generated/tauri-bindings.ts` 被改写

Run: `cd src-tauri && cargo test -p ade-bridge specta_export 2>&1 | tail -15`
Expected: `bindings_are_fresh ... ok`、`command_inventory_and_bindings_are_fresh ... ok`

- [ ] **Step 3: 核对生成的新类型形状**

Run: `grep -n "GitRefEntry_Serialize" -A 14 src/bridge/real/generated/tauri-bindings.ts | head -20`
Expected: 看到 `name: string`、`fullName: string`、`category: "local" | "remote" | "tag"`、
`oid: string`、`isHead: boolean`，以及 `remote?` / `upstream?` / `ahead?` / `behind?` 为可选。
若 `ahead`/`behind` 生成为 `number` 而非 `number | undefined`，说明 `skip_serializing_if` 漏了 —— 回 Task 1 Step 3 修。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/crates/ade-bridge/tests/git_refs.rs src/bridge/real/generated/tauri-bindings.ts
git commit -m "test(ade-bridge): git_refs 契约断言换到 GitRefEntry；再生 tauri bindings"
```

---

## Task 3: TS 共享类型 `git-ref-types.ts`

**Files:**
- Create: `src/shared/git-ref-types.ts`

> 本任务**只新建文件**，不改 `git-inspection-api.ts` / `git-log/types.ts`。
> 那两处换型会打断 `RefPanel.tsx` / `GitLogView.tsx` / `git-log-view.test.tsx` 三个消费点，
> 必须与「删 RefPanel + 改 GitLogView/Toolbar/测试」放进**同一个 commit**（Task 10），
> 否则中间每个 commit 的 `pnpm typecheck` 都是红的 —— 而 typecheck 是每个任务的收尾门禁。

**Interfaces:**
- Consumes: Task 2 Step 4 核对过的生成形状
- Produces:
  - `GitRefCategory = 'local' | 'remote' | 'tag'`
  - `GitRefEntry { name; fullName; category; remote?; oid; isHead; upstream?; ahead?; behind? }`

- [ ] **Step 1: 写 `src/shared/git-ref-types.ts`**

```ts
/**
 * `ade_git::ref_entry` 的 TS 镜像。刻意与 `git-history-types.ts` 的
 * `GitHistoryItemRef`（commit decoration）分开：分支树要 ahead/behind/upstream，
 * 那些字段对 decoration 无意义。
 *
 * 字段名逐字对齐 serde 的 camelCase 输出；改 Rust 侧必须同步这里
 * （`bindings_are_fresh` 只锁生成文件，不锁这份手抄镜像）。
 */
export type GitRefCategory = 'local' | 'remote' | 'tag'

export type GitRefEntry = {
  /** 短名：`main` / `origin/main` / `v1.2.0`。 */
  name: string
  /** 完整 refname：`refs/heads/main`。 */
  fullName: string
  category: GitRefCategory
  /** 仅 `remote` 有值。 */
  remote?: string
  /** commit oid；annotated tag 已 peel 到 commit。 */
  oid: string
  isHead: boolean
  upstream?: string
  /** 相对 upstream 的领先提交数；无 upstream 或 git 未报告时缺省。 */
  ahead?: number
  behind?: number
}
```

- [ ] **Step 2: typecheck 确认新文件干净**

Run: `pnpm typecheck 2>&1 | tail -10`
Expected: 与本任务开始前**完全相同**的输出（新文件未被引用，不产生新错误）

- [ ] **Step 3: Commit**

```bash
git add src/shared/git-ref-types.ts
git commit -m "feat(shared): GitRefEntry 类型镜像（git_refs 换型的前置）"
```

---

## Task 4: `branch-tree-model.ts` —— refs 折成可见行

**Files:**
- Create: `src/renderer/src/components/git-branch/branch-tree-model.ts`
- Test: `src/renderer/src/components/git-branch/branch-tree-model.test.ts`

**Interfaces:**
- Consumes: `GitRefEntry`（`src/shared/git-ref-types.ts`）、`Worktree`（`src/shared/worktree/types.ts`）
- Produces:
  - `BranchTreeRow`（见下）
  - `BranchWorkspaceOwner = { worktreeId: string; displayName: string; path: string }`
  - `buildBranchTreeRows(input: BuildBranchTreeInput): BranchTreeRow[]`
  - `ownerByBranch(worktrees: readonly Worktree[]): Map<string, BranchWorkspaceOwner>`

```ts
export type BranchWorkspaceOwner = {
  worktreeId: string
  displayName: string
  path: string
}

export type BranchTreeRow =
  | {
      kind: 'group'
      id: string
      /** translate key；label 惰性求值（no-top-level-translate 门禁）。 */
      labelKey: string
      labelFallback: string
      depth: number
      expandable: true
      expanded: boolean
      /** 该组在过滤后的条目数，用于组尾计数。 */
      childCount: number
    }
  | {
      kind: 'ref'
      id: string
      depth: number
      ref: GitRefEntry
      /** 当前分支的 upstream（仅 remote 条目可能为真）。 */
      isUpstreamOfHead: boolean
      checkedOutBy: BranchWorkspaceOwner | null
    }
  | {
      kind: 'synthetic'
      id: string
      depth: number
      labelKey: string
      labelFallback: string
      value: 'all-branches' | 'head-current'
    }

export type BuildBranchTreeInput = {
  refs: readonly GitRefEntry[]
  /** 折叠中的组 id；未出现的组视为展开。 */
  collapsed?: ReadonlySet<string>
  /** 当前分支短名（`main`），null = detached/未知。 */
  headBranch?: string | null
  /** 分支短名 → 检出它的 workspace。advisory，见 spec §6.4。 */
  checkedOutByBranch?: ReadonlyMap<string, BranchWorkspaceOwner>
  /** 宿主 B（Git Log）注入 `All Branches` 行；宿主 A 不注入。 */
  includeAllBranchesRow?: boolean
}
```

- [ ] **Step 1: 写失败测试**

Create `src/renderer/src/components/git-branch/branch-tree-model.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'
import { buildBranchTreeRows, ownerByBranch, type BranchTreeRow } from './branch-tree-model'

function ref(partial: Partial<GitRefEntry> & { name: string; fullName: string }): GitRefEntry {
  return {
    category: 'local',
    oid: '0'.repeat(40),
    isHead: false,
    ...partial,
  }
}

const REFS: GitRefEntry[] = [
  ref({ name: 'main', fullName: 'refs/heads/main', isHead: true, ahead: 14 }),
  ref({ name: 'feature/login', fullName: 'refs/heads/feature/login' }),
  ref({ name: 'origin/main', fullName: 'refs/remotes/origin/main', category: 'remote', remote: 'origin' }),
  ref({ name: 'origin/dev', fullName: 'refs/remotes/origin/dev', category: 'remote', remote: 'origin' }),
  ref({ name: 'upstream/main', fullName: 'refs/remotes/upstream/main', category: 'remote', remote: 'upstream' }),
  ref({ name: 'v1.2.0', fullName: 'refs/tags/v1.2.0', category: 'tag' }),
]

const shape = (rows: ReturnType<typeof buildBranchTreeRows>) =>
  rows.map((r) =>
    r.kind === 'group' ? `${'  '.repeat(r.depth)}[${r.id}${r.expanded ? '' : '!'}` :
    r.kind === 'synthetic' ? `${'  '.repeat(r.depth)}${r.value}` :
    `${'  '.repeat(r.depth)}${r.ref.name}`
  )

/** 取可见 ref 行的短名（带 kind 窄化，避开逐处 `as never as` 强转）。 */
function refNames(rows: ReturnType<typeof buildBranchTreeRows>, depth?: number): string[] {
  return rows
    .filter((r): r is Extract<BranchTreeRow, { kind: 'ref' }> =>
      r.kind === 'ref' && (depth === undefined || r.depth === depth))
    .map((r) => r.ref.name)
}

/** 取带 checkedOutBy / isUpstreamOfHead 的原始 ref 行。 */
function refRows(rows: ReturnType<typeof buildBranchTreeRows>) {
  return rows.filter((r): r is Extract<BranchTreeRow, { kind: 'ref' }> => r.kind === 'ref')
}

describe('buildBranchTreeRows', () => {
  it('按 IDEA 组序：Local → Remote（按 remote 二级分组）→ Tags', () => {
    expect(shape(buildBranchTreeRows({ refs: REFS }))).toEqual([
      '[group:local',
      '  main',
      '  feature/login',
      '[group:remote',
      '  [group:remote/origin',
      '    origin/dev',
      '    origin/main',
      '  [group:remote/upstream',
      '    upstream/main',
      '[group:tags',
      '  v1.2.0',
    ])
  })

  it('Local 组排除当前分支（由 HEAD 合成行代表），其余字典序；remote 内按去前缀名排序', () => {
    const rows = buildBranchTreeRows({ refs: REFS })
    // spec §7.2 的「当前分支置顶」由 head-current 合成行实现，
    // 所以 Local 组内**不应**再出现 main —— 只剩 feature/login。
    expect(refNames(rows, 1)).toEqual(['feature/login'])
    expect(refNames(rows, 2)).toEqual(['origin/dev', 'origin/main', 'upstream/main'])
  })

  it('折叠的组只出行头，不出子行；childCount 仍是过滤后的条数', () => {
    const rows = buildBranchTreeRows({ refs: REFS, collapsed: new Set(['group:remote']) })
    const groups = rows.filter((r) => r.kind === 'group')
    expect(rows.some((r) => r.kind === 'ref')).toBe(true)
    expect(refNames(rows).filter((n) => n.startsWith('origin/') || n.startsWith('upstream/'))).toEqual([])
    const remote = groups.find((g) => g.id === 'group:remote')!
    expect(remote.expanded).toBe(false)
    expect(remote.childCount).toBe(3)
  })

  it('空组不出行头', () => {
    const rows = buildBranchTreeRows({ refs: REFS.filter((r) => r.category !== 'tag') })
    expect(rows.some((r) => r.kind === 'group' && r.id === 'group:tags')).toBe(false)
  })

  it('is_head 条目合成 HEAD (Current Branch) 首行；includeAllBranchesRow 追加 All Branches', () => {
    const rows = buildBranchTreeRows({ refs: REFS, includeAllBranchesRow: true })
    expect(rows.slice(0, 2).map((r) => (r.kind === 'synthetic' ? r.value : '?!'))).toEqual([
      'all-branches',
      'head-current',
    ])
    // HEAD 行不重复出现在 Local 组里。
    expect(refNames(rows)).not.toContain('main')
  })

  it('isUpstreamOfHead 只标在当前分支的 upstream 那一条', () => {
    const rows = buildBranchTreeRows({
      refs: REFS.map((r) => (r.name === 'main' ? { ...r, upstream: 'origin/main' } : r)),
      headBranch: 'main',
    })
    expect(refRows(rows).filter((r) => r.isUpstreamOfHead).map((r) => r.ref.name)).toEqual([
      'origin/main',
    ])
  })

  it('ownerByBranch：跳过无 branch 的条目；同分支多 worktree 取第一个', () => {
    const worktrees = [
      { id: 'r::/a', branch: 'main', displayName: 'A', path: '/a' },
      { id: 'r::/b', branch: 'feature/login', displayName: 'B', path: '/b' },
      { id: 'r::/c', branch: '', displayName: 'C', path: '/c' },
    ] as unknown as Worktree[]
    const map = ownerByBranch(worktrees)
    expect([...map.keys()]).toEqual(['main', 'feature/login'])
    expect(map.get('main')).toEqual({ worktreeId: 'r::/a', displayName: 'A', path: '/a' })
  })

  it('checkedOutByBranch 落到 ref 行的 checkedOutBy', () => {
    const owners = new Map([['feature/login', { worktreeId: 'r::/b', displayName: 'B', path: '/b' }]])
    const rows = buildBranchTreeRows({ refs: REFS, checkedOutByBranch: owners })
    const hit = refRows(rows).find((r) => r.ref.name === 'feature/login')!
    expect(hit.checkedOutBy).toEqual({ worktreeId: 'r::/b', displayName: 'B', path: '/b' })
    // 只有本地分支能带 owner；remote/tag 行即使同名也置 null。
    expect(refRows(rows).find((r) => r.ref.category === 'remote')!.checkedOutBy).toBeNull()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-tree-model.test.ts 2>&1 | tail -15`
Expected: FAIL —— 找不到模块 `./branch-tree-model`

- [ ] **Step 3: 实现 `branch-tree-model.ts`**

```ts
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'

export type BranchWorkspaceOwner = {
  worktreeId: string
  displayName: string
  path: string
}

export type BranchTreeRow =
  | {
      kind: 'group'
      id: string
      labelKey: string
      labelFallback: string
      depth: number
      expandable: true
      expanded: boolean
      childCount: number
    }
  | {
      kind: 'ref'
      id: string
      depth: number
      ref: GitRefEntry
      isUpstreamOfHead: boolean
      checkedOutBy: BranchWorkspaceOwner | null
    }
  | {
      kind: 'synthetic'
      id: string
      depth: number
      labelKey: string
      labelFallback: string
      value: 'all-branches' | 'head-current'
    }

export const GROUP_LOCAL = 'group:local'
export const GROUP_REMOTE = 'group:remote'
export const GROUP_TAGS = 'group:tags'
export const ROW_HEAD = 'row:head-current'
export const ROW_ALL = 'row:all-branches'

export const GROUP_LABELS: Record<string, { key: string; fallback: string }> = {
  [GROUP_LOCAL]: { key: 'auto.components.git-branch.groupLocal', fallback: 'Local' },
  [GROUP_REMOTE]: { key: 'auto.components.git-branch.groupRemote', fallback: 'Remote' },
  [GROUP_TAGS]: { key: 'auto.components.git-branch.groupTags', fallback: 'Tags' },
}

export const SYNTHETIC_LABELS: Record<'all-branches' | 'head-current', { key: string; fallback: string }> = {
  'all-branches': { key: 'auto.components.git-branch.allBranches', fallback: 'All Branches' },
  'head-current': { key: 'auto.components.git-branch.headCurrent', fallback: 'HEAD (Current Branch)' },
}

export type BuildBranchTreeInput = {
  refs: readonly GitRefEntry[]
  collapsed?: ReadonlySet<string>
  headBranch?: string | null
  checkedOutByBranch?: ReadonlyMap<string, BranchWorkspaceOwner>
  includeAllBranchesRow?: boolean
}

/** 组 id → 该组的 ref 列表（Remote 组再按 remote 二级分组）。 */
type Bucket = { id: string; label: { key: string; fallback: string }; refs: GitRefEntry[] }

function compareName(a: GitRefEntry, b: GitRefEntry): number {
  // Remote 条目按去掉 `<remote>/` 前缀的名字比，否则 `origin/a` 会排到 `origin/b` 之后。
  const an = a.category === 'remote' ? a.name.replace(/^[^/]+\//, '') : a.name
  const bn = b.category === 'remote' ? b.name.replace(/^[^/]+\//, '') : b.name
  return an.localeCompare(bn)
}

function bucketize(refs: readonly GitRefEntry[]): Bucket[] {
  const local = refs.filter((r) => r.category === 'local')
  const remote = refs.filter((r) => r.category === 'remote')
  const tags = refs.filter((r) => r.category === 'tag')
  const buckets: Bucket[] = []
  if (local.length) buckets.push({ id: GROUP_LOCAL, label: GROUP_LABELS[GROUP_LOCAL]!, refs: local })
  if (remote.length) buckets.push({ id: GROUP_REMOTE, label: GROUP_LABELS[GROUP_REMOTE]!, refs: remote })
  if (tags.length) buckets.push({ id: GROUP_TAGS, label: GROUP_LABELS[GROUP_TAGS]!, refs: tags })
  return buckets
}

function byRemote(refs: GitRefEntry[]): Array<[string, GitRefEntry[]]> {
  const out = new Map<string, GitRefEntry[]>()
  for (const r of refs) {
    const key = r.remote ?? r.name.split('/')[0] ?? ''
    ;(out.get(key) ?? out.set(key, []).get(key)!).push(r)
  }
  return [...out.entries()].sort(([a], [b]) => a.localeCompare(b))
}

/** 当前分支的 upstream 短名（`origin/main`），用于 `★` 标记。 */
function headUpstreamName(refs: readonly GitRefEntry[], headBranch?: string | null): string | null {
  if (!headBranch) return null
  return refs.find((r) => r.category === 'local' && r.name === headBranch)?.upstream ?? null
}

export function ownerByBranch(
  worktrees: readonly Worktree[]
): Map<string, BranchWorkspaceOwner> {
  const out = new Map<string, BranchWorkspaceOwner>()
  for (const wt of worktrees) {
    if (!wt.branch || out.has(wt.branch)) continue
    out.set(wt.branch, {
      worktreeId: wt.id,
      displayName: wt.displayName,
      path: wt.path,
    })
  }
  return out
}

/**
 * refs → 可见行列表（折叠态已应用，可直接渲染与 ↑↓ 导航）。
 *
 * 刻意返回扁平行而非嵌套树：键盘导航与 `data-testid` 定位都要遍历可见行，
 * 扁平结构让「折叠即不可见」成为单一事实源。
 */
export function buildBranchTreeRows(input: BuildBranchTreeInput): BranchTreeRow[] {
  const {
    refs,
    collapsed = new Set<string>(),
    headBranch = null,
    checkedOutByBranch = new Map(),
    includeAllBranchesRow = false,
  } = input

  const head = refs.find((r) => r.isHead) ?? null
  const upstream = headUpstreamName(refs, headBranch)
  const rows: BranchTreeRow[] = []

  if (includeAllBranchesRow) {
    rows.push({
      kind: 'synthetic',
      id: ROW_ALL,
      depth: 0,
      ...SYNTHETIC_LABELS['all-branches'],
      value: 'all-branches',
    })
  }
  if (head) {
    rows.push({
      kind: 'synthetic',
      id: ROW_HEAD,
      depth: 0,
      ...SYNTHETIC_LABELS['head-current'],
      value: 'head-current',
    })
  }

  // HEAD 行已代表当前分支，Local 组里不再重复出它。
  const rest = refs.filter((r) => r !== head)

  for (const bucket of bucketize(rest)) {
    const expanded = !collapsed.has(bucket.id)
    if (bucket.id !== GROUP_REMOTE) {
      rows.push({
        kind: 'group',
        id: bucket.id,
        labelKey: bucket.label.key,
        labelFallback: bucket.label.fallback,
        depth: 0,
        expandable: true,
        expanded,
        childCount: bucket.refs.length,
      })
      if (!expanded) continue
      for (const r of [...bucket.refs].sort(compareName)) {
        rows.push(refRow(r, 1, upstream, checkedOutByBranch))
      }
      continue
    }

    // Remote：一级组头 + 每个 remote 一个二级组头。折叠一级即抹掉全部二级内容。
    rows.push({
      kind: 'group',
      id: bucket.id,
      labelKey: bucket.label.key,
      labelFallback: bucket.label.fallback,
      depth: 0,
      expandable: true,
      expanded,
      childCount: bucket.refs.length,
    })
    if (!expanded) continue
    for (const [remote, remoteRefs] of byRemote(bucket.refs)) {
      const subId = `${GROUP_REMOTE}/${remote}`
      const subExpanded = !collapsed.has(subId)
      rows.push({
        kind: 'group',
        id: subId,
        labelKey: subId,
        labelFallback: remote,
        depth: 1,
        expandable: true,
        expanded: subExpanded,
        childCount: remoteRefs.length,
      })
      if (!subExpanded) continue
      for (const r of [...remoteRefs].sort(compareName)) {
        rows.push(refRow(r, 2, upstream, checkedOutByBranch))
      }
    }
  }

  return rows
}

function refRow(
  r: GitRefEntry,
  depth: number,
  headUpstream: string | null,
  owners: ReadonlyMap<string, BranchWorkspaceOwner>
): BranchTreeRow {
  return {
    kind: 'ref',
    id: `ref:${r.fullName}`,
    depth,
    ref: r,
    isUpstreamOfHead: headUpstream !== null && r.name === headUpstream,
    // 只有本地分支可能被 workspace 检出（远程分支/标签的 owner 语义不成立）。
    checkedOutBy: r.category === 'local' ? owners.get(r.name) ?? null : null,
  }
}
```

- [ ] **Step 4: 跑测试**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-tree-model.test.ts 2>&1 | tail -20`
Expected: 全绿。若 `byRemote` 的 `out.set(key, []).get(key)!` 写法被 lint 拦，改成
`if (!out.has(key)) out.set(key, []); out.get(key)!.push(r)`。

- [ ] **Step 5: Commit**

```bash
git add src/renderer/src/components/git-branch/
git commit -m "feat(git-branch): 分支树模型 refs→可见行（IDEA 组序/二级 remote/HEAD 合成行）"
```

---

## Task 5: `branch-tree-filter.ts` —— 搜索匹配

**Files:**
- Create: `src/renderer/src/components/git-branch/branch-tree-filter.ts`
- Test: `src/renderer/src/components/git-branch/branch-tree-filter.test.ts`

**Interfaces:**
- Consumes: `GitRefEntry`（Task 3）
- Produces: `normalizeBranchQuery(query: string): string`、`matchesBranchQuery(ref: GitRefEntry, query: string): boolean`

- [ ] **Step 1: 写失败测试**

Create `src/renderer/src/components/git-branch/branch-tree-filter.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { matchesBranchQuery, normalizeBranchQuery } from './branch-tree-filter'

const entry = (name: string, over: Partial<GitRefEntry> = {}): GitRefEntry => ({
  name,
  fullName: `refs/heads/${name}`,
  category: 'local',
  oid: '0'.repeat(40),
  isHead: false,
  ...over,
})

describe('normalizeBranchQuery', () => {
  it('trim + 小写', () => {
    expect(normalizeBranchQuery('  FeAture/Login ')).toBe('feature/login')
  })
})

describe('matchesBranchQuery', () => {
  it('空查询命中一切（含全空格）', () => {
    expect(matchesBranchQuery(entry('main'), '')).toBe(true)
    expect(matchesBranchQuery(entry('main'), '   ')).toBe(true)
  })

  it('子串命中：中段、尾段、大小写无关', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'log')).toBe(true)
    expect(matchesBranchQuery(entry('feature/login'), 'LOGIN')).toBe(true)
    expect(matchesBranchQuery(entry('feature/login'), 'eat')).toBe(true)
  })

  it('远程条目可按裸分支名搜（origin/main 命中 "main"）', () => {
    expect(
      matchesBranchQuery(
        entry('origin/main', { category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
        'main'
      )
    ).toBe(true)
  })

  it('段前缀命中：f/log → feature/login（子串规则做不到，是本函数的增量）', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'f/log')).toBe(true)
    expect(matchesBranchQuery(entry('feature/login'), 'feat/i')).toBe(true)
  })

  it('段前缀必须对齐尾部段数：x/log 不命中 feature/login', () => {
    expect(matchesBranchQuery(entry('feature/login'), 'x/log')).toBe(false)
  })

  it('段数多于条目段数不命中', () => {
    expect(matchesBranchQuery(entry('main'), 'a/b/c/main')).toBe(false)
  })

  it('不命中时返回 false', () => {
    expect(matchesBranchQuery(entry('main'), 'zzz')).toBe(false)
  })
})
```

> 段前缀测试（`f/log` → `feature/login`）是本函数区别于「一行 `includes`」的**唯一理由**；
> 若实现只剩 `includes`，那两个用例必红 —— 不要为了让测试过而删掉它们。

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-tree-filter.test.ts 2>&1 | tail -12`
Expected: FAIL —— 找不到模块 `./branch-tree-filter`

- [ ] **Step 3: 实现 `branch-tree-filter.ts`**

```ts
import type { GitRefEntry } from '../../../../shared/git-ref-types'

export function normalizeBranchQuery(query: string): string {
  return query.trim().toLowerCase()
}

/**
 * 两条规则，缺一不可：
 *  1. 子串 —— `eat` 命中 `feature/login`（IDEA 的宽松匹配）。
 *  2. 段前缀 —— `f/log` 命中 `feature/login`。子串规则做不到（`f/log` 不是连续子串），
 *     而这是长层级分支名（`team/feature/foo`）的主要检索方式。
 *
 * 刻意不做 fuzzy / camelHump：误报率高于收益，且与 IDEA 的实际行为不必强行一致。
 */
export function matchesBranchQuery(ref: GitRefEntry, query: string): boolean {
  const needle = normalizeBranchQuery(query)
  if (!needle) return true
  const name = ref.name.toLowerCase()
  if (name.includes(needle)) return true
  return segmentPrefixMatch(name, needle.split('/').filter(Boolean))
}

function segmentPrefixMatch(name: string, needleSegments: readonly string[]): boolean {
  if (needleSegments.length === 0) return false
  const parts = name.split('/')
  if (parts.length < needleSegments.length) return false
  const offset = parts.length - needleSegments.length
  return needleSegments.every((seg, i) => parts[offset + i]!.startsWith(seg))
}
```

- [ ] **Step 4: 跑测试**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-tree-filter.test.ts 2>&1 | tail -15`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add src/renderer/src/components/git-branch/
git commit -m "feat(git-branch): 分支搜索匹配（子串 + 段前缀）"
```

---

## Task 6: `branch-ref-tree.tsx` —— 纯呈现树 + locale key

**Files:**
- Create: `src/renderer/src/components/git-branch/branch-ref-tree.tsx`
- Test: `src/renderer/src/components/git-branch/branch-ref-tree.test.tsx`
- Modify: `src/renderer/src/i18n/locales/en.json`、`zh.json`

**Interfaces:**
- Consumes: `buildBranchTreeRows` / `BranchTreeRow` / `BranchWorkspaceOwner`（Task 4）、`matchesBranchQuery`（Task 5）、`ui/popover` 无关（本组件不含 popover 壳）
- Produces:
  - `BranchRefTree(props: BranchRefTreeProps): React.JSX.Element`
  - `BranchRefTreeProps = { refs; truncated?; headBranch?; checkedOutByBranch?; includeAllBranchesRow?; activeValue?; onActivate; className? }`
  - `branchRowTestId(row): string`（测试定位约定：`branch-row-<id 里的 : 和 / 换成 ->`）

- [ ] **Step 1: 加 locale key（en + zh）**

`src/renderer/src/i18n/locales/en.json` 的 `auto.components` 下新增 `git-branch` 节点（与 `git-log` 同级）：

```json
"git-branch": {
  "searchPlaceholder": "Branch or tag",
  "groupLocal": "Local",
  "groupRemote": "Remote",
  "groupTags": "Tags",
  "allBranches": "All Branches",
  "headCurrent": "HEAD (Current Branch)",
  "refsTruncated": "Some refs are not shown (list limit reached).",
  "empty": "No matching branches.",
  "loading": "Loading branches…",
  "loadFailed": "Failed to read branches.",
  "aheadBehind": "↑{{ahead}} ↓{{behind}}",
  "aheadBehindLabel": "{{ahead}} ahead, {{behind}} behind",
  "aheadOnlyLabel": "{{ahead}} ahead",
  "behindOnlyLabel": "{{behind}} behind",
  "inSyncLabel": "in sync with {{upstream}}",
  "checkedOutIn": "checked out in {{workspace}}",
  "upstreamOfHead": "upstream of {{branch}}",
  "switchTo": "Switch to this branch",
  "newWorkspaceFrom": "New Workspace from Branch…",
  "viewInGitLog": "View in Git Log"
}
```

`zh.json` 同位置：

```json
"git-branch": {
  "searchPlaceholder": "分支或标签",
  "groupLocal": "本地",
  "groupRemote": "远程",
  "groupTags": "标签",
  "allBranches": "全部分支",
  "headCurrent": "HEAD（当前分支）",
  "refsTruncated": "部分引用未显示（已达列表上限）。",
  "empty": "无匹配分支。",
  "loading": "正在加载分支…",
  "loadFailed": "读取分支列表失败。",
  "aheadBehind": "↑{{ahead}} ↓{{behind}}",
  "aheadBehindLabel": "领先 {{ahead}}，落后 {{behind}}",
  "aheadOnlyLabel": "领先 {{ahead}}",
  "behindOnlyLabel": "落后 {{behind}}",
  "inSyncLabel": "与 {{upstream}} 同步",
  "checkedOutIn": "已在「{{workspace}}」检出",
  "upstreamOfHead": "{{branch}} 的上游",
  "switchTo": "切换到该分支",
  "newWorkspaceFrom": "从该分支新建工作区…",
  "viewInGitLog": "在 Git 日志中查看"
}
```

> es/fr/ja/ko **不加** —— `translate(key, fallback)` 的 `defaultValue` 兜底，
> 与 `auto.components.git-log` 的既有先例一致（Global Constraints）。

- [ ] **Step 2: 写失败测试**

Create `src/renderer/src/components/git-branch/branch-ref-tree.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from './branch-ref-tree'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

function entry(over: Partial<GitRefEntry> & { name: string }): GitRefEntry {
  return {
    fullName: `refs/heads/${over.name}`,
    category: 'local',
    oid: '0'.repeat(40),
    isHead: false,
    ...over,
  }
}

const REFS: GitRefEntry[] = [
  entry({ name: 'main', isHead: true, ahead: 2, behind: 1, upstream: 'origin/main' }),
  entry({ name: 'feature/login' }),
  entry({ name: 'origin/main', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
  entry({ name: 'v1.2.0', category: 'tag', fullName: 'refs/tags/v1.2.0' }),
]

let host: HTMLDivElement
let root: Root
let activated: string[]

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  activated = []
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
})

const render = async (props: Partial<Parameters<typeof BranchRefTree>[0]> = {}) => {
  await act(async () => {
    root.render(
      <BranchRefTree
        refs={REFS}
        headBranch="main"
        onActivate={(row) => activated.push(row.id)}
        {...props}
      />
    )
  })
  await act(async () => {
    await Promise.resolve()
  })
}

const row = (id: string): HTMLElement | null => host.querySelector(`[data-testid="branch-row-${id}"]`)
const press = async (el: HTMLElement, key: string): Promise<void> => {
  await act(async () => {
    el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  })
}
const click = async (el: Element | null): Promise<void> => {
  await act(async () => {
    el?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
}
/** happy-dom 下受控 input 必须走原生 value setter，否则 React 收不到 onChange。 */
const typeSearch = async (value: string): Promise<void> => {
  const input = host.querySelector('input[type="search"]')!
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
  await act(async () => {
    setter?.call(input, value)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

describe('BranchRefTree', () => {
  it('渲染合成人物与三组；当前分支不在 Local 组里重复', async () => {
    await render()
    expect(row('row:head-current')).not.toBeNull()
    expect(host.textContent).toContain('feature/login')
    expect(host.textContent).toContain('v1.2.0')
    expect(row('ref:refs/heads/main')).toBeNull()
  })

  it('includeAllBranchesRow 追加 All Branches 且排在 HEAD 之前', async () => {
    await render({ includeAllBranchesRow: true })
    const ids = [...host.querySelectorAll('[data-testid^="branch-row-"]')].map(
      (el) => el.getAttribute('data-testid')
    )
    expect(ids.slice(0, 2)).toEqual(['branch-row-row:all-branches', 'branch-row-row:head-current'])
  })

  it('ahead/behind 计数出现在行尾', async () => {
    await render()
    const head = row('row:head-current')!
    expect(head.textContent).toContain('↑2')
    expect(head.textContent).toContain('↓1')
  })

  it('checkedOutByBranch 给行加 workspace 徽标（tooltip 文案含 displayName）', async () => {
    await render({
      checkedOutByBranch: new Map([
        ['feature/login', { worktreeId: 'r::/b', displayName: 'B', path: '/b' }],
      ]),
    })
    const el = row('ref:refs/heads/feature/login')!
    expect(el.querySelector('[data-testid="branch-workspace-badge"]')).not.toBeNull()
    expect(el.getAttribute('title')).toContain('B')
  })

  it('isUpstreamOfHead 给远程行加 ★', async () => {
    await render()
    const el = row('ref:refs/remotes/origin/main')!
    expect(el.querySelector('[data-testid="branch-upstream-star"]')).not.toBeNull()
  })

  it('搜索按 matchesBranchQuery 裁剪；无命中出行内空态而非空面板', async () => {
    await render()
    await typeSearch('login')
    expect(host.textContent).toContain('feature/login')
    expect(host.textContent).not.toContain('v1.2.0')
    await typeSearch('zzz')
    expect(host.textContent).toContain('No matching branches.')
  })

  it('折叠 Remote 组头：子行消失，再点恢复；折叠态不因搜索而丢失', async () => {
    await render()
    await click(row('group:remote'))
    expect(row('ref:refs/remotes/origin/main')).toBeNull()
    await typeSearch('login')
    await typeSearch('')
    expect(row('ref:refs/remotes/origin/main')).toBeNull() // 仍折叠
    await click(row('group:remote'))
    expect(row('ref:refs/remotes/origin/main')).not.toBeNull()
  })

  it('单击 ref 行 → onActivate(row)；单击组头不触发 onActivate', async () => {
    await render()
    await click(row('ref:refs/heads/feature/login'))
    expect(activated).toEqual(['ref:refs/heads/feature/login'])
    await click(row('group:tags'))
    expect(activated).toHaveLength(1)
  })

  it('truncated=true 出截断提示，false/缺省不出', async () => {
    await render({ truncated: true })
    expect(host.textContent).toContain('Some refs are not shown')
    await render({ truncated: false })
    expect(host.textContent).not.toContain('Some refs are not shown')
  })

  it('空 refs 出行内空态', async () => {
    await render({ refs: [] })
    expect(host.textContent).toContain('No matching branches.')
  })

  it('搜索框里按 ↓ 进第一行；↑↓ 在可见行间移动焦点', async () => {
    await render()
    const input = host.querySelector('input[type="search"]')!
    await press(input, 'ArrowDown')
    expect((document.activeElement as HTMLElement)?.dataset.rowIndex).toBe('0')
    await press(document.activeElement as HTMLElement, 'ArrowDown')
    expect((document.activeElement as HTMLElement)?.dataset.rowIndex).toBe('1')
    await press(document.activeElement as HTMLElement, 'ArrowUp')
    expect((document.activeElement as HTMLElement)?.dataset.rowIndex).toBe('0')
  })

  it('→ 展开、← 折叠当前组，与单击组头共用同一份 collapsed state', async () => {
    await render()
    await click(row('group:remote')!)
    expect(row('ref:refs/remotes/origin/main')).toBeNull()
    await press(row('group:remote')!, 'ArrowRight')
    expect(row('ref:refs/remotes/origin/main')).not.toBeNull()
    await press(row('group:remote')!, 'ArrowLeft')
    expect(row('ref:refs/remotes/origin/main')).toBeNull()
  })

  // Enter 不在此测：happy-dom 下 KeyboardEvent('Enter') 不会在 <button> 上合成 click，
  // 真浏览器的行为由「单击 ref 行 → onActivate」那条用例等价覆盖。
})
```

- [ ] **Step 3: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-ref-tree.test.tsx 2>&1 | tail -12`
Expected: FAIL —— 找不到模块 `./branch-ref-tree`

- [ ] **Step 4: 实现 `branch-ref-tree.tsx`**

```tsx
import { useCallback, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import {
  buildBranchTreeRows,
  type BranchTreeRow,
  type BranchWorkspaceOwner,
} from './branch-tree-model'
import { matchesBranchQuery } from './branch-tree-filter'

/** 行 id 里的 `:` `/` 原样进 data-testid（querySelector 侧用字面量拼，无需转义）。 */
export function branchRowTestId(row: BranchTreeRow): string {
  return `branch-row-${row.id}`
}

export type BranchRefTreeProps = {
  refs: readonly GitRefEntry[]
  /** 后端截断过 ref 列表时出行内提示。 */
  truncated?: boolean
  headBranch?: string | null
  checkedOutByBranch?: ReadonlyMap<string, BranchWorkspaceOwner>
  /** Git Log 宿主注入 `All Branches`；Source Control 宿主不注入。 */
  includeAllBranchesRow?: boolean
  /** 高亮行 id（合成人物用 `row:*`，ref 用 `ref:<fullName>`）。 */
  activeValue?: string | null
  onActivate: (row: BranchTreeRow) => void
  className?: string
}

export function BranchRefTree({
  refs,
  truncated = false,
  headBranch = null,
  checkedOutByBranch,
  includeAllBranchesRow = false,
  activeValue = null,
  onActivate,
  className,
}: BranchRefTreeProps): React.JSX.Element {
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const containerRef = useRef<HTMLDivElement>(null)

  const visibleRefs = useMemo(
    () => (query.trim() ? refs.filter((r) => matchesBranchQuery(r, query)) : [...refs]),
    [refs, query]
  )

  const rows = useMemo(
    () =>
      buildBranchTreeRows({
        refs: visibleRefs,
        collapsed,
        headBranch,
        checkedOutByBranch,
        includeAllBranchesRow,
      }),
    [visibleRefs, collapsed, headBranch, checkedOutByBranch, includeAllBranchesRow]
  )

  const toggleGroup = useCallback((id: string) => {
    setCollapsed((cur) => {
      const next = new Set(cur)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const focusRow = useCallback((index: number) => {
    const el = containerRef.current?.querySelector<HTMLElement>(
      `[data-row-index="${index}"]`
    )
    el?.focus()
  }, [])

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp' &&
          event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') return
      const target = event.target as HTMLElement
      const indexed = Number(target.dataset.rowIndex)
      const current = Number.isNaN(indexed) ? -1 : indexed

      // 焦点在搜索框（无 data-row-index）时，↓ 进首行 —— 弹层打开即聚焦搜索框，
      // 不能要求用户先鼠标点一下才能键盘导航。
      if (current === -1) {
        if (event.key !== 'ArrowDown') return
        event.preventDefault()
        focusRow(0)
        return
      }

      event.preventDefault()
      if (event.key === 'ArrowDown') focusRow(Math.min(rows.length - 1, current + 1))
      else if (event.key === 'ArrowUp') focusRow(Math.max(0, current - 1))
      else {
        const row = rows[current]
        if (row?.kind !== 'group') return
        // 与单击组头共用同一份 collapsed state：两条路径必须互逆，否则会出现
        // 「单击折叠、←→ 无法展开」这类只有真键盘才暴露的偏差。
        const open = event.key === 'ArrowRight'
        setCollapsed((cur) => {
          const next = new Set(cur)
          if (open) next.delete(row.id)
          else next.add(row.id)
          return next
        })
      }
    },
    [rows, focusRow]
  )

  return (
    <div ref={containerRef} data-testid="branch-tree" className={cn('flex flex-col', className)}>
      <label className="mb-1 flex items-center gap-1.5 rounded border border-input px-1.5 py-0.5">
        <Search className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
        <input
          type="search"
          value={query}
          autoFocus
          onChange={(event) => setQuery(event.target.value)}
          placeholder={translate(
            'auto.components.git-branch.searchPlaceholder',
            'Branch or tag'
          )}
          className="min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground/60"
        />
      </label>
      {truncated && (
        <p className="px-1 pb-1 text-[11px] leading-4 text-muted-foreground">
          {translate(
            'auto.components.git-branch.refsTruncated',
            'Some refs are not shown (list limit reached).'
          )}
        </p>
      )}
      {rows.length === 0 ? (
        <p className="px-1 py-1 text-xs text-muted-foreground">
          {translate('auto.components.git-branch.empty', 'No matching branches.')}
        </p>
      ) : (
        <div role="tree" className="max-h-72 overflow-auto outline-none">
          {rows.map((row, index) => (
            <BranchRowButton
              key={row.id}
              row={row}
              index={index}
              active={activeValue === row.id}
              onActivate={onActivate}
              onToggleGroup={toggleGroup}
            />
          ))}
        </div>
      )}
    </div>
  )
}

function BranchRowButton({
  row,
  index,
  active,
  onActivate,
  onToggleGroup,
}: {
  row: BranchTreeRow
  index: number
  active: boolean
  onActivate: (row: BranchTreeRow) => void
  onToggleGroup: (id: string) => void
}): React.JSX.Element {
  const indent = { paddingLeft: `${row.depth * 12 + 4}px` }
  const shared =
    'flex w-full items-center gap-1.5 truncate rounded px-1 py-0.5 text-left text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring'
  const onClick =
    row.kind === 'group' ? () => onToggleGroup(row.id) : () => onActivate(row)

  return (
    <button
      type="button"
      role="treeitem"
      aria-selected={active}
      aria-expanded={row.kind === 'group' ? row.expanded : undefined}
      data-testid={branchRowTestId(row)}
      data-row-index={index}
      title={rowTitle(row)}
      onClick={onClick}
      className={cn(shared, active ? 'bg-accent text-foreground' : 'hover:bg-accent/40')}
      style={indent}
    >
      <RowLeading row={row} />
      <span className="min-w-0 flex-1 truncate">{rowLabel(row)}</span>
      <RowTrailing row={row} />
    </button>
  )
}

/**
 * 图标映射是本设计新定义的（spec §7.1），不是「IDEA 就是这样」：
 *  🏷 = git 报告的 HEAD 分支；★ = 当前分支的 upstream；▣ = 被某 ade workspace 检出。
 * 用字符而非 lucide 图标：分支树行高 20px，lucide 在 text-xs 下视觉重量过大。
 */
function RowLeading({ row }: { row: BranchTreeRow }): React.JSX.Element | null {
  if (row.kind === 'group') {
    return (
      <span aria-hidden="true" className="w-3 shrink-0 text-center text-muted-foreground">
        {row.expanded ? '⌄' : '›'}
      </span>
    )
  }
  if (row.kind === 'synthetic') {
    return <span aria-hidden="true" className="w-3 shrink-0 text-center">◉</span>
  }
  if (row.ref.isHead) {
    return <span aria-hidden="true" className="w-3 shrink-0 text-center">🏷</span>
  }
  if (row.isUpstreamOfHead) {
    return (
      <span aria-hidden="true" data-testid="branch-upstream-star" className="w-3 shrink-0 text-center">
        ★
      </span>
    )
  }
  if (row.ref.category === 'tag') {
    return <span aria-hidden="true" className="w-3 shrink-0 text-center">🏷</span>
  }
  return <span aria-hidden="true" className="w-3 shrink-0" />
}

function RowTrailing({ row }: { row: BranchTreeRow }): React.JSX.Element | null {
  if (row.kind !== 'ref') return null
  const { ahead, behind } = row.ref
  const badge = row.checkedOutBy ? (
    <span
      data-testid="branch-workspace-badge"
      className="shrink-0 text-[10px] text-muted-foreground"
      aria-hidden="true"
    >
      ▣
    </span>
  ) : null
  if (ahead == null && behind == null) return badge
  return (
    <>
      {badge}
      <span className="shrink-0 tabular-nums text-[10px] text-muted-foreground" aria-hidden="true">
        {translate('auto.components.git-branch.aheadBehind', '↑{{ahead}} ↓{{behind}}', {
          ahead: ahead ?? 0,
          behind: behind ?? 0,
        })}
      </span>
    </>
  )
}

function rowLabel(row: BranchTreeRow): string {
  if (row.kind === 'group') {
    // remote 二级分组的 labelKey 就是 remote 名本身（`group:remote/origin`），不走 translate。
    return row.labelKey.startsWith('group:remote/')
      ? row.labelFallback
      : translate(row.labelKey, row.labelFallback)
  }
  if (row.kind === 'synthetic') return translate(row.labelKey, row.labelFallback)
  return row.ref.name
}

function rowTitle(row: BranchTreeRow): string | undefined {
  if (row.kind === 'ref' && row.checkedOutBy) {
    return translate('auto.components.git-branch.checkedOutIn', 'checked out in {{workspace}}', {
      workspace: row.checkedOutBy.displayName,
    })
  }
  if (row.kind === 'ref') {
    const { ahead, behind, upstream } = row.ref
    if (ahead == null && behind == null) return upstream ?? undefined
    if (ahead != null && behind != null) {
      return translate('auto.components.git-branch.aheadBehindLabel', '{{ahead}} ahead, {{behind}} behind', {
        ahead,
        behind,
      })
    }
    if (ahead != null) {
      return translate('auto.components.git-branch.aheadOnlyLabel', '{{ahead}} ahead', { ahead })
    }
    return translate('auto.components.git-branch.behindOnlyLabel', '{{behind}} behind', {
      behind: behind ?? 0,
    })
  }
  if (row.kind === 'group') return `${row.labelFallback} (${row.childCount})`
  return undefined
}
```

- [ ] **Step 5: 跑测试**

Run: `pnpm vitest run src/renderer/src/components/git-branch/ 2>&1 | tail -25`
Expected: 全绿（含 Task 4/5 的模型与查询测试）

- [ ] **Step 6: 过 i18n AST 门禁与 typecheck**

Run: `pnpm vitest run src/renderer/src/i18n/no-top-level-translate.test.ts 2>&1 | tail -10 && pnpm typecheck 2>&1 | tail -10`
Expected: 两者均绿（`translate` 全在函数体内）

- [ ] **Step 7: Commit**

```bash
git add src/renderer/src/components/git-branch/ src/renderer/src/i18n/locales/en.json src/renderer/src/i18n/locales/zh.json
git commit -m "feat(git-branch): IDEA 风格分支树呈现组件 + git-branch locale 命名空间"
```

---

## Task 7: `use-branch-refs.ts` —— 取数与失效

**Files:**
- Create: `src/renderer/src/components/git-branch/use-branch-refs.ts`
- Test: `src/renderer/src/components/git-branch/use-branch-refs.test.tsx`

**Interfaces:**
- Consumes: `window.api.git.refs({ worktreePath })`（返回 Task 2 换型后的 `GitRefsResult`）
- Produces: `useBranchRefs(worktreePath: string | null): { refs: GitRefEntry[]; truncated: boolean; loading: boolean; error: string | null; invalidate: () => void }`

- [ ] **Step 1: 写失败测试**

Create `src/renderer/src/components/git-branch/use-branch-refs.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { useBranchRefs } from './use-branch-refs'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const entry: GitRefEntry = {
  name: 'main',
  fullName: 'refs/heads/main',
  category: 'local',
  oid: '0'.repeat(40),
  isHead: true,
}

let host: HTMLDivElement
let root: Root
const refs = vi.fn()
let api: { current: GitRefEntry[]; truncated: boolean; fail: boolean }

function Probe({
  path,
  onChange,
}: {
  path: string | null
  onChange: (v: unknown) => void
}): React.JSX.Element {
  const value = useBranchRefs(path)
  onChange(value)
  return <span>{value.loading ? 'loading' : 'idle'}</span>
}

let latest: { refs: GitRefEntry[]; truncated: boolean; loading: boolean; error: string | null; invalidate: () => void }

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  api = { current: [entry], truncated: false, fail: false }
  refs.mockReset().mockImplementation(() =>
    api.fail ? Promise.reject(new Error('boom')) : Promise.resolve({ refs: api.current, truncated: api.truncated })
  )
  vi.stubGlobal('window', { api: { git: { refs } } })
  latest = { refs: [], truncated: false, loading: true, error: null, invalidate: () => {} }
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
  vi.unstubAllGlobals()
})

const render = async (path: string | null = '/wt') => {
  await act(async () => {
    root.render(<Probe path={path} onChange={(v) => { latest = v as never }} />)
  })
  await act(async () => { await Promise.resolve() })
}
const flush = async () => {
  await act(async () => { await Promise.resolve() })
}

describe('useBranchRefs', () => {
  it('挂载取一次，refs/truncated 落进返回值，loading 归 false', async () => {
    await render()
    expect(refs).toHaveBeenCalledTimes(1)
    expect(refs).toHaveBeenCalledWith({ worktreePath: '/wt' })
    expect(latest.refs).toEqual([entry])
    expect(latest.loading).toBe(false)
    expect(latest.error).toBeNull()
  })

  it('path 为 null 时不请求', async () => {
    await render(null)
    expect(refs).not.toHaveBeenCalled()
    expect(latest.loading).toBe(false)
  })

  it('invalidate 再取一次并换新数据', async () => {
    await render()
    api.current = [{ ...entry, name: 'dev', fullName: 'refs/heads/dev' }]
    await act(async () => { latest.invalidate() })
    await flush()
    expect(refs).toHaveBeenCalledTimes(2)
    expect(latest.refs[0]?.name).toBe('dev')
  })

  it('失败降级为空列表 + error，不抛给渲染方', async () => {
    api.fail = true
    await render()
    expect(latest.refs).toEqual([])
    expect(latest.error).toBe('boom')
    expect(latest.loading).toBe(false)
  })

  it('path 变化重新取数', async () => {
    await render('/a')
    await act(async () => {
      root.render(<Probe path="/b" onChange={(v) => { latest = v as never }} />)
    })
    await flush()
    expect(refs).toHaveBeenLastCalledWith({ worktreePath: '/b' })
    expect(refs).toHaveBeenCalledTimes(2)
  })

  it('卸载后到达的响应不写 state（不抛 set-state-on-unmounted）', async () => {
    let resolve!: (v: { refs: GitRefEntry[]; truncated: boolean }) => void
    refs.mockImplementationOnce(
      () => new Promise((r) => { resolve = r })
    )
    await act(async () => {
      root.render(<Probe path="/wt" onChange={() => {}} />)
    })
    act(() => root.unmount())
    expect(() => resolve({ refs: [entry], truncated: false })).not.toThrow()
    root = createRoot(host) // 让 afterEach 的 unmount 不炸
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/use-branch-refs.test.tsx 2>&1 | tail -12`
Expected: FAIL —— 找不到模块 `./use-branch-refs`

- [ ] **Step 3: 实现**

```ts
import { useCallback, useEffect, useState } from 'react'
import type { GitRefEntry } from '../../../../shared/git-ref-types'

export type UseBranchRefsResult = {
  refs: GitRefEntry[]
  truncated: boolean
  loading: boolean
  error: string | null
  /** 让下一次渲染重新取数（写操作成功后由调用方触发）。 */
  invalidate: () => void
}

/**
 * `git_refs` 的取数 hook。刻意不做缓存/去重：refs 列表是一次 for-each-ref，
 * 便宜且必须新鲜（分支被外部增删是常态），缓存只会制造第二份真相。
 */
export function useBranchRefs(worktreePath: string | null): UseBranchRefsResult {
  const [refs, setRefs] = useState<GitRefEntry[]>([])
  const [truncated, setTruncated] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(worktreePath !== null)
  const [version, setVersion] = useState(0)

  const invalidate = useCallback(() => setVersion((v) => v + 1), [])

  useEffect(() => {
    if (!worktreePath) {
      setLoading(false)
      return
    }
    let alive = true
    setLoading(true)
    window.api.git
      .refs({ worktreePath })
      .then((result) => {
        if (!alive) return
        setRefs(result.refs)
        setTruncated(result.truncated)
        setError(null)
      })
      .catch((e: unknown) => {
        if (!alive) return
        // 降级为空列表：popover 出空态而不是崩面板（spec §7.5）。
        setRefs([])
        setTruncated(false)
        setError(e instanceof Error ? e.message : String(e))
      })
      .finally(() => {
        if (alive) setLoading(false)
      })
    return () => {
      alive = false
    }
  }, [worktreePath, version])

  return { refs, truncated, loading, error, invalidate }
}
```

- [ ] **Step 4: 跑测试**

Run: `pnpm vitest run src/renderer/src/components/git-branch/use-branch-refs.test.tsx 2>&1 | tail -20`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add src/renderer/src/components/git-branch/
git commit -m "feat(git-branch): useBranchRefs 取数 hook（失败降级 + invalidate）"
```

---

## Task 8: `branch-actions.ts` —— 动作声明表

**Files:**
- Create: `src/renderer/src/components/git-branch/branch-actions.ts`
- Test: `src/renderer/src/components/git-branch/branch-actions.test.ts`

**Interfaces:**
- Consumes: `BranchTreeRow` / `BranchWorkspaceOwner`（Task 4）、`GitRefEntry`
- Produces:
  - `BranchActionId = 'switch' | 'new-workspace' | 'view-in-log'`
  - `BranchActionContext`、`BranchAction`
  - `BRANCH_ACTIONS: readonly BranchAction[]`
  - `primaryBranchAction(row, ctx): BranchAction | null`
  - `branchActionForRef(ref, ownerOf): 'switch' | 'new-workspace'`（宿主与动作表共用的 owner 判定单一入口）

> **不在此提供 `resolveBranchActions(row, ctx)` 这类「枚举某行可用动作」的辅助函数**：
> 增量 1 唯一的生产消费者是 `primaryBranchAction`，菜单壳子（`branch-menu.tsx`）在增量 2。
> 现在就开一个只有测试在调的导出 API 是投机接口（YAGNI）；spec §7.4 的可用性矩阵
> 在测试里直接 `BRANCH_ACTIONS.filter(a => a.visible(row, ctx))` 断言即可，
> 矩阵本身由 `visible` 逐条承担。

- [ ] **Step 1: 写失败测试**

Create `src/renderer/src/components/git-branch/branch-actions.test.ts`：

```ts
import { describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow } from './branch-tree-model'
import {
  BRANCH_ACTIONS,
  branchActionForRef,
  primaryBranchAction,
  type BranchActionContext,
} from './branch-actions'

/** 矩阵断言用的局部枚举：不进生产代码（见本任务 Interfaces 下的说明）。 */
const visibleActions = (row: BranchTreeRow, ctx: BranchActionContext): string[] =>
  BRANCH_ACTIONS.filter((a) => a.visible(row, ctx)).map((a) => a.id)

const refEntry = (over: Partial<GitRefEntry> & { name: string }): GitRefEntry => ({
  fullName: `refs/heads/${over.name}`,
  category: 'local',
  oid: '0'.repeat(40),
  isHead: false,
  ...over,
})

const refRow = (ref: GitRefEntry, checkedOutBy = null): BranchTreeRow => ({
  kind: 'ref',
  id: `ref:${ref.fullName}`,
  depth: 1,
  ref,
  isUpstreamOfHead: false,
  checkedOutBy,
})

const groupRow = (): BranchTreeRow => ({
  kind: 'group',
  id: 'group:local',
  labelKey: 'auto.components.git-branch.groupLocal',
  labelFallback: 'Local',
  depth: 0,
  expandable: true,
  expanded: true,
  childCount: 2,
})

const synthetic = (value: 'all-branches' | 'head-current'): BranchTreeRow => ({
  kind: 'synthetic',
  id: `row:${value}`,
  depth: 0,
  labelKey: `k:${value}`,
  labelFallback: value,
  value,
})

function ctx(over: Partial<BranchActionContext> = {}): BranchActionContext {
  return {
    headBranch: 'main',
    ownerOf: () => null,
    activateWorkspace: vi.fn(),
    newWorkspaceFrom: vi.fn(),
    viewInGitLog: vi.fn(),
    ...over,
  }
}

const OWNER = { worktreeId: 'r::/b', displayName: 'B', path: '/b' }

describe('BRANCH_ACTIONS', () => {
  it('label 是惰性函数（no-top-level-translate AST 门禁要求）', () => {
    for (const action of BRANCH_ACTIONS) {
      expect(typeof action.label).toBe('function')
      expect(typeof action.label()).toBe('string')
      expect(action.label().length).toBeGreaterThan(0)
    }
  })
})

describe('branchActionForRef', () => {
  it('有 owner → switch，无 owner → new-workspace（单一判定入口）', () => {
    const ownerOf = (name: string) => (name === 'main' ? OWNER : null)
    expect(branchActionForRef(refEntry({ name: 'main' }), ownerOf)).toBe('switch')
    expect(branchActionForRef(refEntry({ name: 'dev' }), ownerOf)).toBe('new-workspace')
  })
})

describe('可见性矩阵（spec §7.4）', () => {
  it('组头行不出任何动作', () => {
    expect(visibleActions(groupRow(), ctx())).toEqual([])
  })

  it('有 owner 的本地分支：switch + new-workspace + view-in-log', () => {
    expect(visibleActions(refRow(refEntry({ name: 'main' }), OWNER), ctx({ ownerOf: () => OWNER }))).toEqual([
      'switch',
      'new-workspace',
      'view-in-log',
    ])
  })

  it('宿主裁剪靠「不传 viewInGitLog」，而非额外的 opts', () => {
    expect(
      visibleActions(refRow(refEntry({ name: 'main' }), OWNER), ctx({
        ownerOf: () => OWNER,
        viewInGitLog: undefined,
      }))
    ).toEqual(['switch', 'new-workspace'])
  })

  it('switch 对无 owner 的行不可见', () => {
    expect(visibleActions(refRow(refEntry({ name: 'dev' }), null), ctx())).toEqual(['new-workspace'])
  })

  it('remote / tag 行不出 switch（即使 ownerOf 能查到同名）', () => {
    const c = ctx({ ownerOf: () => OWNER })
    const remote = refRow(
      refEntry({ name: 'origin/main', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
      OWNER
    )
    expect(visibleActions(remote, c)).toEqual(['new-workspace', 'view-in-log'])
    const tag = refRow(refEntry({ name: 'v1.2.0', category: 'tag', fullName: 'refs/tags/v1.2.0' }), OWNER)
    expect(visibleActions(tag, c)).toEqual(['new-workspace', 'view-in-log'])
  })

  it('合成人物只出 view-in-log；Source Control 宿主下为空', () => {
    expect(visibleActions(synthetic('head-current'), ctx())).toEqual(['view-in-log'])
    expect(
      visibleActions(synthetic('head-current'), ctx({ viewInGitLog: undefined }))
    ).toEqual([])
  })
})

describe('primaryBranchAction', () => {
  it('有 owner 的本地分支 → switch 并带 owner 执行', () => {
    const activateWorkspace = vi.fn()
    const row = refRow(refEntry({ name: 'main' }), OWNER)
    const action = primaryBranchAction(row, ctx({ ownerOf: () => OWNER, activateWorkspace }))
    expect(action?.id).toBe('switch')
    action?.run(row, ctx({ ownerOf: () => OWNER, activateWorkspace }))
    expect(activateWorkspace).toHaveBeenCalledWith(OWNER)
  })

  it('无 owner → new-workspace 并带 ref 执行', () => {
    const newWorkspaceFrom = vi.fn()
    const ref = refEntry({ name: 'dev' })
    const row = refRow(ref)
    const c = ctx({ newWorkspaceFrom })
    expect(primaryBranchAction(row, c)?.id).toBe('new-workspace')
    primaryBranchAction(row, c)?.run(row, c)
    expect(newWorkspaceFrom).toHaveBeenCalledWith(ref)
  })

  it('合成人物在 Source Control 宿主返回 null（不可 activate，避免假动作）', () => {
    expect(primaryBranchAction(synthetic('head-current'), ctx())).toBeNull()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-actions.test.ts 2>&1 | tail -12`
Expected: FAIL —— 找不到模块 `./branch-actions`

- [ ] **Step 3: 实现**

```ts
import { translate } from '@/i18n/i18n'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow, BranchWorkspaceOwner } from './branch-tree-model'

export type BranchActionId = 'switch' | 'new-workspace' | 'view-in-log'

export type BranchActionContext = {
  /** 当前分支短名，null = detached。 */
  headBranch: string | null
  /** advisory：分支短名 → 检出它的 workspace（spec §6.4）。 */
  ownerOf: (branchName: string) => BranchWorkspaceOwner | null
  activateWorkspace: (owner: BranchWorkspaceOwner) => void
  newWorkspaceFrom: (ref: GitRefEntry) => void
  /** 仅 Git Log 宿主提供；缺省即不出该动作。 */
  viewInGitLog?: (value: 'all-branches' | 'head-current' | GitRefEntry) => void
}

export type BranchAction = {
  id: BranchActionId
  /** 惰性求值：`no-top-level-translate.test.ts` 是 AST 门禁。 */
  label: () => string
  visible: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
  run: (row: BranchTreeRow, ctx: BranchActionContext) => void
}

/** ref 行 → 该走 switch 还是 new-workspace 的**唯一**判定入口，宿主与动作表共用。 */
export function branchActionForRef(
  ref: GitRefEntry,
  ownerOf: (branchName: string) => BranchWorkspaceOwner | null
): BranchActionId {
  // 只有本地分支可能被 workspace 检出并直接跳转；remote/tag 一律走新建 workspace。
  return ref.category === 'local' && ownerOf(ref.name) ? 'switch' : 'new-workspace'
}

function refOf(row: BranchTreeRow): GitRefEntry | null {
  return row.kind === 'ref' ? row.ref : null
}

export const BRANCH_ACTIONS: readonly BranchAction[] = [
  {
    id: 'switch',
    label: () => translate('auto.components.git-branch.switchTo', 'Switch to this branch'),
    visible: (row, ctx) => {
      const ref = refOf(row)
      return ref !== null && ref.category === 'local' && ctx.ownerOf(ref.name) !== null
    },
    run: (row, ctx) => {
      const ref = refOf(row)
      const owner = ref ? ctx.ownerOf(ref.name) : null
      if (owner) ctx.activateWorkspace(owner)
    },
  },
  {
    id: 'new-workspace',
    label: () =>
      translate('auto.components.git-branch.newWorkspaceFrom', 'New Workspace from Branch…'),
    visible: (row) => refOf(row) !== null,
    run: (row, ctx) => {
      const ref = refOf(row)
      if (ref) ctx.newWorkspaceFrom(ref)
    },
  },
  {
    id: 'view-in-log',
    label: () => translate('auto.components.git-branch.viewInGitLog', 'View in Git Log'),
    visible: (row, ctx) =>
      ctx.viewInGitLog !== undefined &&
      (row.kind === 'ref' || row.kind === 'synthetic'),
    run: (row, ctx) => {
      if (row.kind === 'ref') ctx.viewInGitLog?.(row.ref)
      else if (row.kind === 'synthetic') ctx.viewInGitLog?.(row.value)
    },
  },
]

/**
 * Enter / 单击的动作。刻意**只**覆盖 ref 行与（Git Log 宿主的）合成人物：
 * Source Control 里点 `HEAD (Current Branch)` 无意义，返回 null 让宿主吞掉点击，
 * 而不是执行一个看起来像成功的空动作。
 */
export function primaryBranchAction(
  row: BranchTreeRow,
  ctx: BranchActionContext
): BranchAction | null {
  if (row.kind === 'ref') {
    const id = branchActionForRef(row.ref, ctx.ownerOf)
    return BRANCH_ACTIONS.find((a) => a.id === id) ?? null
  }
  if (row.kind === 'synthetic' && ctx.viewInGitLog) {
    return BRANCH_ACTIONS.find((a) => a.id === 'view-in-log') ?? null
  }
  return null
}
```

- [ ] **Step 4: 跑测试 + AST 门禁**

Run: `pnpm vitest run src/renderer/src/components/git-branch/ 2>&1 | tail -20`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
git add src/renderer/src/components/git-branch/
git commit -m "feat(git-branch): 动作声明表（switch/new-workspace/view-in-log + 主动作判定）"
```

---

## Task 9: 宿主 A —— Source Control 头部分支切换器

**Files:**
- Create: `src/renderer/src/components/git-branch/source-control-branch-switcher.tsx`
- Test: `src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx`
- Modify: `src/shared/workspace-source.ts`（新增一个 telemetry source 枚举值，见 Step 3）
- Modify: `src/renderer/src/components/right-sidebar/source-control/panel/branch-context-row.tsx`
- Modify: `src/renderer/src/components/right-sidebar/source-control/panel/header-toolbar.tsx`
- Modify: `src/renderer/src/components/right-sidebar/source-control/panel/panel-ready.tsx`

**Interfaces:**
- Consumes: `BranchRefTree`（Task 6）、`useBranchRefs`（Task 7）、`primaryBranchAction` / `ownerByBranch`（Task 4/8）
- Produces:
  - `SourceControlBranchSwitcher({ worktreeId, children }: { worktreeId: string; children: React.ReactNode }): React.JSX.Element`
  - `branch-context-row.tsx` 新增**可选** prop `branchSwitcherWorktreeId?: string | null`（沿 `SourceControlBranchContextRow` → `StackedCompareFlow` → `HeadIdentity` 透传）

> **为什么只加一个可选 prop、其余全走 store**：`SourceControlBranchSwitcher` 需要的
> `worktreePath` / `repoId` / `worktrees` / `setActiveWorktree` / `openModal` 都能由
> `worktreeId` 从 store 推出（`findWorktreeById`）。把 5 个回调沿 3 层组件钻下去会改动
> `header-toolbar` / `panel-content` / `panel-ready` 的既有 props 面，并连带打红
> `SourceControl.*.test.tsx` 那批大桩。单 prop + 自取 store 的代价是本组件不再是纯呈现件，
> 由它自己的测试覆盖。

- [ ] **Step 1: 写失败测试**

Create `src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { Worktree } from '../../../../shared/worktree/types'
import { SourceControlBranchSwitcher } from './source-control-branch-switcher'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const setActiveWorktree = vi.fn()
const openModal = vi.fn()
const refs = vi.fn()

const worktrees: Worktree[] = [
  { id: 'r::/a', repoId: 'r', branch: 'main', displayName: 'A', path: '/a' },
  { id: 'r::/b', repoId: 'r', branch: 'dev', displayName: 'B', path: '/b' },
] as unknown as Worktree[]

vi.mock('@/store', () => {
  const useAppStore = Object.assign(
    (selector: (s: unknown) => unknown) => selector(state),
    { getState: () => state }
  )
  return { useAppStore }
})
const state = {
  worktreesByRepo: { r: worktrees },
  repos: [{ id: 'r', path: '/repo' }],
  setActiveWorktree,
  openModal,
}

function entry(over: Partial<GitRefEntry> & { name: string }): GitRefEntry {
  return {
    fullName: `refs/heads/${over.name}`,
    category: 'local',
    oid: '0'.repeat(40),
    isHead: false,
    ...over,
  }
}

let host: HTMLDivElement
let root: Root

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  setActiveWorktree.mockReset()
  openModal.mockReset()
  refs.mockReset().mockResolvedValue({
    refs: [
      entry({ name: 'main', isHead: true }),
      entry({ name: 'dev' }),
      entry({ name: 'fresh' }),
      entry({ name: 'origin/main', category: 'remote', remote: 'origin', fullName: 'refs/remotes/origin/main' }),
    ],
    truncated: false,
  })
  vi.stubGlobal('window', { api: { git: { refs } } })
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
  vi.unstubAllGlobals()
})

const open = async () => {
  await act(async () => {
    root.render(
      <SourceControlBranchSwitcher worktreeId="r::/a">
        <button type="button">main</button>
      </SourceControlBranchSwitcher>
    )
  })
  await act(async () => {
    host.querySelector('button')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
  await act(async () => {
    await Promise.resolve()
  })
}
const clickRow = async (id: string) => {
  await act(async () => {
    host.querySelector(`[data-testid="branch-row-${id}"]`)!.dispatchEvent(
      new MouseEvent('click', { bubbles: true })
    )
  })
}

describe('SourceControlBranchSwitcher', () => {
  it('渲染 children 为触发器，不额外包一层可见文案', async () => {
    await act(async () => {
      root.render(
        <SourceControlBranchSwitcher worktreeId="r::/a">
          <span data-testid="trigger">main</span>
        </SourceControlBranchSwitcher>
      )
    })
    expect(host.querySelector('[data-testid="trigger"]')?.textContent).toBe('main')
  })

  it('打开时取 refs（用 worktree 的 path，不是 repo path）', async () => {
    await open()
    expect(refs).toHaveBeenCalledWith({ worktreePath: '/a' })
  })

  it('点有 owner 的分支 → setActiveWorktree(该 worktreeId, 该 host)，不新建 workspace', async () => {
    await open()
    await clickRow('ref:refs/heads/dev')
    expect(setActiveWorktree).toHaveBeenCalledTimes(1)
    expect(setActiveWorktree.mock.calls[0]?.[0]).toBe('r::/b')
    expect(openModal).not.toHaveBeenCalled()
  })

  it('点无 owner 的分支 → openModal(new-workspace-composer) 且预填 initialBaseBranch', async () => {
    await open()
    await clickRow('ref:refs/heads/fresh')
    expect(setActiveWorktree).not.toHaveBeenCalled()
    expect(openModal).toHaveBeenCalledWith('new-workspace-composer', {
      initialRepoId: 'r',
      initialBaseBranch: 'fresh',
      telemetrySource: 'source_control_branch_menu',
    })
  })

  it('点 remote 分支 → 一律新建 workspace，start-point 用去前缀名', async () => {
    await open()
    await clickRow('ref:refs/remotes/origin/main')
    expect(openModal).toHaveBeenCalledWith(
      'new-workspace-composer',
      expect.objectContaining({ initialBaseBranch: 'origin/main' })
    )
  })

  it('点 HEAD (Current Branch) 行不触发任何写动作', async () => {
    await open()
    await clickRow('row:head-current')
    expect(setActiveWorktree).not.toHaveBeenCalled()
    expect(openModal).not.toHaveBeenCalled()
  })

  it('worktreeId 找不到对应 worktree → 只渲染 children，点击不取数', async () => {
    await act(async () => {
      root.render(
        <SourceControlBranchSwitcher worktreeId="nope">
          <button type="button">main</button>
        </SourceControlBranchSwitcher>
      )
    })
    await act(async () => {
      host.querySelector('button')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(refs).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx 2>&1 | tail -12`
Expected: FAIL —— 找不到模块 `./source-control-branch-switcher`

- [ ] **Step 3: 先扩 telemetry source，再写组件**

`ComposerModalData.telemetrySource` 的类型是 `WorkspaceSource`（`NewWorkspaceComposerModal.tsx:41`），
它是一个**闭合联合**（`src/shared/workspace-source.ts`）：传 `'source_control_branch_menu'` 而不扩枚举
会直接 `pnpm typecheck` 红。已核实该联合全仓库**无穷举 switch**，仅被透传到
`workspace_created` 事件（`src/shared/worktree/create-types.ts:136`），所以加一个成员是安全的。

`src/shared/workspace-source.ts` 的 `WORKSPACE_SOURCE_VALUES` 改为：

```ts
export const WORKSPACE_SOURCE_VALUES = [
  'command_palette',
  'sidebar',
  'shortcut',
  'drag_drop',
  'onboarding',
  'terminal_context_menu',
  'source_control_branch_menu',
  'unknown'
] as const
```

Run: `pnpm typecheck 2>&1 | tail -8`
Expected: 绿（无新增错误）

- [ ] **Step 4: 实现 `source-control-branch-switcher.tsx`**

```tsx
import { useMemo, useState } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import { translate } from '@/i18n/i18n'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from './branch-ref-tree'
import { ownerByBranch, type BranchWorkspaceOwner } from './branch-tree-model'
import { primaryBranchAction, type BranchActionContext } from './branch-actions'
import { useBranchRefs } from './use-branch-refs'

export type SourceControlBranchSwitcherProps = {
  worktreeId: string
  /** 触发器内容（分支名元素）。刻意由宿主渲染，保持 `HeadIdentity` 现有样式与 tooltip。 */
  children: React.ReactNode
}

/**
 * Source Control 头部的分支树弹层（spec §5 宿主 A）。
 *
 * 只吃一个 `worktreeId`，其余从 store 自取：把 5 个回调沿 3 层组件钻下去会改动
 * header-toolbar / panel-content / panel-ready 的既有 props 面并打红一批大桩，
 * 代价大于「非纯呈现件」这条收益。
 */
export function SourceControlBranchSwitcher({
  worktreeId,
  children,
}: SourceControlBranchSwitcherProps): React.JSX.Element {
  const [open, setOpen] = useState(false)
  const worktree = useAppStore((s) => findWorktreeById(s.worktreesByRepo, worktreeId))
  const worktrees = useAppStore((s) => s.worktreesByRepo)
  const setActiveWorktree = useAppStore((s) => s.setActiveWorktree)
  const openModal = useAppStore((s) => s.openModal)

  const { refs, truncated, loading, error } = useBranchRefs(open ? (worktree?.path ?? null) : null)
  const owners = useMemo(() => {
    const all = Object.values(worktrees).flat()
    return ownerByBranch(all)
  }, [worktrees])

  const ctx = useMemo<BranchActionContext>(() => {
    const ownerOf = (branchName: string): BranchWorkspaceOwner | null =>
      owners.get(branchName) ?? null
    return {
      headBranch: worktree?.branch || null,
      ownerOf,
      activateWorkspace: (owner) => {
        setOpen(false)
        // executionHostId 由 store 从该 worktree 的 identity 推；这里传第二参数为 undefined
        // 即走 worktree 自身的 host（与 sidebar 跳转同路径）。
        setActiveWorktree(owner.worktreeId)
      },
      newWorkspaceFrom: (ref: GitRefEntry) => {
        setOpen(false)
        openModal('new-workspace-composer', {
          initialRepoId: worktree?.repoId,
          initialBaseBranch: ref.name,
          telemetrySource: 'source_control_branch_menu',
        })
      },
      // 宿主 A 不提供 viewInGitLog —— 该动作只属于 Git Log 宿主（spec §5 的动作裁剪）。
    }
  }, [owners, setActiveWorktree, openModal, worktree?.branch, worktree?.repoId])

  if (!worktree) return <>{children}</>

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        {children}
      </PopoverTrigger>
      <PopoverContent align="start" side="bottom" sideOffset={4} className="w-64 p-1.5">
        {loading && refs.length === 0 ? (
          <p className="px-1 py-1 text-xs text-muted-foreground">
            {translate('auto.components.git-branch.loading', 'Loading branches…')}
          </p>
        ) : error && refs.length === 0 ? (
          // 失败不弹错也不自动收起：popover 里一行文案（spec §7.5）。
          <p className="px-1 py-1 text-xs text-muted-foreground">
            {translate('auto.components.git-branch.loadFailed', 'Failed to read branches.')}
          </p>
        ) : (
          <BranchRefTree
            refs={refs}
            truncated={truncated}
            headBranch={ctx.headBranch}
            checkedOutByBranch={owners}
            onActivate={(row) => {
              primaryBranchAction(row, ctx)?.run(row, ctx)
            }}
          />
        )}
      </PopoverContent>
    </Popover>
  )
}
```

> `PopoverTrigger asChild` 要求 children 是**单个可转发 ref 的元素**。`HeadIdentity` 现在渲染的是
> `TooltipTrigger asChild` 包着的 `span` —— Step 6 的接线把它换成 `button`，两层 asChild 才能串起来。

- [ ] **Step 5: 跑组件自身测试**

Run: `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx 2>&1 | tail -25`
Expected: 全绿。若 Radix Popover 在 happy-dom 下不渲染 content，加
`vi.mock('@/components/ui/popover', …)` 出一个「直接渲染 children 内容」的替身，
并在测试里注明「Popover 壳由 `ui/popover` 自己的测试覆盖」—— **不要**为了过测试改组件。

- [ ] **Step 6: 接线 `branch-context-row.tsx`**

`HeadIdentity` 换成接受可选 `switcherWorktreeId`，并在提供时把分支名渲染成 `button` 触发器：

```tsx
function HeadIdentity({
  display,
  switcherWorktreeId = null,
}: {
  display: WorktreeGitIdentityDisplay
  switcherWorktreeId?: string | null
}): React.JSX.Element {
  if (display.kind === 'detached') {
    return (
      <DetachedHeadBadge
        display={display}
        side="bottom"
        // Why: tooltip carries the full detached explanation; keep it keyboard-reachable.
        tabIndex={0}
        className="min-w-0 max-w-full shrink"
      />
    )
  }

  const branchAriaLabel = translate(
    'auto.components.right.sidebar.SourceControl.a4e93c21d7',
    'Current branch: {{value0}}',
    { value0: display.branchName }
  )

  // Why: focusable + tooltip so truncated long branch names stay discoverable.
  // `block` is load-bearing: `truncate` clips nothing on an inline box.
  if (!switcherWorktreeId) {
    // 无上下文时保持逐字节不变的旧呈现 —— 现有 SourceControl.* 测试的桩不必跟着改。
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            className="block min-w-0 max-w-full truncate rounded-sm font-mono text-[10.5px] font-medium text-foreground/90 outline-none focus-visible:ring-1 focus-visible:ring-ring"
            tabIndex={0}
            aria-label={branchAriaLabel}
            data-testid="source-control-head-identity"
          >
            {display.branchName}
          </span>
        </TooltipTrigger>
        <TooltipContent side="bottom" sideOffset={6} className="max-w-72 break-all font-mono">
          {display.branchName}
        </TooltipContent>
      </Tooltip>
    )
  }

  return (
    <SourceControlBranchSwitcher worktreeId={switcherWorktreeId}>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-haspopup="tree"
          aria-label={branchAriaLabel}
          className="block min-w-0 max-w-full truncate rounded-sm border-0 bg-transparent p-0 text-left font-mono text-[10.5px] font-medium text-foreground/90 outline-none hover:text-foreground hover:underline focus-visible:ring-1 focus-visible:ring-ring"
          data-testid="source-control-head-identity"
        >
          {display.branchName}
        </button>
      </TooltipTrigger>
    </SourceControlBranchSwitcher>
  )
}
```

再沿 `StackedCompareFlow` → `SourceControlBranchContextRow` 各加一个可选 prop 并透传：

```tsx
// StackedCompareFlow 的入参对象里加：
  switcherWorktreeId?: string | null
// 两处 <HeadIdentity display={…} /> 改为：
  <HeadIdentity display={headDisplay} switcherWorktreeId={switcherWorktreeId} />

// SourceControlBranchContextRow 的入参里加：
  branchSwitcherWorktreeId?: string | null
// 传给 StackedCompareFlow：
  switcherWorktreeId={branchSwitcherWorktreeId}
```

文件头部加 import：

```tsx
import { SourceControlBranchSwitcher } from '@/components/git-branch/source-control-branch-switcher'
```

- [ ] **Step 7: 接线 `header-toolbar.tsx` 与 `panel-ready.tsx`**

`header-toolbar.tsx`：props 类型加 `branchSwitcherWorktreeId?: string | null`，
解构后传给 `SourceControlBranchContextRow`：

```tsx
<SourceControlBranchContextRow
  summary={branchSummary}
  compareBaseRef={compareBaseRef}
  headDisplay={headDisplay}
  branchLineTotal={branchLineTotal}
  branchSwitcherWorktreeId={branchSwitcherWorktreeId}
  onChangeBaseRef={onChangeBaseRef}
  onRetry={onRefreshBranchCompare}
/>
```

`panel-ready.tsx`：`SourceControlHeaderToolbar` 调用处加一行（`activeWorktreeId` 该文件已有）：

```tsx
branchSwitcherWorktreeId={activeWorktreeId}
```

- [ ] **Step 8: 确认没打红既有面板测试**

Run: `pnpm vitest run src/renderer/src/components/right-sidebar 2>&1 | tail -25`
Expected: 全绿 —— 因为现有测试都不传 `branchSwitcherWorktreeId`，走的是逐字节不变的旧分支。
若某个测试红了，说明它构造了 `HeadIdentity` 的新分支，回 Step 6 检查旧分支是否真的一致。

Run: `pnpm typecheck 2>&1 | tail -10`
Expected: 绿（`git-log` 那三处报错要到 Task 10 才出现，本任务时 `GitRefs` 还没换型）

- [ ] **Step 9: Commit**

```bash
git add src/renderer/src/components/git-branch/ \
        src/shared/workspace-source.ts \
        src/renderer/src/components/right-sidebar/source-control/panel/
git commit -m "feat(source-control): 头部分支名接入 IDEA 风格分支树切换器"
```

---

## Task 10: 宿主 B —— Git Log 删左栏、branch chip 改弹层（含换型原子提交）

**Files:**
- Delete: `src/renderer/src/components/git-log/RefPanel.tsx`
- Modify: `src/shared/preload-api/api/git-inspection-api.ts:27-28, 72`
- Modify: `src/renderer/src/components/git-log/types.ts:6`
- Modify: `src/renderer/src/components/git-log/GitLogView.tsx:10, 52, 133-152, 245-266`
- Modify: `src/renderer/src/components/git-log/Toolbar.tsx`
- Modify: `src/renderer/src/components/git-log/git-log-view.test.tsx`
- Modify: `src/renderer/src/i18n/locales/en.json` / `zh.json`（清 `git-log` 孤儿 key）

**Interfaces:**
- Consumes: `BranchRefTree`、`GitRefEntry`
- Produces: `Toolbar` 新增 props `refs: GitRefEntry[]`、`refsTruncated: boolean`、`onPickRef: (value: 'all-branches' | 'head-current' | GitRefEntry) => void`

> **本任务必须一次 commit 完成**：换型 `GitRefs` 会打断 `RefPanel.tsx`（读 `id` / `category: 'branches'`）、
> `GitLogView.tsx`、`git-log-view.test.tsx` 三处，分开提交会让中间 commit 的 `pnpm typecheck` 红。

- [ ] **Step 1: 先记下当前 typecheck 的错误集**

Run: `pnpm typecheck 2>&1 | tail -20`
Expected: 当前绿（本任务尚未换型）。这一步是为了让 Step 6 的「错误清零」有对照基线。

- [ ] **Step 2: 换型 preload-api 与 git-log/types**

`src/shared/preload-api/api/git-inspection-api.ts`：把 27-28 行的手抄定义替换为引用共享类型。
头部 `use` 区加：

```ts
import type { GitRefEntry } from '../../git-ref-types'
```

原位置：

```ts
/** `git_refs` result: capped ref list + truncation flag. */
export type GitRefsResult = { refs: GitRefEntry[]; truncated: boolean }
```

若 `GitHistoryItemRef` 在该文件已无其它引用，删掉它的 import。

`src/renderer/src/components/git-log/types.ts` 整体替换为：

```ts
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { GitHistoryItem, GitHistoryResult } from '../../../../shared/git-history-types'
import type { GitBranchChangeEntry } from '../../../../shared/git-diff-compare-types'
import type { GitLogFilter } from './query'

export type GitLogSnapshot = GitHistoryResult
export type GitRefs = { refs: GitRefEntry[]; truncated: boolean }

/** Data source for the native Git Log view (implemented over tauri commands). */
export interface GitLogRepository {
  loadSnapshot(filter?: GitLogFilter, skip?: number): Promise<GitLogSnapshot>
  loadRefs(): Promise<GitRefs>
  loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]>
}
```

- [ ] **Step 3: 改 `Toolbar.tsx` —— branch chip 变弹层触发器**

整体替换 `src/renderer/src/components/git-log/Toolbar.tsx`：

```tsx
import { useState, type ChangeEvent } from 'react'
import { translate } from '@/i18n/i18n'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from '../git-branch/branch-ref-tree'
import type { GitLogFilter } from './query'

type Props = {
  branch: string
  query: string
  filter: GitLogFilter
  remoteCount: number
  localCount: number
  refs: GitRefEntry[]
  refsTruncated: boolean
  onQuery: (v: string) => void
  onFilterChange: (patch: Partial<GitLogFilter>) => void
  onRefresh: () => void
  /** 与 Source Control 宿主共用同一棵树，但 activate 语义是「设查看 ref」（spec §5 宿主 B）。 */
  onPickRef: (value: 'all-branches' | 'head-current' | GitRefEntry) => void
}

const chip =
  'rounded border border-input bg-transparent px-2 py-1 text-xs text-foreground outline-none focus:ring-1 focus:ring-ring'

export function Toolbar({
  branch,
  query,
  filter,
  remoteCount,
  localCount,
  refs,
  refsTruncated,
  onQuery,
  onFilterChange,
  onRefresh,
  onPickRef,
}: Props): React.JSX.Element {
  const [open, setOpen] = useState(false)
  const text = (key: 'author' | 'grep' | 'path') => (e: ChangeEvent<HTMLInputElement>) =>
    onFilterChange({ [key]: e.target.value || undefined })
  const date = (key: 'since' | 'until') => (e: ChangeEvent<HTMLInputElement>) =>
    onFilterChange({ [key]: e.target.value || undefined })

  return (
    <div className="flex flex-col gap-2 border-b border-border px-3 py-2 text-sm">
      <div className="flex items-center gap-2">
        <span className="font-medium text-foreground">
          {translate('auto.components.git-log.title', 'Git Log')}
        </span>
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              aria-haspopup="tree"
              data-testid="branch-picker"
              className="rounded bg-secondary px-1.5 py-0.5 text-xs text-secondary-foreground hover:bg-secondary/70"
            >
              {branch}
            </button>
          </PopoverTrigger>
          <PopoverContent align="start" side="bottom" sideOffset={4} className="w-64 p-1.5">
            <BranchRefTree
              refs={refs}
              truncated={refsTruncated}
              headBranch={branch}
              includeAllBranchesRow
              activeValue={
                filter.refs === undefined || filter.refs === 'head'
                  ? 'row:head-current'
                  : filter.refs === 'all'
                    ? 'row:all-branches'
                    : `ref:refs/heads/${filter.refs}`
              }
              onActivate={(row) => {
                setOpen(false)
                if (row.kind === 'ref') onPickRef(row.ref)
                else if (row.kind === 'synthetic') onPickRef(row.value)
              }}
            />
          </PopoverContent>
        </Popover>
        <span className="text-xs text-muted-foreground">↑{localCount} ↓{remoteCount}</span>
        <input
          data-testid="search"
          value={query}
          placeholder={translate('auto.components.git-log.filterVisible', 'Filter visible…')}
          onChange={(e) => onQuery(e.target.value)}
          className={`ml-auto w-48 ${chip}`}
        />
        <button
          type="button"
          onClick={onRefresh}
          className="rounded border border-border px-2 py-1 text-xs hover:bg-accent"
        >
          {translate('auto.components.git-log.refresh', 'Refresh')}
        </button>
      </div>
      {/* Backend filters (debounced). Empty fields are omitted so the host applies defaults. */}
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <input data-testid="filter-author" value={filter.author ?? ''} placeholder={translate('auto.components.git-log.author', 'Author')} onChange={text('author')} className={chip} />
        <input data-testid="filter-grep" value={filter.grep ?? ''} placeholder={translate('auto.components.git-log.grep', 'Message (grep)')} onChange={text('grep')} className={chip} />
        <input data-testid="filter-path" value={filter.path ?? ''} placeholder={translate('auto.components.git-log.path', 'Path')} onChange={text('path')} className={chip} />
        <label className="flex items-center gap-1 text-muted-foreground">
          {translate('auto.components.git-log.since', 'Since')}
          <input data-testid="filter-since" type="date" value={filter.since ?? ''} onChange={date('since')} className={chip} />
        </label>
        <label className="flex items-center gap-1 text-muted-foreground">
          {translate('auto.components.git-log.until', 'Until')}
          <input data-testid="filter-until" type="date" value={filter.until ?? ''} onChange={date('until')} className={chip} />
        </label>
      </div>
    </div>
  )
}
```

- [ ] **Step 4: 改 `GitLogView.tsx` —— 删左栏、接 `onPickRef`**

四处改动：

1. 删掉第 10 行 `import { RefPanel } from './RefPanel'`。
2. 第 4 行的 `GitHistoryItemRef` import 换成 `GitRefEntry`：
   ```ts
   import type { GitHistoryItem, GitHistoryResult } from '../../../../shared/git-history-types'
   import type { GitRefEntry } from '../../../../shared/git-ref-types'
   ```
3. 第 52 行 refs state 换型：
   ```ts
   const [refs, setRefs] = useState<GitRefEntry[]>([])
   ```
4. 在 `applyFilter` 附近加 ref 选择器（把树的 activate 语义翻译成 `filter.refs`）：
   ```ts
   /** 分支树 activate → 设查看范围。语义映射集中在这一处，Toolbar 不做翻译。 */
   const pickRef = useCallback((value: 'all-branches' | 'head-current' | GitRefEntry) => {
     if (value === 'all-branches') applyFilter({ refs: 'all' })
     else if (value === 'head-current') applyFilter({ refs: undefined })
     else applyFilter({ refs: value.name })
   }, [applyFilter])
   ```
5. `Toolbar` 调用处补三个 prop：
   ```tsx
   <Toolbar
     branch={branch}
     query={query}
     filter={filter}
     localCount={ahead}
     remoteCount={behind}
     refs={refs}
     refsTruncated={refsTruncated}
     onQuery={setQuery}
     onFilterChange={applyFilter}
     onRefresh={() => void reload(filter)}
     onPickRef={pickRef}
   />
   ```
6. 布局：删掉 254 行的 `<RefPanel … />`，让 `GraphList` 独占那一行。
   133-152 行的 refs 取数 effect **保留不动**（它喂的就是工具栏弹层，注释里那句
   "degrades the picker to the built-in Current / All ranges" 已不成立，改为：
   `// Refs load once; a failure degrades the branch picker to an empty list rather than blocking the log.`）

- [ ] **Step 5: 删 `RefPanel.tsx` 并清孤儿 key**

```bash
git rm src/renderer/src/components/git-log/RefPanel.tsx
```

`en.json` / `zh.json` 的 `auto.components.git-log` 下删掉随文件失效的 key：
`filterBranches`、`current`、`all`、`refsTruncated`、`groupBranches`、`groupRemoteBranches`、`groupTags`
（`title` / `filterVisible` / `refresh` / `author` / `grep` / `path` / `since` / `until` /
`loadingMore` / `loadMoreFailed` / `retry` / `loadMore` / `selectCommit` / `stats` / `openDiff` /
`loadFailed` / `unavailable` / `loading` / `workspaceUnavailable` **全部保留**）。

Run: `grep -rn "git-log.filterBranches\|git-log.current\|git-log.all'\|git-log.refsTruncated\|git-log.groupBranches\|git-log.groupRemoteBranches\|git-log.groupTags" src/ | grep -v locales`
Expected: 无输出（确认没有残留引用后才删 key）

- [ ] **Step 6: 改测试**

`git-log-view.test.tsx`：
1. 删掉 `import { RefPanel } from './RefPanel'`。
2. `fakeRepo()` 的 `loadRefs` 返回换型：
   ```ts
   const result: GitRefs = {
     refs: [{
       name: 'feature',
       fullName: 'refs/heads/feature',
       category: 'local',
       oid: '0'.repeat(40),
       isHead: false,
     }],
     truncated: false,
   }
   ```
3. `snapshot()` 的 `currentRef` 保持 `GitHistoryItemRef` 形状（那是 decoration 面，**没换型**）—— 别顺手改。
4. 删掉整个 `it('RefPanel search filters the branch list by name', …)` 用例
   （其过滤覆盖已在 Task 5 的 `branch-tree-filter.test.ts`）。
5. 第一个用例里 `expect(host.textContent).toContain('feature')` 现在**不再成立** ——
   refs 只在弹层打开后才渲染。改为：
   ```ts
   expect(host.querySelector('[data-testid="branch-picker"]')?.textContent).toContain('main')
   ```
6. `it('picking All range re-queries with refs=all after the debounce')` 改成走弹层：
   ```ts
   await act(async () => {
     host.querySelector('[data-testid="branch-picker"]')!.dispatchEvent(
       new MouseEvent('click', { bubbles: true })
     )
   })
   await act(async () => { await Promise.resolve() })
   const all = host.querySelector('[data-testid="branch-row-row:all-branches"]')
   expect(all).not.toBeNull()
   await act(async () => {
     all!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
   })
   ```
   （若 Radix Popover 在 happy-dom 下不渲染 content，按 Task 9 Step 5 的同一策略桩 `ui/popover`。）
7. 新增一条：
   ```ts
   it('picking a branch from the popover re-queries with that ref name', async () => {
     const { repo, calls } = fakeRepo()
     await act(async () => { root.render(<GitLogView repository={repo} />) })
     await flush()
     await act(async () => {
       host.querySelector('[data-testid="branch-picker"]')!.dispatchEvent(
         new MouseEvent('click', { bubbles: true })
       )
     })
     await act(async () => { await Promise.resolve() })
     await act(async () => {
       host.querySelector('[data-testid="branch-row-ref:refs/heads/feature"]')!.dispatchEvent(
         new MouseEvent('click', { bubbles: true })
       )
     })
     await act(async () => { await new Promise((r) => setTimeout(r, 400)) })
     expect(calls[calls.length - 1]?.filter).toEqual({ refs: 'feature' })
   })
   ```

- [ ] **Step 7: 跑门禁**

Run: `pnpm typecheck 2>&1 | tail -15`
Expected: 绿（Step 1 的基线错误集为空，此处也必须为空）

Run: `pnpm vitest run src/renderer/src/components/git-log src/renderer/src/components/git-branch 2>&1 | tail -30`
Expected: 全绿

Run: `pnpm vitest run src/renderer/src/i18n 2>&1 | tail -20`
Expected: 全绿（孤儿 key 清理未破坏任何 locale 测试）

- [ ] **Step 8: Commit**

```bash
git add -A src/shared/preload-api src/renderer/src/components/git-log \
           src/renderer/src/components/git-branch src/renderer/src/i18n/locales
git commit -m "refactor(git-log)!: 删左栏 RefPanel，分支选择合并进头部同款树弹层

GitRefsResult/GitRefs 元素类型由 GitHistoryItemRef 换成 GitRefEntry，不做兼容。
清理 RefPanel 独占的 6 个 locale key。"
```

---

## Task 11: 增量收尾门禁 + 手工验收

- [ ] **Step 1: 全量 Rust**

Run: `cd src-tauri && cargo test --workspace 2>&1 | tail -30`
Expected: exit 0。`bindings_are_fresh` 与 `command_inventory_and_bindings_are_fresh` 必须绿 ——
它们是 bindings 与手抄镜像不同步的兜底锁。若 `orcinus-cli` 的 `cli_basics` 报 5 项失败，
那是过期构建产物（既有环境问题），先 `cargo build -p orcinus-cli --bin orcinus` 再复跑。

- [ ] **Step 2: 全量 TS**

Run: `pnpm typecheck && pnpm test 2>&1 | tail -40`
Expected: 全绿。后台跑，以「判活 + 失败计数为 0」判定绿色，不要只看最后几行。

- [ ] **Step 3: 手工验收（必须真开应用看，测试替代不了）**

Run: `pnpm dev`

逐项确认并记录实际结果：

1. 右侧栏 Source Control 头部，分支名 `main` 现在是**可点的**（hover 有下划线），点开出树。
2. 树里有 `HEAD (Current Branch)`、`Local`、`Remote > origin > …`；当前分支带 🏷 且不在 Local 组里重复出现。
3. 搜索框输入 `f/` 之类段前缀能裁剪；清空后折叠态**不丢**。
4. 点一个有 workspace 检出的分支 → 面板切到那个 workspace，弹层关闭。
5. 点一个无 workspace 的分支 → 新建工作区 composer 打开且基线分支已预填。
6. 打开 Git Log tab：左栏**没了**，图谱列表变宽；点工具栏那个分支 chip 出同一棵树，且顶部多 `All Branches`。
7. 在 Git Log 树里选一个分支 → 列表按该 ref 重查；选 `All Branches` → 全量。
8. ahead/behind 数字与 `git -C <path> for-each-ref --format='%(refname) %(upstream:track)'` 的实际输出一致（拿真仓核一条）。
9. 中文语言下树文案为中文；切到 es/fr/ja/ko 时显示英文 fallback（不是 key 字面量）。

- [ ] **Step 4: CodeReview 门禁**

对本增量全分支 diff 派一次 CodeReview（`review_scope` 用默认 current-session 会漏掉已 commit 的内容，
故明确给 Git 目标）：

```bash
git diff main..HEAD --stat
```

把该 diff 范围交给 CodeReview 子代理，要求覆盖完整性/正确性/影响面三项，并特别要求核对：
- `git-ref-types.ts` 手抄镜像与 `tauri-bindings.ts` 生成形状是否逐字段一致（既有「TS 与 Rust 白名单不同步」陷阱）
- `buildBranchTreeRows` 的折叠语义与键盘 ←→ 是否共用同一份 `collapsed` state
- 删 locale key 后是否有残留引用

- [ ] **Step 5: 按审查结论修完，再打增量 tag/commit**

```bash
git log --oneline main..HEAD
```

确认每个 commit 的 `pnpm typecheck` 都能独立成立（增量 1 的原子性要求）。若中间某个 commit 是红的，
用 `git rebase -i main` 合并相邻的「换型 + 修消费者」commit。
