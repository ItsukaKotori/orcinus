# Git Log 原生视图（增量 3）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 git-log 插件独有价值（完整 Log 布局、分支范围、后端过滤、offset 分页）迁入主应用原生 Git Log 视图（主编辑器区全宽 tab），复用主应用既有提交/diff/写操作，迁移达平后删除插件。

**Architecture:** 后端把已建好的 `ade_git::history_with`/`list_refs` 包成 tauri 命令 `git_log_query`/`git_refs`；前端新增 `TabContentType:'git-log'`（独立可见类型）+ `openGitLogTab` + 渲染分派；把插件 React 组件移植进 `components/git-log/`，数据层从 `callHost` 换成 `window.api.git`；提交文件双击复用主应用 diff-open；最后退役插件。

**Tech Stack:** Rust（ade-bridge tauri 命令 + ade_git）、TypeScript/React 19、zustand tabs slice、@tanstack/react-virtual、vitest、tauri-specta。

**Spec:** `docs/superpowers/specs/2026-10-21-git-log-native-design.md`

## Global Constraints

- 命令注册：`src-tauri/crates/ade-bridge/src/specta_export.rs` 的 `collect_commands![...]` 列表；命令须 `#[tauri::command] #[specta::specta]`。
- 命令一律先 `require_authorized_worktree(&state.fs, &args.worktree_path)?` 再 `run_blocking(...)`。
- 数据契约 camelCase serde，复用 `GitHistoryResult`/`GitHistoryItem`/`GitHistoryItemRef`/`GitRefsResult`（`@shared/git-history*`）。
- `git_log_query` query 字段名逐字：`refs/author/since/until/grep/path/skip/limit`。
- 注入防护沿用 ade_git（rev 先 `resolve_commit`、path `--` 分隔拒 `-`、日期 `is_date_prefix` 形状）。
- 前端 api 三处同步：`src/bridge/real/git.ts`、`src/bridge/mock/git-api.ts`、`src/preload/api-types.ts`（`PreloadApi['git']`）。
- i18n：新文案 `translate('auto.components.git-log.<hash>', 'English')` + 六 locale（en/zh/ja/ko/fr/es）补键；UI 可见串英文为准。
- 虚拟列表测试抽纯函数避 happy-dom 无布局（既有惯例）。
- 门禁：`pnpm build:orcinus-cli && cargo test --workspace`、`pnpm typecheck`、`pnpm build:web`、`pnpm test`。
- 每任务 TDD + 提交；ade_git 不改（仅新增命令包装）。

---

### Task 1: 后端 `git_log_query` 命令

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/git.rs`（加 args 结构 + 命令）
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`（`collect_commands!` 加 `commands::git::git_log_query`）
- Test: `src-tauri/crates/ade-bridge/tests/git_log_query.rs`（新建）

**Interfaces:**
- Consumes: `ade_git::history::history_with(&str, &GitHistoryQuery) -> Result<GitHistoryResult, CoreError>`；`GitHistoryRefs{Head,All,Rev}`；`GitHistoryQuery{refs,author,since,until,grep,path,base_ref,skip,limit}`（均 pub，2b 建）。
- Produces: tauri 命令 `git_log_query(state, args: GitLogQueryArgs) -> Result<GitHistoryResult, BridgeError>`；`GitLogQueryArgs { worktree_path: String, query: GitLogQueryIn }`，`GitLogQueryIn`（camelCase serde，`#[serde(rename_all="camelCase")]`）字段 `refs: Option<String>, author/since/until/grep/path: Option<String>, skip: Option<u32>, limit: Option<u32>`。

- [ ] **Step 1: 写失败测试** `tests/git_log_query.rs`：临时多分支 git 仓（仿 `ade-git/tests/history_query.rs` 的 `repo()`），命令层不易直接调（需 AppState），故测 `impl` 辅助函数 `git_log_query_impl(worktree_path, &GitLogQueryIn) -> Result<GitHistoryResult, BridgeError>`：`refs=Some("all")` 含两分支、`refs=Some("nope")` 报错、`skip` 分页、`author` 过滤。

- [ ] **Step 2: 运行确认失败** — Run: `cargo test -p ade-bridge --test git_log_query` → 编译失败（`git_log_query_impl` 未定义）。

- [ ] **Step 3: 实现** 在 `commands/git.rs`：
```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLogQueryIn {
    pub refs: Option<String>,
    pub author: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub grep: Option<String>,
    pub path: Option<String>,
    pub skip: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLogQueryArgs {
    pub worktree_path: String,
    pub query: GitLogQueryIn,
}

fn to_history_query(q: &GitLogQueryIn) -> Result<ade_git::history::GitHistoryQuery, BridgeError> {
    use ade_git::history::{GitHistoryQuery, GitHistoryRefs};
    for (k, v) in [("since", q.since.as_deref()), ("until", q.until.as_deref())] {
        if let Some(v) = v {
            if !is_date_prefix(v) {
                return Err(BridgeError::invalid(format!("{k} must be an ISO date")));
            }
        }
    }
    let refs = match q.refs.as_deref() {
        None | Some("head") => GitHistoryRefs::Head,
        Some("all") => GitHistoryRefs::All,
        Some(rev) => GitHistoryRefs::Rev(rev.to_string()),
    };
    Ok(GitHistoryQuery {
        refs,
        author: q.author.clone(),
        since: q.since.clone(),
        until: q.until.clone(),
        grep: q.grep.clone(),
        path: q.path.clone(),
        base_ref: None,
        skip: q.skip.unwrap_or(0),
        limit: q.limit.unwrap_or(100).clamp(1, ade_git::history::GIT_HISTORY_MAX_LIMIT),
    })
}

pub fn git_log_query_impl(worktree_path: &str, q: &GitLogQueryIn) -> Result<GitHistoryResult, BridgeError> {
    let hq = to_history_query(q)?;
    ade_git::history::history_with(worktree_path, &hq).map_err(BridgeError::from_core)
}

#[tauri::command]
#[specta::specta]
pub async fn git_log_query(state: State<'_, AppState>, args: GitLogQueryArgs) -> Result<GitHistoryResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_log_query_impl(&args.worktree_path, &args.query)).await
}
```
（`is_date_prefix` 从 ade-plugins api.rs 复制为 ade-bridge 私有 fn，或命令侧仅校验 refs、日期交给 ade_git；若 `BridgeError::invalid`/`from_core` 名字不同，按 `errors.rs` 现有构造器对齐。）在 `specta_export.rs` `collect_commands!` 加 `commands::git::git_log_query,`。

- [ ] **Step 4: 运行确认通过** — Run: `cargo test -p ade-bridge --test git_log_query` → PASS。

- [ ] **Step 5: 提交** `git add -A && git commit -m "feat(git-log): 后端 git_log_query 命令（history_with 包装，range/过滤/分页/日期形状）（增量 3 T1）"`

---

### Task 2: 后端 `git_refs` 命令

**Files:** Modify `commands/git.rs`、`specta_export.rs`；Test `tests/git_refs.rs`（新建）。

**Interfaces:** Consumes `ade_git::history::list_refs(&str) -> Result<GitRefsResult, CoreError>`（2e 建）。Produces 命令 `git_refs(state, args: GitRefsArgs{worktree_path}) -> Result<GitRefsResult, BridgeError>`。

- [ ] Step 1 失败测试：临时仓含 main/feature/annotated tag → `git_refs_impl(path)` 返回 `{refs:[含 main/feature], truncated:false}`；造 >max 触发 truncated（用 `list_refs_capped` 不可达则跳过，仅断言正常）。
- [ ] Step 2 运行失败。
- [ ] Step 3 实现：
```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRefsArgs { pub worktree_path: String }

pub fn git_refs_impl(worktree_path: &str) -> Result<ade_git::history::GitRefsResult, BridgeError> {
    ade_git::history::list_refs(worktree_path).map_err(BridgeError::from_core)
}

#[tauri::command]
#[specta::specta]
pub async fn git_refs(state: State<'_, AppState>, args: GitRefsArgs) -> Result<ade_git::history::GitRefsResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_refs_impl(&args.worktree_path)).await
}
```
specta_export 加 `commands::git::git_refs,`。
- [ ] Step 4 运行通过。
- [ ] Step 5 提交。

---

### Task 3: 前端 git api 接线（real + mock + 类型）

**Files:** Modify `src/bridge/real/git.ts`、`src/bridge/mock/git-api.ts`、`src/preload/api-types.ts`；Modify `src/bridge/real/generated/tauri-bindings.ts`（specta 重生成）。Test: `src/bridge/real/git.test.ts`。

**Interfaces:** Produces `window.api.git.logQuery(args: {worktreePath, query}) -> Promise<GitHistoryResult>`、`window.api.git.refs(args:{worktreePath}) -> Promise<GitRefsResult>`。

- [ ] Step 1 失败测试（git.test.ts）：断言 `createGitRealApi().logQuery` 调 `invokeCommand('git_log_query', { args })`、`refs` 调 `'git_refs'`。
- [ ] Step 2 运行失败。
- [ ] Step 3 实现：git.ts 加 `logQuery: (args) => invokeCommand('git_log_query', { args }), refs: (args) => invokeCommand('git_refs', { args }),`；api-types.ts `PreloadApi['git']` 加两方法签名；mock/git-api.ts 加返回 fixture 的实现；重跑 specta 绑定生成（若仓库有 `pnpm` 生成脚本则运行，否则手改 tauri-bindings.ts 增两命令）。
- [ ] Step 4 `pnpm test src/bridge/real/git.test.ts` + `pnpm typecheck` 通过。
- [ ] Step 5 提交。

---

### Task 4: tab 扩展 `'git-log'`（类型 + 可见类型 + 绑定 + open action + 渲染分派）

**Files:** Modify `src/shared/tab-types.ts`、`src/renderer/src/store/slices/tabs/tabs-slice-contract.ts`、新建 `src/renderer/src/store/slices/tabs/tabs-git-log-actions.ts`、`create-tabs-slice.ts`（装配）、`src/renderer/src/components/tab-group/TabGroupPanel.tsx`（渲染分支）；Test `tabs-git-log-actions.test.ts`。

**Interfaces:** Produces `TabContentType` 含 `'git-log'`；`WorkspaceVisibleTabType` 含 `'git-log'`；`toVisibleTabType('git-log')==='git-log'`；`Tab.gitLog?: { worktreeId: string; instanceId: string; initialRefs?: string }`；store action `openGitLogTab(input: { worktreeId: string; groupId?: string })`（同 worktree 去重聚焦）。

- [ ] Step 1 失败测试：`openGitLogTab({worktreeId})` 建一个 `contentType:'git-log'` tab；再次调用聚焦同一 tab（不重复）；`toVisibleTabType('git-log')` 返回 `'git-log'`。
- [ ] Step 2 运行失败。
- [ ] Step 3 实现：tab-types.ts 两 union 加 `'git-log'` + `toVisibleTabType` 分支返回自身 + `Tab.gitLog?` 绑定；仿 `tabs-plugin-actions.ts` 写 `createTabsGitLogActions`（去重按 `contentType==='git-log' && gitLog.worktreeId===input.worktreeId`）；`create-tabs-slice.ts` 合并；`tabs-slice-contract.ts` 加 `openGitLogTab` 类型；TabGroupPanel.tsx 在 `activeTab?.contentType === 'plugin'` 分支旁加 `=== 'git-log'` → 暂渲染占位 `<div data-testid="git-log-tab"/>`（Task 5 换真视图）。
- [ ] Step 4 `pnpm test src/renderer/src/store/slices/tabs/tabs-git-log-actions.test.ts` + typecheck 通过。
- [ ] Step 5 提交。

---

### Task 5: 移植 Git Log 视图组件 + tauri 数据仓库 + 接入 tab

**Files:** Create `src/renderer/src/components/git-log/`（移植 `Toolbar.tsx/RefPanel.tsx/GraphList.tsx/CommitDetail.tsx/GitLogView.tsx/format.ts/select-rows.ts` 自 `plugins/git-log/src/ui/`；`query.ts` 自 `plugins/git-log/src/domain/query.ts`）；Create `src/renderer/src/components/git-log/tauri-repository.ts`；Modify `TabGroupPanel.tsx`（占位→`<GitLogView/>`）；Test 移植 `git-log-view.test.tsx`/`select-rows.test.ts`/`query.test.ts`。

**Interfaces:** Consumes `window.api.git.logQuery/refs/commitCompare/commitDiff`（T3）、`GitHistoryResult`/`GitRefs`（`@shared`）。Produces `GitLogView({ repository, worktreeId, onOpenFileDiff })`、`TauriGitLogRepository implements GitLogRepository`。

- [ ] Step 1 写 `tauri-repository.ts`（先失败测试 `tauri-repository.test.ts`：mock `window.api.git`，断言 `loadSnapshot(filter,skip)` 调 `logQuery({worktreePath, query: logParams(filter,skip)})`、`loadRefs()` 调 `refs`）：
```ts
import { logParams, type GitLogFilter } from './query'
import type { GitLogRepository, GitLogSnapshot, GitRefs } from './types'
export class TauriGitLogRepository implements GitLogRepository {
  constructor(private worktreePath: string) {}
  async loadSnapshot(filter: GitLogFilter = {}, skip = 0): Promise<GitLogSnapshot> {
    return window.api.git.logQuery({ worktreePath: this.worktreePath, query: logParams(filter, skip) })
  }
  async loadRefs(): Promise<GitRefs> {
    return window.api.git.refs({ worktreePath: this.worktreePath })
  }
  async loadCommitFiles(oid: string) {
    return (await window.api.git.commitCompare({ worktreePath: this.worktreePath, commitId: oid })).entries
  }
}
```
（`commitCompare` 入参名按现有 api；`types.ts` 从插件 domain 移植。）
- [ ] Step 2 移植组件文件到 `components/git-log/`，逐文件改 import：`@shared/...` 保持；相对路径按新目录；删除对 `host-bridge`/`host-repository` 依赖；`GitLogView` 顶部 `repository` 由外部注入（tab 里 `new TauriGitLogRepository(worktreePath)`）。
- [ ] Step 3 TabGroupPanel `git-log` 分支渲染 `<GitLogView repository={new TauriGitLogRepository(pathFor(tab.gitLog.worktreeId))} worktreeId={tab.gitLog.worktreeId} onOpenFileDiff={...}/>`（path 解析按现有 worktree→path 工具；diff 打开先留 no-op，Task 6 接）。
- [ ] Step 4 移植测试改 import 后运行；`pnpm test src/renderer/src/components/git-log` + typecheck + `pnpm build:web` 通过。
- [ ] Step 5 提交。

---

### Task 6: 提交文件 → diff 打开（复用主应用 diff）

**Files:** Modify `components/git-log/GitLogView.tsx`/`CommitDetail.tsx`（双击文件回调）、`TabGroupPanel.tsx`（提供 `onOpenFileDiff`）；Test 视图回调。

**Interfaces:** Consumes 主应用现有「打开 commit 文件 diff」路径（`sync/git-history-panel.tsx` 的 `onOpenCommitFile` 所连的 store action；核实其签名后复用）。Produces `onOpenFileDiff(commitId, entry)`。

- [ ] Step 1 核实：读 `git-history-panel.tsx` 的 `onOpenCommitFile` 来源（panel.tsx/use-action-dispatch 里连的 open-diff action），确定可复用函数与入参。
- [ ] Step 2 失败测试：GitLogView 双击文件 → 调用注入的 `onOpenFileDiff(commitId, entry)` 一次。
- [ ] Step 3 实现：CommitDetail 文件行 `onDoubleClick` → `onOpenFileDiff`；TabGroupPanel 把该回调接到核实到的 open-diff action（内部走 `git_commit_diff`）。
- [ ] Step 4 `pnpm test src/renderer/src/components/git-log` + typecheck 通过。
- [ ] Step 5 提交。

---

### Task 7: range + 过滤 chips + offset 分页接线（后端驱动）

**Files:** Modify `components/git-log/GitLogView.tsx`（filter/debounce/skip/loadMore 状态机——直接采用插件 `GitLogView.tsx` 已实现的逻辑）、`query.ts`（已含 `logParams/mergePages/PAGE_SIZE`）。Test 移植插件 `git-log-view.test.tsx` 的 skips===[0,2,4]/debounce/reload 竞态用例。

- [ ] Step 1–5：移植插件 GitLogView 的 filter/debounced/skip/hasMore/loadingMore/reloadInFlight/requestSeq/hasLoaded 状态机（2b–2e 已含全部修复），仅把 `repository` 换 `TauriGitLogRepository`；带其测试。运行 `pnpm test src/renderer/src/components/git-log` + typecheck 绿；提交。

---

### Task 8: 分支树 IDEA 化 + truncated 提示

**Files:** Modify `components/git-log/RefPanel.tsx`（分组 branches/remote branches/tags + 搜索框 + 选为范围 + truncated 提示，插件版已有基础）。Test 补搜索/分组断言。

- [ ] Step 1–5：加搜索过滤分支名 + 保持 Current/All/选分支驱动 `filter.refs`；truncated 提示已有；补测试；typecheck/test 绿；提交。

---

### Task 9: 入口（Source Control 按钮 + 命令面板）+ 底部条跳转

**Files:** Modify `right-sidebar/source-control/panel/header-toolbar.tsx`（或 panel.tsx）加「Git Log」按钮 → `openGitLogTab(activeWorktreeId)`；`sync/git-history-panel.tsx` 头部加「在 Git Log 中打开」跳转；命令面板注册 `git.log.open`。Test 按钮触发 openGitLogTab。

- [ ] Step 1–5：加按钮（复用现有 header 按钮组件）+ 跳转按钮 + 命令面板动作；测试点击调用 `openGitLogTab`；typecheck/test 绿；提交。

---

### Task 10: i18n 六 locale 补键

**Files:** Modify `src/renderer/src/i18n/locales/{en,zh,ja,ko,fr,es}.json`（`auto.components.git-log.*` + 入口按钮键）。

- [ ] Step 1–3：抽取新视图所有 `translate(...)` 键，六 locale 各补对应翻译（en 为准，zh 用中文，其余按语言）；跑 i18n 完整性测试（若有 `locale-*.test`/key-parity 测试）。
- [ ] Step 4 `pnpm test src/renderer/src/i18n` + typecheck 绿。
- [ ] Step 5 提交。

---

### Task 11: 退役 git-log 插件

**Files:** Delete `plugins/git-log/`（整目录）；Modify `package.json`（删 `build:git-log` 脚本 + typecheck 末段 `tsc -p plugins/git-log/tsconfig.json`）；Modify `src/shared/plugins/plugin-host-api.ts`（删 `git.log/git.refs/git.commitFiles` spec）、`src/shared/plugins/plugin-capabilities.ts`（删 `git:read`）、`src/bridge/real/plugins.ts`（错误码数组可留通用码，删 git 专属若仅服务这些）、`src-tauri/crates/ade-plugins/src/host/api.rs`（删三 MethodSpec，SPECS 19→16）、`services.rs`（删 `git_log/git_refs/git_commit_files` trait 方法 + `GitLogQuery`）、`methods.rs`（删 dispatch/handler/fakes + 测试）、`gate.rs`（panel 面回退）、`ade-bridge/plugins_service.rs`（删 `git_log/git_refs/git_commit_files` 实现 + `map_core_err` 若无他用）；相关测试同步；docs 更新。

**保留**：`ade_git::history_with/list_refs/GitHistoryQuery/GitRefsResult`（Task 1/2 命令使用）。

- [ ] Step 1 先全仓 grep 确认 `git.log`/`git.refs`/`git.commitFiles`/`git:read`/`GitLogQuery` 无插件外调用方（主应用走 tauri 命令，不经 host 方法）。
- [ ] Step 2 删插件目录 + package.json 脚本/类型检查项。
- [ ] Step 3 删 host 面（TS + Rust）+ 同步 SPECS 计数/panel 面/测试；`cargo test --workspace` + `pnpm typecheck`。
- [ ] Step 4 全门禁：`pnpm build:orcinus-cli && cargo test --workspace && pnpm typecheck && pnpm build:web && pnpm test`（新视图 + 受影响面全绿）。
- [ ] Step 5 更新 `docs/phase4b-git-log-query-record.md`（记插件退役、能力迁主应用）+ 提交。

---

## Self-Review 备注（执行前须核实）

- Task 1 `BridgeError::invalid`/`from_core` 构造器名以 `ade-bridge/src/errors.rs` 实际为准；`is_date_prefix` 若不在 ade-bridge，复制为私有 fn 或改由 ade_git 校验。
- Task 5/6 worktree→path 解析、`commitCompare`/`commitDiff` 入参名、open-diff action 签名，执行时按现有 api 核实（Task 6 Step 1 已含核实步）。
- Task 11 删 host 面牵动 ade-plugins/ade-bridge 多处测试，务必先 grep 无外部调用方再删。
