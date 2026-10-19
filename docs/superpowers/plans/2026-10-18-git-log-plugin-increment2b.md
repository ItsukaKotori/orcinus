# Git Log 插件 增量 2b（VcsLog 真查询）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Git Log 插件显示**全分支/过滤/分页的真实提交历史**——扩 `ade_git` 后端查询（`--all`/rev 范围 + author/date/path/grep 过滤 + skip 分页）、`git.log` 扩参、新增只读 `git.refs`、UI 接选择器+过滤 chips+无限滚动。

**Architecture:** 查询在**后端 `ade_git`** 执行（翻成 `git log` 参数），host 方法 `git.log`/`git.refs` 只读、`active-worktree` 限域、路径不下发；`git.*` 真实实现落 `ade-bridge`（`ade-plugins` 不依赖 `ade-git`）。UI `HostRepository` 经 panel 桥取数；分页 `skip+=limit` 无限滚动、后端过滤。快照 `GitHistoryResult` serde（camelCase）不变。

**Tech Stack:** Rust（`ade-git`/`ade-plugins`/`ade-bridge`）、TypeScript、React 19、zod、vitest、cargo。

**Spec:** `docs/superpowers/specs/2026-10-18-git-log-plugin-increment2b-design.md`

## Global Constraints

- 沿用 capability `git:read`（**不新增 kind**）；`git.log`/`git.refs` 均 `scope:active-worktree`、`panel:true`、`mutation:false`、只读。
- `ade-plugins` **不得依赖 `ade-git`**；真实 git 调用只在 `ade-bridge` 的 `BridgePluginHostServices`。
- **路径不下发插件**：`refs=Rev(r)`/`path` 严格校验（`--end-of-options`、`-- <path>` 分隔、rev 先 `resolve_commit` 成 oid）；错误原文只 `eprintln` 宿主侧。
- `GitHistoryResult`/`GitBranchChangeEntry`/`GitHistoryItemRef` 的 Rust serde 保持 `#[serde(rename_all="camelCase")]`；结果 JSON 形状与 TS `@shared/git-history-types` 逐字段一致（分页只增 `hasMore`/items 子集，不改字段名）。
- `history()`（2a 签名 `history(path,limit,base_ref)`）**保持向后兼容**：内部转调 `history_with`，ade-bridge/其它既有调用不破。
- 分页：`--skip=<skip> -n{limit+1}`，`hasMore = parsed.len() > limit`，`items = take(limit)`。默认页 `limit=100`（契约 1..200）。
- 不引入写操作；不改现有 Source Control UI / 现有 git bridge 命令语义；`ade-git` 只增不改现有行为。
- metadata（current/remote/mergeBase/ahead/behind）恒按 HEAD 计算，不随 `refs` 过滤/分页变。
- 错误码复用既有集：bad rev / unknown oid → `unknown-oid`；非 git 仓 → `not-a-git-repo`；无 active worktree → `no-active-worktree`；意外失败 → `unavailable`。
- 本增量有 Rust 改动 → `cargo test --workspace` 必跑；**运行前先 `pnpm build:orcinus-cli`** 产出 sidecar（否则 `orcinus-app` build.rs 报缺 `binaries/orcinus-<triple>`）。

---

## File Structure

后端（`src-tauri/crates/ade-git/src/history.rs`）：`GitHistoryQuery` 类型 + `GitHistoryRefs` 枚举 + `history_with(path, query)` + `list_refs(path)`；`history()` 转薄封装。
Rust host 面：`ade-plugins/src/types.rs`（无新 kind）；`ade-plugins/src/host/api.rs`（git.log params 校验器扩展 + 新 `git.refs` MethodSpec + refs 结果校验器；SPECS 18→19）；`ade-plugins/src/host/services.rs`（trait `git_log` 签名加 query、新增 `git_refs`，默认实现）；`ade-plugins/src/host/methods.rs`（dispatch `git.refs` + `git.log` 解析 query + FakeServices 桩）；`ade-bridge/src/plugins_service.rs`（`git_log` 走 `history_with`、`git_refs` 走 `list_refs`、`is_inside_work_tree` 缓存）。
TS 面：`src/shared/plugins/plugin-host-api.ts`（git.log params 扩、加 git.refs）；`src/shared/plugins/plugin-panel-bridge.ts`（错误码 union）；`plugins/git-log/src/domain/types.ts`（query 类型 + `gitRefsSchema` + `gitHistoryResultSchema` 不变复用）；`plugins/git-log/src/domain/host-repository.ts`（loadSnapshot(query)/loadRefs/loadMore）；`plugins/git-log/src/domain/query.ts`（纯 reducer：queryToLogParams/mergePages/applyFilters）。
UI：`plugins/git-log/src/ui/{Toolbar,GraphList,GitLogView}.tsx`（选择器/chips/无限滚动/分页态）。
测试与文档：各 `.test.ts`/`.test.tsx`/Rust tests；`docs/phase4-git-log-plugin-record.md` 追加 2b 终态。

---

## Task 1: 后端 `ade_git` 查询扩展（GitHistoryQuery / history_with / list_refs）

**Files:**
- Modify: `src-tauri/crates/ade-git/src/history.rs`
- Test: `src-tauri/crates/ade-git/src/history.rs`（`#[cfg(test)]` 内，沿用现有临时 git 仓测试风格）或 `src-tauri/crates/ade-git/tests/history_query.rs`

**Interfaces:**
- Consumes: 现有 `run`/`run_text`、`resolve_commit`、`parse_git_history_log`、`GIT_HISTORY_COMMIT_FORMAT`、`git_history_ref_from_full_name`、`GitHistoryItem/Ref/Result`（camelCase serde）、`GIT_HISTORY_MAX_LIMIT=200`。
- Produces:
  - `pub enum GitHistoryRefs { Head, All, Rev(String) }`
  - `pub struct GitHistoryQuery { refs: GitHistoryRefs, author: Option<String>, since: Option<String>, until: Option<String>, grep: Option<String>, path: Option<String>, skip: u32, limit: u32 }`（`Default`＝Head/skip0/limit 默认）
  - `pub fn history_with(worktree_path: &str, query: &GitHistoryQuery) -> Result<GitHistoryResult, CoreError>`
  - `pub fn list_refs(worktree_path: &str) -> Result<Vec<GitHistoryItemRef>, CoreError>`

- [ ] **Step 1: 写失败测试（history_with：--all + author 过滤 + skip 分页）**

`src-tauri/crates/ade-git/tests/history_query.rs`:

```rust
use ade_git::history::{history_with, list_refs, GitHistoryQuery, GitHistoryRefs};
use std::process::Command;

fn git(dir: &str, args: &[&str]) {
    let ok = Command::new("git").arg("-C").arg(dir)
        .args(["-c","user.name=T","-c","user.email=t@e.com","-c","commit.gpgsign=false"])
        .args(args).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &["init","-b","main","."]);
    std::fs::write(d.path().join("a"), "1").unwrap();
    git(p, &["add","."]); git(p, &["commit","-m","one"]);
    git(p, &["-c","user.name=Lin","-c","user.email=l@e.com","checkout","-b","feature"]);
    std::fs::write(d.path().join("b"), "2").unwrap();
    git(p, &["add","."]); git(p, &["commit","-m","feat two"]);
    git(p, &["checkout","main"]);
    std::fs::write(d.path().join("a"), "1\n2").unwrap();
    git(p, &["add","."]); git(p, &["commit","-m","two on main"]);
    d
}

#[test]
fn head_scope_excludes_feature_only_commit() {
    let d = repo(); let p = d.path().to_str().unwrap();
    let q = GitHistoryQuery { refs: GitHistoryRefs::Head, limit: 50, ..Default::default() };
    let r = history_with(p, &q).unwrap();
    assert!(!r.items.iter().any(|i| i.subject == "feat two")); // feature-only 不在 HEAD 可达
}

#[test]
fn all_scope_includes_both_branches() {
    let d = repo(); let p = d.path().to_str().unwrap();
    let q = GitHistoryQuery { refs: GitHistoryRefs::All, limit: 50, ..Default::default() };
    let r = history_with(p, &q).unwrap();
    assert!(r.items.iter().any(|i| i.subject == "feat two"));
    assert!(r.items.iter().any(|i| i.subject == "two on main"));
}

#[test]
fn author_filter_and_skip_paging_and_has_more() {
    let d = repo(); let p = d.path().to_str().unwrap();
    let q = GitHistoryQuery { refs: GitHistoryRefs::All, author: Some("Lin".into()), limit: 50, ..Default::default() };
    let r = history_with(p, &q).unwrap();
    assert!(r.items.iter().all(|i| i.author.as_deref() == Some("Lin")));
    // 分页：limit=1 → hasMore true、skip=1 取到第二条
    let q1 = GitHistoryQuery { refs: GitHistoryRefs::All, limit: 1, ..Default::default() };
    let page0 = history_with(p, &q1).unwrap();
    assert_eq!(page0.items.len(), 1);
    assert!(page0.has_more);
    let q2 = GitHistoryQuery { refs: GitHistoryRefs::All, limit: 1, skip: 1, ..Default::default() };
    let page1 = history_with(p, &q2).unwrap();
    assert_ne!(page0.items[0].id, page1.items[0].id);
}

#[test]
fn list_refs_categorizes_branches_and_remotes() {
    let d = repo(); let p = d.path().to_str().unwrap();
    let refs = list_refs(p).unwrap();
    assert!(refs.iter().any(|r| r.name == "main" && format!("{:?}", r.category).contains("Branches")));
    assert!(refs.iter().any(|r| r.name == "feature"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-git --test history_query`
Expected: FAIL（`history_with`/`list_refs`/类型未定义）

- [ ] **Step 3: 定义 query 类型 + history_with + list_refs**

在 `ade-git/src/history.rs`：新增类型（`#[derive(Clone, Debug, Default)]`；`GitHistoryRefs` 默认 `Head`）。把现有 `history()` 的主体改名为 `history_with(worktree_path, query)`，其中：

- 现有 HEAD 解析、`current_ref`/`remote_ref`/`base_ref`/`merge_base` 计算块**保持不变、无条件执行**（与 query.refs 无关）。
- 替换那段 `git log` 命令构造（`run(... &["log", format!("--format={GIT_HISTORY_COMMIT_FORMAT}"), "-z","--topo-order","--decorate=full", format!("-n{}", limit+1), &head_oid])`）为按 `query` 组装：

```rust
let limit = query.limit.clamp(1, GIT_HISTORY_MAX_LIMIT);
let mut args: Vec<String> = vec![
    "log".into(),
    format!("--format={GIT_HISTORY_COMMIT_FORMAT}"),
    "-z".into(),
    "--topo-order".into(),
    "--decorate=full".into(),
];
match &query.refs {
    GitHistoryRefs::Head => { /* 保留原 head_oid 作为范围末尾，见下 */ }
    GitHistoryRefs::All => args.push("--all".into()),
    GitHistoryRefs::Rev(r) => {
        let oid = resolve_commit(worktree_path, r).ok_or_else(|| CoreError::new(/* unknown-oid 风格 */))?;
        args.push(oid);
    }
}
if let Some(a) = &query.author { args.push(format!("--author={a}")); }
if let Some(g) = &query.grep { args.push(format!("--grep={g}")); }
if let Some(s) = &query.since { args.push(format!("--since={s}")); }
if let Some(u) = &query.until { args.push(format!("--until={u}")); }
if query.skip > 0 { args.push(format!("--skip={}", query.skip)); }
args.push(format!("-n{}", limit + 1));
if let Some(path) = &query.path {
    if path.is_empty() || path.starts_with('-') { return Err(CoreError::/* invalid path */); }
    args.push("--".into()); args.push(path.clone());
}
// Head 变体：把 head_oid 作为范围 rev 放在过滤串之前、`--` 之前。
```

> Head 变体需在拼 `--author/...`/`-n` 之前放入 `head_oid`（revs 位置参数须在 pathspec `--` 前）。实现时：先 `args.push(rev)`（Head→`head_oid`；All→无；Rev→解析得的 oid），再 push 过滤/`--skip`/`-n`/pathspec。若 `refs=Head` 且 `head_oid` 解析失败（空仓），沿用现有 `history()` 的"无 HEAD→返回空结果"早退。`limit+1`/`take(limit)`/`has_more` 逻辑保持。

`history()` 改为：
```rust
pub fn history(worktree_path: &str, limit: Option<u32>, base_ref: Option<&str>) -> Result<GitHistoryResult, CoreError> {
    let query = GitHistoryQuery {
        refs: GitHistoryRefs::Head,
        skip: 0,
        limit: clamp_history_limit(limit),
        ..Default::default()
    };
    // base_ref 语义（若有）：保留原逻辑——如原 history 用 base_ref 影响 metadata，则 history_with 需带 base_ref 字段。
    history_with(worktree_path, &query)
}
```
> 若现有 `history()` 的 `base_ref` 参数确有实质作用，则把它并入 `GitHistoryQuery`（加 `base_ref: Option<String>`）并在 metadata 块使用；否则忽略。**实现者：核对 `history()` 里 `base_ref` 是否被用——2a 里 ade-bridge 传的是 `None`。**

新增 `list_refs`：
```rust
pub fn list_refs(worktree_path: &str) -> Result<Vec<GitHistoryItemRef>, CoreError> {
    let stdout = run_text(worktree_path, &[
        "for-each-ref", "--format=%(refname)%09%(objectname)",
        "refs/heads", "refs/remotes", "refs/tags",
    ])?;
    let mut out = Vec::new();
    for line in stdout.lines() {
        let mut it = line.split('\t');
        let (Some(full), Some(oid)) = (it.next(), it.next()) else { continue };
        if full.is_empty() { continue; }
        out.push(git_history_ref_from_full_name(full, /* short */ shorten_ref(full), oid));
    }
    Ok(out)
}
```
> `git_history_ref_from_full_name(full_name, short_name, oid)` 已存在（`history.rs:365` 附近，被 `resolve_upstream_ref`/`resolve_named_ref` 用）；`shorten_ref` 去掉 `refs/heads/`、`refs/remotes/`→`origin/x`、`refs/tags/` 前缀（复用该函数内已有的分类逻辑或就近实现）。测试断言 category 含 `Branches` 依赖 `GitHistoryRefCategory` 的 Debug；若 `git_history_ref_from_full_name` 签名不同，按其真实签名调用。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p ade-git`
Expected: PASS（含 `history_query`；且既有 `history()` 相关测试不破）

- [ ] **Step 5: 若 ade-bridge 编译因 `history()` 行为微调受影响，一并修**

Run: `cargo check -p ade-bridge`
Expected: 通过（`history()` 向后兼容）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/crates/ade-git/src/history.rs src-tauri/crates/ade-git/tests/history_query.rs
git commit -m "feat(git-log): ade_git 查询扩展 GitHistoryQuery/history_with/list_refs（--all + 过滤 + skip 分页），history 向后兼容（增量 2b T1）"
```

---

## Task 2: TS host 面（git.log 扩参 + git.refs + 错误码 union）

**Files:**
- Modify: `src/shared/plugins/plugin-host-api.ts`
- Modify: `src/shared/plugins/plugin-panel-bridge.ts`
- Test: `src/shared/plugins/plugin-git-host-api.test.ts`（扩）

**Interfaces:**
- Consumes: `git:read`（已有）。
- Produces: `git.log` params 扩为 query 字段；新增 `git.refs`（params `{}`）；`PluginPanelActionErrorCode` 增 4 码。

- [ ] **Step 1: 扩测试（plugin-git-host-api.test.ts 追加）**

```ts
it('git.log 接受 query 字段并拒非法', () => {
  const m = byName('git.log')!
  expect(m.params.safeParse({ refs: 'all', author: 'Lin', since: '2026-01-01', skip: 0, limit: 100 }).success).toBe(true)
  expect(m.params.safeParse({ refs: 'refs/heads/feature' }).success).toBe(true)
  expect(m.params.safeParse({ limit: 201 }).success).toBe(false)
  expect(m.params.safeParse({ skip: -1 }).success).toBe(false)
  expect(m.params.safeParse({ nope: 1 }).success).toBe(false) // strict
})
it('git.refs 存在、无参、panel-callable、capability git:read', () => {
  const m = byName('git.refs')
  expect(m).toMatchObject({ panel: true, capability: 'git:read', mutation: false, scope: 'active-worktree' })
  expect(m!.params.safeParse({}).success).toBe(true)
})
```
错误码：在 `plugin-panel-bridge.ts` 的 `PluginPanelActionErrorCode` union 追加 `'no-active-worktree' | 'not-a-git-repo' | 'unknown-oid' | 'unavailable'`；断言：
```ts
import { PluginPanelActionErrorCode } from './plugin-panel-bridge'
// 类型层：编译期即可；运行时用一个 z.enum 的 .enum 断言（若导出）
```
（若该类型未导出运行时数组，仅做 `expectTypeOf` 或跳过运行时断言，类型由 tsc 保证。）

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test src/shared/plugins/plugin-git-host-api.test.ts`
Expected: FAIL

- [ ] **Step 3: 扩 git.log params + 加 git.refs**

`plugin-host-api.ts`：改 `git.log.params`：
```ts
params: z.object({
  refs: z.union([z.literal('head'), z.literal('all'), z.string().min(1).max(255)]).optional(),
  author: z.string().min(1).max(200).optional(),
  since: z.string().min(1).max(64).optional(),
  until: z.string().min(1).max(64).optional(),
  grep: z.string().min(1).max(200).optional(),
  path: z.string().min(1).max(1024).optional(),
  skip: z.number().int().min(0).max(1_000_000).optional(),
  limit: z.number().int().min(1).max(200).optional(),
}).strict(),
result: pluginJsonValueSchema
```
新增 `git.refs`：
```ts
spec({
  name: 'git.refs', since: '1.1', scope: 'active-worktree',
  capability: 'git:read', mutation: false, panel: true,
  params: z.object({}).strict(),
  result: pluginJsonValueSchema,
}),
```

- [ ] **Step 4: 错误码 union**

`plugin-panel-bridge.ts` 的 `PluginPanelActionErrorCode`（约 81-90）追加四个码。

- [ ] **Step 5: 运行 + typecheck**

Run: `pnpm test src/shared/plugins && pnpm typecheck`
Expected: PASS（含 bridge 折叠逻辑测试仍绿——新码不再折叠）

- [ ] **Step 6: Commit**

```bash
git add src/shared/plugins/plugin-host-api.ts src/shared/plugins/plugin-panel-bridge.ts src/shared/plugins/plugin-git-host-api.test.ts
git commit -m "feat(git-log): TS host 面 git.log 扩 query + 新 git.refs + 错误码 union（增量 2b T2）"
```

---


## Task 3: Rust host 面（MethodSpec / trait 签名 / dispatch / 桩 / gate）

**Files:**
- Modify: `src-tauri/crates/ade-plugins/src/host/api.rs`
- Modify: `src-tauri/crates/ade-plugins/src/host/services.rs`
- Modify: `src-tauri/crates/ade-plugins/src/host/methods.rs`
- Modify: `src-tauri/crates/ade-plugins/src/service.rs`（FakeHostServices）
- Modify: `src-tauri/crates/ade-plugins/src/host/gate.rs`（panel-surface 测试）

**Interfaces:**
- Consumes: T2 的方法名 `git.log`(query)/`git.refs`、capability `git:read`。
- Produces:
  - `pub struct GitLogQuery { refs: Option<String>, author: Option<String>, since: Option<String>, until: Option<String>, grep: Option<String>, path: Option<String>, skip: u32, limit: u32 }`（`ade-plugins`，无 ade-git 依赖）
  - trait `fn git_log(&self, worktree_id: Option<&str>, query: &GitLogQuery) -> Result<Value, HostServiceError>`（**替换 2a 的 `git_log(worktree_id, limit)`**）
  - trait `fn git_refs(&self, worktree_id: Option<&str>) -> Result<Value, HostServiceError>`（默认 `unavailable`）

- [ ] **Step 1: 加 `GitLogQuery` + trait 方法（含默认实现）**

`services.rs`：新增 `GitLogQuery`（`#[derive(Clone, Debug, Default)]`，`skip:0, limit:100`）。把 2a 的 `git_log` 签名改为接收 `&GitLogQuery`（保留默认体 `unavailable`），新增 `git_refs`（默认 `unavailable`）。

- [ ] **Step 2: api.rs — 扩 `git_log_params` + 加 `git.refs` MethodSpec**

`git_log_params`：允许并规范化 `refs`(head/all/非 `-` 起、1..255)、`author`、`since`、`until`、`grep`、`path`(非 `-` 起)、`skip`(0..1_000_000)、`limit`(1..200)；`.strict()` 语义（未知键→`Err`）。返回规范化 `Map`。新增 MethodSpec：
```rust
MethodSpec { name: "git.refs", capability: Capability::GitRead, mutation: false, panel_callable: true, params: empty_object_params, result: git_result },
```
`static SPECS: [MethodSpec; 18]`→`[MethodSpec; 19]`；`pub fn specs() -> &'static [MethodSpec; 19]`。

- [ ] **Step 3: methods.rs — dispatch + handler**

`dispatch` 加 `"git.refs" => git_refs_handler(params, ctx)`；把 2a `git_log` handler 改为解析 query：
```rust
fn git_log_handler(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let worktree_id = resolve_worktree(ctx);
    let query = GitLogQuery {
        refs: params.get("refs").and_then(Value::as_str).map(str::to_string),
        author: params.get("author").and_then(Value::as_str).map(str::to_string),
        since: params.get("since").and_then(Value::as_str).map(str::to_string),
        until: params.get("until").and_then(Value::as_str).map(str::to_string),
        grep: params.get("grep").and_then(Value::as_str).map(str::to_string),
        path: params.get("path").and_then(Value::as_str).map(str::to_string),
        skip: params.get("skip").and_then(Value::as_u64).unwrap_or(0) as u32,
        limit: params.get("limit").and_then(Value::as_u64).unwrap_or(100) as u32,
    };
    Ok(ctx.services.git_log(worktree_id.as_deref(), &query)?)
}
fn git_refs_handler(_params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let worktree_id = resolve_worktree(ctx);
    Ok(ctx.services.git_refs(worktree_id.as_deref())?)
}
```
（沿用 2a 的 `git.log`→改名 `git_log_handler`，或保留 `git_log` 名并同步 dispatch。）

- [ ] **Step 4: 更新两处 FakeServices**

`methods.rs` 的 `FakeServices` 与 `service.rs` 的 `FakeHostServices`：`git_log` 改签名 `(worktree_id, _query)`、加 `git_refs`，返回既有桩 JSON（`git_refs`→`json!([])`）。

- [ ] **Step 5: gate 测试 panel-surface 7→8**

`gate.rs` `panel_surface_is_exactly_the_*_panel_callable_methods`：追加 `"git.refs"`；改名 …eight…；`method_for(GitRead)` 保持 `Some("git.log")`。api.rs 元数据测试：`specs().len()`→19、names 列表加 `git.refs`、panel 列表加 `git.refs`。

- [ ] **Step 6: cargo test -p ade-plugins**

Run: `cargo test -p ade-plugins`
Expected: PASS（gate/dispatch/元数据；ade-bridge 因默认实现仍可编译）

- [ ] **Step 7: Commit**

```bash
git add src-tauri/crates/ade-plugins/src
git commit -m "feat(git-log): Rust host 面 git.log 扩 query + git.refs（MethodSpec/trait/dispatch/桩/gate，ade-bridge 待实现）（增量 2b T3）"
```

---

## Task 4: ade-bridge 真实实现（history_with / list_refs / 探测缓存）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/plugins_service.rs`
- Test: `src-tauri/crates/ade-bridge/tests/plugins_service.rs`

**Interfaces:**
- Consumes: `ade_git::history::{history_with, list_refs, GitHistoryQuery, GitHistoryRefs}`（T1）、`GitLogQuery`（T3）、`WorktreeContexts`。
- Produces: 覆盖 `git_log`（走 `history_with`）、`git_refs`（走 `list_refs`）；`is_git_worktree(path)` 缓存。

- [ ] **Step 1: 写失败测试（git_log refs=all + 过滤 + 分页；git_refs）**

`tests/plugins_service.rs`（复用 `RepoWorktrees`/`init_git_repo`；扩一个多分支仓 fixture `init_git_repo_multi` 或直接复用 T1 风格）：
```rust
#[test]
fn git_log_all_author_filter_and_refs_wired() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init_git_repo_multi(repo.path()); // 两分支两作者
    let services = git_host_services(root.path(), ui_store(root.path(), Some(WORKTREE)),
        Arc::new(RepoWorktrees { path: repo.path().to_string_lossy().into_owned() }));
    let all = ade_plugins::GitLogQuery { refs: Some("all".into()), ..Default::default() };
    let v = services.git_log(Some(WORKTREE), &all).unwrap();
    assert!(v["items"].as_array().unwrap().len() >= 2);
    let by_author = ade_plugins::GitLogQuery { refs: Some("all".into()), author: Some("Lin".into()), ..Default::default() };
    let v2 = services.git_log(Some(WORKTREE), &by_author).unwrap();
    assert!(v2["items"].as_array().unwrap().iter().all(|i| i["author"].as_str() == Some("Lin")));
    let refs = services.git_refs(Some(WORKTREE)).unwrap();
    assert!(refs.as_array().unwrap().iter().any(|r| r["name"].as_str() == Some("feature")));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p ade-bridge git_log_all_author_filter_and_refs_wired`
Expected: FAIL（git_log 仍走旧 `history`/git_refs 未实现）

- [ ] **Step 3: 实现 git_log（history_with）+ git_refs（list_refs）+ 缓存**

`plugins_service.rs`：
```rust
fn git_log(&self, worktree_id: Option<&str>, query: &GitLogQuery) -> Result<Value, HostServiceError> {
    let path = self.resolve_worktree_path(worktree_id)?;
    self.assert_git_repo(&path)?;
    let refs = match query.refs.as_deref() {
        None | Some("head") => GitHistoryRefs::Head,
        Some("all") => GitHistoryRefs::All,
        Some(r) => GitHistoryRefs::Rev(r.to_string()),
    };
    let ade = ade_git::history::GitHistoryQuery {
        refs, author: query.author.clone(), since: query.since.clone(), until: query.until.clone(),
        grep: query.grep.clone(), path: query.path.clone(), skip: query.skip, limit: query.limit,
    };
    let result = ade_git::history::history_with(&path, &ade)
        .map_err(|e| { eprintln!("[ade-bridge] git.log failed: {e}"); HostServiceError::new("not-a-git-repo", "git history is unavailable") })?;
    serde_json::to_value(result).map_err(|e| { eprintln!("[ade-bridge] {e}"); HostServiceError::new("unavailable", "git history is unavailable") })
}
fn git_refs(&self, worktree_id: Option<&str>) -> Result<Value, HostServiceError> {
    let path = self.resolve_worktree_path(worktree_id)?;
    self.assert_git_repo(&path)?;
    let refs = ade_git::history::list_refs(&path)
        .map_err(|e| { eprintln!("[ade-bridge] git.refs failed: {e}"); HostServiceError::new("not-a-git-repo", "refs unavailable") })?;
    serde_json::to_value(refs).map_err(|_| HostServiceError::new("unavailable", "refs unavailable"))
}
```
缓存：把 2a 内联的 `is_inside_work_tree` 判断抽成 `assert_git_repo(&path)`，并用 `Mutex<HashSet<String>>`（或 `LruCache`）记住"已确认为 git 仓"的 path，命中即跳过 spawn；`Rev(r)` 解析失败由 `history_with` 返回 Err → 统一 `unknown-oid`（在此 `map_err` 按 CoreError 变体区分；若难区分则沿用 `not-a-git-repo`，UI 靠 message，可接受）。

- [ ] **Step 4: 运行 + 全 ade-bridge**

Run: `cargo test -p ade-bridge`
Expected: PASS（含既有 2a git_log 测试与新；若 2a `git_log(Some, None)` 调用因签名变，改测试传 `&GitLogQuery::default()`）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/crates/ade-bridge
git commit -m "feat(git-log): ade-bridge git.log 走 history_with + 新 git.refs + is_git_worktree 缓存（增量 2b T4）"
```

---

## Task 5: UI 数据层（query/refs/分页 reducer + HostRepository）

**Files:**
- Create: `plugins/git-log/src/domain/query.ts`
- Create: `plugins/git-log/src/domain/query.test.ts`
- Modify: `plugins/git-log/src/domain/types.ts`（`GitLogRepository` 接口 + `gitRefsSchema` + `GitLogQuery` 类型）
- Modify: `plugins/git-log/src/domain/host-repository.ts`
- Modify: `plugins/git-log/src/fixtures/fixture-repository.ts`（实现新接口）
- Modify: `plugins/git-log/src/domain/types.ts`

**Interfaces:**
- Produces:
  - `type GitLogFilter = { refs: 'head'|'all'|string; author?: string; since?: string; until?: string; grep?: string; path?: string }`
  - `GitLogRepository { loadSnapshot(filter, skip): Promise<GitLogSnapshot>; loadCommitFiles(oid); loadRefs(): Promise<GitHistoryItemRef[]> }`
  - 纯函数 `logParams(filter, skip, limit)`、`mergePages(prev, next)`（分页累加，按 id 去重）
  - `gitRefsSchema`（`GitHistoryItemRef[]`）

- [ ] **Step 1: 写失败测试（reducer 纯函数 + HostRepository）**

`query.test.ts`（node）：
```ts
import { expect, it } from 'vitest'
import { logParams, mergePages, type GitLogFilter } from './query'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'

it('logParams 映射 filter→git.log params（默认页 100）', () => {
  const f: GitLogFilter = { refs: 'all', author: 'Lin', path: 'src/a.ts' }
  expect(logParams(f, 0)).toEqual({ refs: 'all', author: 'Lin', path: 'src/a.ts', skip: 0, limit: 100 })
})
it('mergePages 累加并按 id 去重', () => {
  const a = buildFixtureSnapshot(); const b = buildFixtureSnapshot()
  const merged = mergePages(a, b)
  expect(merged.items.length).toBe(a.items.length) // 全重复 → 不增
})
```
HostRepository 测试（happy-dom，扩 `host-repository.test.ts`）加 `loadRefs`/带参 `loadSnapshot`：
```ts
it('loadSnapshot(filter,skip) 发 git.log 带 query', async () => {
  stubParentReply({ ok: true, value: buildFixtureSnapshot() })
  const snap = await new HostRepository().loadSnapshot({ refs: 'all' }, 100)
  expect(snap.items.length).toBeGreaterThanOrEqual(30)
})
it('loadRefs 发 git.refs', async () => {
  stubParentReply({ ok: true, value: [{ id: 'refs/heads/main', name: 'main', category: 'branches' }] })
  const refs = await new HostRepository().loadRefs()
  expect(refs[0]?.name).toBe('main')
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/domain/query.test.ts`
Expected: FAIL

- [ ] **Step 3: query.ts + types 接口 + gitRefsSchema**

`query.ts`：`export interface GitLogFilter {...}`；`logParams(filter, skip, limit=100)`（剔除 undefined）；`mergePages(prev, next): GitHistoryResult`（`items` 拼接按 `id` 去重、其余字段取 next 的 metadata/hasMore/limit）。`types.ts`：`GitLogRepository` 改新签名；加 `gitRefsSchema`（refSchema 数组）；`GitLogSnapshot` 不变。

- [ ] **Step 4: HostRepository + FixtureRepository 实现新接口**

`HostRepository`：`loadSnapshot(filter,skip)`→`gitHistoryResultSchema.parse(callHost('git.log', logParams(filter,skip)))`；`loadRefs()`→`gitRefsSchema.parse(callHost('git.refs', {}))`；`loadCommitFiles` 不变。`FixtureRepository`：`loadSnapshot(filter,skip)` 对 fixture 本地按 filter refs/author/grep 粗筛 + skip 切片（够测试；不需与后端等价）；`loadRefs()` 返回 fixture 的 refs。

- [ ] **Step 5: 运行测试通过**

Run: `pnpm test plugins/git-log/src/domain`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add plugins/git-log/src/domain plugins/git-log/src/fixtures
git commit -m "feat(git-log): UI 数据层 query reducer + HostRepository(loadSnapshot(filter,skip)/loadRefs)（增量 2b T5）"
```

---

## Task 6: UI 组件（分支选择器 / 过滤 chips / 无限滚动 / 分页态）

**Files:**
- Modify: `plugins/git-log/src/ui/Toolbar.tsx`（refs 选择 + author/date/path 控件 + search）
- Modify: `plugins/git-log/src/ui/GitLogView.tsx`（filter state/refs/skip/hasMore/loadingMore + load/loadMore）
- Modify: `plugins/git-log/src/ui/GraphList.tsx`（触底回调 onLoadMore）
- Modify: `plugins/git-log/src/ui/select-rows.ts`（refs 过滤已是本地；grep 走后端）
- Test: `plugins/git-log/src/ui/git-log-view.test.tsx`（扩：选择器/过滤变更重查；loadMore 累加，纯 reducer 部分）

**Interfaces:** Consumes T5 的 `GitLogRepository`/`GitLogFilter`/`mergePages`。Produces：可交互真查询 UI。

- [ ] **Step 1: 扩视图测试（happy-dom 冒烟 + reducer）**

在 `git-log-view.test.tsx` 增：mount 后 `loadRefs` 被调、选择"全部分支"触发 `git.log refs=all`（用 stub 记录 callHost action/params），"加载更多"累加 `rows`。因依赖 virtualizer，只断言"已发起 git.refs/git.log"与"分页累加 reducer"（后者在 query.test 已覆盖，UI 层做冒烟）。

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/ui/git-log-view.test.tsx`
Expected: FAIL（新 prop/行为未接）

- [ ] **Step 3: GitLogView 状态机**

`GitLogView` 加 `filter: GitLogFilter`（默认 `{refs:'head'}`）、`refs: GitHistoryItemRef[]`、`skip`、`hasMore`、`loadingMore`。`load()`：`repository.loadSnapshot(filter, 0)`→`setRows`+`skip=limit`+`hasMore`；`useEffect` 上 `repository.loadRefs()`。`loadMore()`：`repository.loadSnapshot(filter, skip)`→`mergePages` 累加、`skip+=limit`、更新 hasMore；失败保留已加载。filter 变更 → `load()`（重置 skip）。`Toolbar` 传 `onFilterChange`。

- [ ] **Step 4: Toolbar 选择器 + chips**

`Toolbar` 增：`<select>` 分支（Head/全部/`refs` 项，值映射 `filter.refs`）；Author/Date(since/until)/Path 输入 + 搜索(grep) 受控 → `onFilterChange`。保留现有 search（并入 grep）。

- [ ] **Step 5: GraphList onLoadMore**

`GraphList` 在 `virt` 末项进入视口（`scrollElement` 触底）或渲染出一个"Load more"按钮 → `onLoadMore()`（`loadingMore` 时禁用）。

- [ ] **Step 6: 运行 UI 测试 + 全 plugins + build + typecheck**

Run: `pnpm test plugins/git-log && pnpm build:git-log && pnpm typecheck`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add plugins/git-log/src/ui
git commit -m "feat(git-log): UI 分支选择器 + 过滤 chips + 无限滚动分页接真查询（增量 2b T6）"
```

---

## Task 7: 集成门禁 + 契约 + 手工 + 记录

**Files:** Modify `docs/phase4-git-log-plugin-record.md`（2b 终态）。

- [ ] **Step 1: 后端契约对齐校验**：跑 `git.log` 全参数样例（`refs=all`+`author`+`skip`）经 `ade_git::history_with` → serde JSON 能被 `gitHistoryResultSchema.parse` 通过（在 T5 的 host-contract 或新增用例里，用真实 `history_with` 输出样例断言 TS 端 schema）。

- [ ] **Step 2: 全量门禁**

Run: `pnpm build:orcinus-cli && cargo test --workspace && pnpm typecheck && pnpm build:web && pnpm build:git-log && pnpm test`
Expected: 全绿；全量 `pnpm test` 若仅 3 个既有无关时序 flaky（满并行超时、单跑过），记 record，不阻塞。

- [ ] **Step 3: 后端隔离白名单**

Run: `git diff --name-only main...HEAD`
Expected: 落在 `ade-git/history.rs(+tests)`、`ade-plugins/{api,gate,services,methods,service}.rs`、`ade-bridge/{plugins_service.rs,tests}`、`src/shared/plugins/*`、`plugins/git-log/**`、docs。**不**碰 Source Control UI、现有 git bridge 命令、phase3c 文件、`git.commitFiles` 语义。

- [ ] **Step 4: 手工验收（tauri dev + 真多分支仓）**

- [ ] 切「全部分支」→ 图含多分支；选具体分支 → 仅其历史；author/date/path/grep 过滤生效。
- [ ] 滚动到底「加载更多」→ 累加、不重复、不丢；过滤/切分支 → 重置首屏。
- [ ] 非 git 仓 / 无 active worktree → 错误态；`git.refs` 空/失败 → 选择器降级（仅 Head/全部）。

- [ ] **Step 5: 更新 record + 提交**

```bash
git add docs/phase4-git-log-plugin-record.md
git commit -m "docs: 增量 2b 记录（真查询/git.refs/错误码/探测缓存/门禁）与手工清单留痕"
```

---

## Self-Review（规格 → 任务覆盖）

**1. Spec coverage**：§2.1 后端查询→T1；git.refs(list_refs)→T1/T4；host 方法(git.log 扩/git.refs)→T2(TS)+T3(Rust 面)+T4(实现)；TS schema→T5；错误码 union→T2；UI 选择器/chips/无限滚动→T5(数据)+T6(组件)；探测缓存→T4；§3 契约/§4 细则→T1/T3/T4/T7；§5 UI→T5/T6；§6 测试→各 T + T7；§7 隔离→T7 白名单。✓ 无缺项。
**2. Placeholder 扫描**：无 TBD。Rust `CoreError` 构造 / `git_history_ref_from_full_name` 签名 / `LruCache` 可用性处标注"按真实签名/既有依赖适配"——是定位指引（函数确在 `history.rs`），非占位；若某依赖（如 lru crate）不在，则退化为 `HashSet` 缓存或每次 probe（T4 已给兜底）。
**3. 类型一致性**：`GitLogQuery`(ade-plugins T3) ↔ `GitHistoryQuery`(ade-git T1) 字段一一对应；trait `git_log(worktree_id,&GitLogQuery)`/`git_refs(worktree_id)` 在 T3 定义、T4 实现、T3 桩覆盖；TS `git.log` params(T2) ↔ methods.rs 解析(T3) ↔ UI logParams(T5) 字段名一致（refs/author/since/until/grep/path/skip/limit）。`GitLogRepository` 新签名 T5 改，T6 消费，FixtureRepository T5 同步。✓
