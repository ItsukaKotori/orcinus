# Git Log 插件 增量 2a（真实 git 数据接入）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Git Log 插件显示当前 worktree 的真实 HEAD 提交历史与单提交变更文件——新增 `git:read` capability 与 `git.log`/`git.commitFiles` 两个只读、panel 可调的 host 方法（复用 Rust `ade-git`），UI 把 `FixtureRepository` 换成经 panel 桥调 host 方法的 `HostRepository`。

**Architecture:** host 方法面纯新增：TS `PLUGIN_HOST_API_V0` + Rust `Capability` 枚举 + `MethodSpec`（`gate::decide` 依据）+ `host/methods.rs` dispatch + `PluginHostServices` trait；`git_*` 的**真实实现落在 `ade-bridge`**（那里同时依赖 `ade-git` 与 worktree 目录），`ade-plugins` 不新增 `ade-git` 依赖；host 内部把 `active_worktree_id → repo path` 后调 `ade_git::history`/`commit_compare`，**路径不下发插件**。UI 快照形状 = `GitHistoryResult` 不变，视图/图谱/主题零改动。

**Tech Stack:** Rust（`ade-plugins` / `ade-bridge` / `ade-git`）、TypeScript、React 19、zod、vitest、cargo。

**Spec:** `docs/superpowers/specs/2026-10-18-git-log-plugin-increment2-design.md`

## Global Constraints

- 能力/方法表**纯新增**：`git:read` capability（10→11）、`git.log`/`git.commitFiles` 方法；不改任何现有 capability/method 语义。
- `ade-plugins` **不得新增对 `ade-git` 的依赖**；真实 git 调用只在 `ade-bridge` 的 `BridgePluginHostServices`。
- `git.log`/`git.commitFiles`：`scope: 'active-worktree'`、`panel: true`、`capability: 'git:read'`、`mutation: false`；**只读**、只作用当前 active worktree、**不返回 fs 路径**。
- 结果契约：Rust serde `rename_all = "camelCase"`（`GitHistoryResult`/`GitBranchChangeEntry` 已是），字段逐一对齐 TS `@shared/git-history-types` 的 `GitHistoryResult`、`@shared/git-diff-compare-types` 的 `GitBranchChangeEntry`。
- `git.log` params `limit` 复用 `GIT_HISTORY_MAX_LIMIT = 200`（1..200，可省）。
- 复用入口：`ade_git::history(worktree_path: &str, limit: Option<u32>, base_ref: Option<&str>) -> Result<GitHistoryResult, CoreError>`；`ade_git::compare::commit_compare(worktree_path: &str, commit_id: &str) -> Result<GitCommitCompareResult, CoreError>`（`.entries: Vec<GitBranchChangeEntry>`）。
- manifest 规范名 `ade-plugin.json`；consent 变为 `workspace:open-tab` + `git:read`（新增 capability → 重新 consent）。
- 本增量**有 Rust 改动** → `cargo test` 必跑（`-p ade-plugins -p ade-bridge`，收尾全量）。
- UI 视图层（GraphList/RefPanel/CommitDetail/select-rows/git-graph-model）与 `@theme inline` parity **不动**；`FixtureRepository` 仅供 `git-log-view.test.tsx` 注入，运行时 `tab-main` 固定 `HostRepository`。

---

## File Structure

修改（Rust host 面）：
- `src-tauri/crates/ade-plugins/src/types.rs` — `Capability` 加 `GitRead` 变体 + `as_str`/`FromStr`/`ALL`。
- `src-tauri/crates/ade-plugins/src/host/gate.rs`（或方法元数据表所在）— 新增 `git.log`/`git.commitFiles` 的 `MethodSpec`（capability `GitRead`、panel_callable `true`）；更新 panel-surface 测试 5→7。
- `src-tauri/crates/ade-plugins/src/host/methods.rs` — `dispatch()` 增 `git.log`/`git.commitFiles` 分支（校验 params → 调 `ctx.services.*`）。
- `src-tauri/crates/ade-plugins/src/host/services.rs` — `PluginHostServices` trait 增 `git_log`/`git_commit_files`（返回 `serde_json::Value`）。
- `src-tauri/crates/ade-plugins/src/{host/methods.rs,service.rs}` 的 `FakeServices` — 各补桩实现。
- `src-tauri/crates/ade-bridge/src/plugins_service.rs` — `BridgePluginHostServices` 实现 `git_log`/`git_commit_files`；`WorktreeContexts` trait 增 `path_for`，其 impl 由 worktree 目录提供路径。

修改（TS host 面）：
- `src/shared/plugins/plugin-capabilities.ts` — `PLUGIN_CAPABILITY_KINDS` 加 `'git:read'` + 描述。
- `src/shared/plugins/plugin-host-api.ts` — `PLUGIN_HOST_API_V0` 加 `git.log`/`git.commitFiles`。

新建/修改（插件 UI）：
- `plugins/git-log/src/ui/host-bridge.ts`（新）— panel 侧 `call(action, params)` postMessage 助手（复用 `orca-panel-action`/`-result`）。
- `plugins/git-log/src/domain/host-repository.ts`（新）— `HostRepository implements GitLogRepository`。
- `plugins/git-log/src/ui/tab-main.tsx` — 注入 `HostRepository`。
- `plugins/git-log/src/ui/GitLogView.tsx` — `load`/`loadCommitFiles` 加 loading/error + try/catch。
- `plugins/git-log/ade-plugin.json` — `capabilities` 加 `git:read`、`version` bump。
- `docs/phase4-git-log-plugin-record.md` — consent/门禁更新。

---

## Task 1: TS host 能力面（git:read + git.log/git.commitFiles）

**Files:**
- Modify: `src/shared/plugins/plugin-capabilities.ts`
- Modify: `src/shared/plugins/plugin-host-api.ts`
- Test: `src/shared/plugins/plugin-git-host-api.test.ts`（新）

**Interfaces:**
- Consumes: 现有 `PLUGIN_CAPABILITY_KINDS`、`spec({...})`、`pluginJsonValueSchema`（`z.json()`）。
- Produces: capability kind `'git:read'`；host 方法 `git.log`（params `{limit?:1..200}`→`z.json()`）、`git.commitFiles`（params `{oid:7..40}`→`z.json()`），二者 `panel:true`/`capability:'git:read'`/`scope:'active-worktree'`/`mutation:false`。

- [ ] **Step 1: 写失败测试**

Create `src/shared/plugins/plugin-git-host-api.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { PLUGIN_HOST_API_V0 } from './plugin-host-api'
import { PLUGIN_CAPABILITY_KINDS } from './plugin-capabilities'

const byName = (n: string) => PLUGIN_HOST_API_V0.find((m) => m.name === n)

describe('git:read host surface', () => {
  it('capability kind 含 git:read', () => {
    expect(PLUGIN_CAPABILITY_KINDS as readonly string[]).toContain('git:read')
  })
  it('git.log panel-callable、capability=git:read、limit 上限 200', () => {
    const m = byName('git.log')
    expect(m).toMatchObject({ panel: true, capability: 'git:read', scope: 'active-worktree', mutation: false })
    expect(m!.params.safeParse({ limit: 200 }).success).toBe(true)
    expect(m!.params.safeParse({ limit: 201 }).success).toBe(false)
    expect(m!.params.safeParse({}).success).toBe(true)
  })
  it('git.commitFiles oid 长度 7..40', () => {
    const m = byName('git.commitFiles')
    expect(m).toMatchObject({ panel: true, capability: 'git:read', mutation: false })
    expect(m!.params.safeParse({ oid: 'abc' }).success).toBe(false)
    expect(m!.params.safeParse({ oid: 'a'.repeat(40) }).success).toBe(true)
  })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test src/shared/plugins/plugin-git-host-api.test.ts`
Expected: FAIL（git:read / git.log 未定义）

- [ ] **Step 3: 加 capability kind**

在 `src/shared/plugins/plugin-capabilities.ts` 的 `PLUGIN_CAPABILITY_KINDS` 数组按字母序加入 `'git:read'`（在 `'events:subscribe'` 与 `'notifications:show'` 之间合适位置），并在 `PLUGIN_CAPABILITY_DESCRIPTIONS` 增：
```ts
  'git:read': 'Read the active worktree commit history and per-commit changed files',
```

- [ ] **Step 4: 加两个 host 方法**

在 `src/shared/plugins/plugin-host-api.ts` 的 `PLUGIN_HOST_API_V0` 数组内新增（放在 `workspace.*` 附近；`since` 用 `'1.1'`）：
```ts
  spec({
    name: 'git.log',
    since: '1.1',
    scope: 'active-worktree',
    capability: 'git:read',
    mutation: false,
    panel: true,
    params: z.object({ limit: z.number().int().min(1).max(200).optional() }).strict(),
    result: pluginJsonValueSchema
  }),
  spec({
    name: 'git.commitFiles',
    since: '1.1',
    scope: 'active-worktree',
    capability: 'git:read',
    mutation: false,
    panel: true,
    params: z.object({ oid: z.string().min(7).max(40) }).strict(),
    result: pluginJsonValueSchema
  }),
```
> `result` 用 `pluginJsonValueSchema`（`z.json()`）：真实形状由 Rust serde（camelCase）产出，UI 侧 `HostRepository` 用 `@shared` 的 `GitHistoryResult`/`GitBranchChangeEntry` 类型消费；契约一致性由 Task 6 的跨端测试锁定。

- [ ] **Step 5: 运行测试 + typecheck**

Run: `pnpm test src/shared/plugins/plugin-git-host-api.test.ts && pnpm typecheck`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/shared/plugins/plugin-capabilities.ts src/shared/plugins/plugin-host-api.ts src/shared/plugins/plugin-git-host-api.test.ts
git commit -m "feat(git-log): TS host 面新增 git:read + git.log/git.commitFiles（增量 2a）"
```

---

## Task 2: Rust 能力枚举 + MethodSpec + dispatch + trait 打通（桩实现）

**Files:**
- Modify: `src-tauri/crates/ade-plugins/src/types.rs`
- Modify: `src-tauri/crates/ade-plugins/src/host/gate.rs`（`MethodSpec`/`decide` 依据的表 + panel-surface 测试）
- Modify: `src-tauri/crates/ade-plugins/src/host/services.rs`（trait）
- Modify: `src-tauri/crates/ade-plugins/src/host/methods.rs`（dispatch + `FakeServices`）
- Modify: `src-tauri/crates/ade-plugins/src/service.rs`（另一处 `FakeServices`）

**Interfaces:**
- Consumes: Task 1 的方法名/capability 字符串（`git.log`/`git.commitFiles`/`git:read`）。
- Produces: `Capability::GitRead`；trait `fn git_log(&self, limit: Option<u32>) -> Result<Value, HostServiceError>` 与 `fn git_commit_files(&self, oid: &str) -> Result<Value, HostServiceError>`；`dispatch("git.log"/"git.commitFiles")` 走 gate → services。

- [ ] **Step 1: 加 Capability 变体**

在 `types.rs` `pub enum Capability { ... }` 增 `GitRead,`；`as_str()`（约 75 行的 match）增 `Capability::GitRead => "git:read",`；`FromStr`/`from_str` 的映射增 `"git:read" => Ok(Capability::GitRead)`；`Capability::ALL` 数组加 `Capability::GitRead`。同步修 `as_str` 单测（约 279 行）新增断言 `Capability::GitRead.as_str() == "git:read"`。

- [ ] **Step 2: 加 MethodSpec 条目 + panel 集合**

定位 `host/gate.rs` 的 `decide` 所读的 `MethodSpec` 表（`workspace.readContext` → `capability: Capability::WorkspaceRead, panel_callable: true` 所在处），新增：
```rust
// "git.log"      => MethodSpec { capability: Capability::GitRead, panel_callable: true,  .. }
// "git.commitFiles" => MethodSpec { capability: Capability::GitRead, panel_callable: true, .. }
```
（沿用该表既有条目的完整字段写法。）更新 `gate.rs` 测试 `panel_surface_is_exactly_the_five_panel_callable_methods`：改名 `..._seven_...`，`panel_methods` 数组追加 `"git.log"`, `"git.commitFiles"`，并核对其余断言计数随之。

- [ ] **Step 3: 加 trait 方法**

`host/services.rs` 的 `PluginHostServices` trait 增：
```rust
    /// Read-only commit history for the caller's active worktree (host-resolved path).
    fn git_log(&self, limit: Option<u32>) -> Result<serde_json::Value, HostServiceError>;
    /// Changed files for one commit in the caller's active worktree.
    fn git_commit_files(&self, oid: &str) -> Result<serde_json::Value, HostServiceError>;
```

- [ ] **Step 4: dispatch 分支**

`host/methods.rs` 的 dispatch `match`（约 213 行 `workspace.readContext => read_context(ctx)` 同级）增：
```rust
        "git.log" => git_log(params, ctx),
        "git.commitFiles" => git_commit_files(params, ctx),
```
并新增处理函数（校验参数后转调 service；无 active worktree → `no-active-worktree`）：
```rust
fn git_log(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let limit = params.get("limit").and_then(Value::as_u64).map(|n| n as u32);
    ctx.services.git_log(limit).map_err(handler_err_from_service)
}
fn git_commit_files(params: &Value, ctx: &HostCallContext<'_>) -> Result<Value, HandlerError> {
    let Some(oid) = params.get("oid").and_then(Value::as_str) else {
        return Err(HandlerError::new("invalid", "missing oid"));
    };
    ctx.services.git_commit_files(oid).map_err(handler_err_from_service)
}
```
> 若该文件已把 `HostServiceError` 转 `HandlerError` 的辅助函数命名为别（如 `map_service_err`），沿用现名。`HandlerError::new` 构造按文件既有写法。

- [ ] **Step 5: 补两处 FakeServices**

`host/methods.rs` 的 `FakeServices` 与 `service.rs` 的 `FakeHostServices`（均 `impl PluginHostServices`）各补：
```rust
    fn git_log(&self, _limit: Option<u32>) -> Result<serde_json::Value, HostServiceError> {
        Ok(serde_json::json!({ "items": [], "hasIncomingChanges": false, "hasOutgoingChanges": false, "hasMore": false, "limit": 50 }))
    }
    fn git_commit_files(&self, _oid: &str) -> Result<serde_json::Value, HostServiceError> {
        Ok(serde_json::json!([]))
    }
```

- [ ] **Step 6: dispatch 单测**

在 `host/methods.rs` 测试模块加：
```rust
    #[test]
    fn git_log_panel_callable_and_delegates() {
        let mut ctx = base_ctx(); // 该文件既有构造 helper
        ctx.via_panel = true;
        ctx.granted = Some(vec![Capability::GitRead]);
        let value = expect_ok(execute("git.log", json!({ "limit": 30 }), &ctx));
        assert_eq!(value["limit"].as_u64(), Some(50)); // FakeServices 桩回默认
    }
```
> `base_ctx`/`execute`/`expect_ok` 用该测试模块既有 helper；若命名不同，替换为文件里实际 helper 名。

- [ ] **Step 7: cargo 编译 + 测试**

Run: `cargo test -p ade-plugins`
Expected: PASS（gate panel-surface=7、dispatch git.log、Capability 单测）

- [ ] **Step 8: Commit**

```bash
git add src-tauri/crates/ade-plugins/src
git commit -m "feat(git-log): Rust 能力面打通 git:read + git.log/git.commitFiles（trait/dispatch/gate，ade-bridge 待实现）（增量 2a）"
```

---

## Task 3: ade-bridge 真实实现（worktree→path + 调 ade-git）

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/plugins_service.rs`（`WorktreeContexts::path_for` + `BridgePluginHostServices::git_log/git_commit_files`）
- Test: `src-tauri/crates/ade-bridge/tests/plugins_service.rs`

**Interfaces:**
- Consumes: Task 2 的 trait `git_log`/`git_commit_files`；`ade_git::history(path,limit,None)`；`ade_git::compare::commit_compare(path,oid).entries`。
- Produces: `WorktreeContexts::path_for(&self, worktree_id: &str) -> Option<String>`（trait + 默认实现或具体实现）。

- [ ] **Step 1: 加 path_for 到 WorktreeContexts**

`plugins_service.rs` 的 `pub trait WorktreeContexts`（约 241 行）增：
```rust
    fn path_for(&self, worktree_id: &str) -> Option<String>;
```
在其具体实现（约 264 行 `fn resolve(...)` 所在 impl，内部已 `list_all_worktrees(...)`）增：从同一 `list_all_worktrees` 结果里按 `worktree_id` 匹配返回其 `path`（`String`）。匹配不到返回 `None`。

- [ ] **Step 2: 实现 git_log（失败测试先行）**

`tests/plugins_service.rs` 增（用一个临时 git 仓库 fixture；沿用该测试文件已有的建仓库/取 worktree_id 的 helper）：
```rust
#[test]
fn git_log_returns_active_worktree_history_camel_case() {
    let (services, worktree_id) = services_with_git_repo(); // 建 2 commit 的临时 repo + 置 lastActiveWorktreeId
    let value = services.git_log(Some(50)).expect("git.log ok");
    assert_eq!(value["items"].as_array().map(|a| a.len()), Some(2));
    assert!(value["items"][0].get("parentIds").is_some()); // camelCase 契约
    assert!(value.get("hasMore").is_some());
}
#[test]
fn git_log_without_active_worktree_is_no_active_worktree() {
    let services = services_with_no_active_worktree();
    let err = services.git_log(None).unwrap_err();
    assert_eq!(err.code(), "no-active-worktree");
}
```

- [ ] **Step 3: 运行确认失败**

Run: `cargo test -p ade-bridge git_log_returns_active_worktree_history_camel_case`
Expected: FAIL（git_log 未实现 / 走 `todo!()`）

- [ ] **Step 4: 实现 git_log / git_commit_files**

`impl PluginHostServices for BridgePluginHostServices`（约 502 行块内）增：
```rust
    fn git_log(&self, limit: Option<u32>) -> Result<Value, HostServiceError> {
        let path = self.active_worktree_path()?;
        let result = ade_git::history::history(&path, limit, None)
            .map_err(|e| HostServiceError::new("not-a-git-repo", e.to_string()))?;
        serde_json::to_value(result).map_err(|e| HostServiceError::new("internal", e.to_string()))
    }
    fn git_commit_files(&self, oid: &str) -> Result<Value, HostServiceError> {
        let path = self.active_worktree_path()?;
        let cmp = ade_git::compare::commit_compare(&path, oid)
            .map_err(|e| HostServiceError::new("unknown-oid", e.to_string()))?;
        serde_json::to_value(cmp.entries).map_err(|e| HostServiceError::new("internal", e.to_string()))
    }
```
并加私有 helper：
```rust
    fn active_worktree_path(&self) -> Result<String, HostServiceError> {
        let id = self
            .active_worktree_context()
            .ok_or_else(|| HostServiceError::new("no-active-worktree", "no active worktree".into()))?
            .worktree_id;
        self.worktrees
            .as_ref()
            .and_then(|w| w.path_for(&id))
            .ok_or_else(|| HostServiceError::new("no-active-worktree", "worktree path unavailable".into()))
    }
```
> `HostServiceError::new(code, message)`、`.code()`、`self.worktrees: Option<Arc<dyn WorktreeContexts>>`、`serde_json::to_value`、`ade_git::history::history` 与 `ade_git::compare::commit_compare` 均沿用本文件/该 crate 既有约定（`history`/`compare` 已在 `ade-bridge/commands/git.rs` 被引用，路径与函数名一致）。`history(...)` 返回的 `GitHistoryResult` serde 已是 camelCase，与 UI 类型对齐。

- [ ] **Step 5: 运行测试通过**

Run: `cargo test -p ade-bridge`（含 `git_log_*` 两条）
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/crates/ade-bridge/src/plugins_service.rs src-tauri/crates/ade-bridge/tests/plugins_service.rs
git commit -m "feat(git-log): ade-bridge 实现 git.log/git.commitFiles（worktree→path 调 ade-git）（增量 2a）"
```

---


## Task 4: 插件 UI 数据层（host-bridge 调用 + HostRepository + 加载/错误态）

**Files:**
- Create: `plugins/git-log/src/ui/host-bridge.ts`
- Create: `plugins/git-log/src/domain/host-repository.ts`
- Create: `plugins/git-log/src/domain/host-repository.test.ts`
- Modify: `plugins/git-log/src/ui/GitLogView.tsx`（loading/error + try/catch）
- Modify: `plugins/git-log/src/ui/tab-main.tsx`（注入 `HostRepository`）

**Interfaces:**
- Consumes: panel 桥协议 `orca-panel-action`/`orca-panel-action-result`（同 `launcher.html`）；`git.log`/`git.commitFiles`；`GitLogRepository`/`GitLogSnapshot`（Task 3 增量 1 已有 `domain/types.ts`）；`GitHistoryResult`/`GitBranchChangeEntry`。
- Produces: `callHost(action: string, params?: unknown): Promise<unknown>`（reject on `{ok:false}`）；`class HostRepository implements GitLogRepository`；`GitLogView` 增 `status: 'loading'|'ready'|'error'` + `error: string | null` 内部态。

- [ ] **Step 1: 写失败测试（mock 桥）**

Create `plugins/git-log/src/domain/host-repository.test.ts`:

```ts
// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from 'vitest'
import { callHost } from '../ui/host-bridge'
import { HostRepository } from './host-repository'

// 让 window.parent.postMessage 触发一条匹配 requestId 的 orca-panel-action-result
function stubHostReply(result: Record<string, unknown>): void {
  // @ts-expect-error 测试替身
  window.parent = { postMessage: (msg: { requestId: string }) => {
    setTimeout(() => window.dispatchEvent(new MessageEvent('message', { data: { type: 'orca-panel-action-result', requestId: msg.requestId, ...result } })), 0)
  } }
}

beforeEach(() => { vi.restoreAllMocks() })

it('callHost 解析 ok 结果', async () => {
  stubHostReply({ ok: true, value: { limit: 200, items: [] } })
  await expect(callHost('git.log', { limit: 200 })).resolves.toEqual({ limit: 200, items: [] })
})

it('callHost 对 !ok 抛错', async () => {
  stubHostReply({ ok: false, errorCode: 'no-active-worktree', error: 'none' })
  await expect(callHost('git.log')).rejects.toThrow(/no-active-worktree|none/)
})

it('HostRepository.loadCommitFiles 透传 oid 并返回数组', async () => {
  stubHostReply({ ok: true, value: [{ path: 'a.ts', status: 'modified' }] })
  const files = await new HostRepository().loadCommitFiles('a'.repeat(40))
  expect(files[0]?.path).toBe('a.ts')
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/domain/host-repository.test.ts`
Expected: FAIL（模块缺失）

- [ ] **Step 3: host-bridge 调用助手**

Create `plugins/git-log/src/ui/host-bridge.ts`:

```ts
let seq = 0

/** 在沙箱 panel/tab 内调宿主方法：postMessage `orca-panel-action` → 等 `orca-panel-action-result`。
 *  `data.ok` 真则 resolve(value)，否则 reject。opaque origin，`'*'` 是唯一可用 targetOrigin。 */
export function callHost(action: string, params?: unknown): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const requestId = `gl-${action}-${Date.now()}-${++seq}`
    const onResult = (event: MessageEvent): void => {
      const data = event.data as { type?: string; requestId?: string; ok?: boolean; value?: unknown; error?: string; errorCode?: string }
      if (!data || data.type !== 'orca-panel-action-result' || data.requestId !== requestId) return
      window.removeEventListener('message', onResult)
      if (data.ok) resolve(data.value)
      else reject(new Error(`${data.errorCode ?? 'host-call-failed'}: ${data.error ?? ''}`))
    }
    window.addEventListener('message', onResult)
    window.parent.postMessage({ type: 'orca-panel-action', requestId, action, params }, '*')
  })
}
```

- [ ] **Step 4: HostRepository**

Create `plugins/git-log/src/domain/host-repository.ts`:

```ts
import type { GitHistoryResult } from '@shared/git-history-types'
import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import { callHost } from '../ui/host-bridge'
import type { GitLogRepository, GitLogSnapshot } from './types'

const SNAPSHOT_LIMIT = 200

export class HostRepository implements GitLogRepository {
  async loadSnapshot(): Promise<GitLogSnapshot> {
    return (await callHost('git.log', { limit: SNAPSHOT_LIMIT })) as GitHistoryResult
  }
  async loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]> {
    return (await callHost('git.commitFiles', { oid })) as GitBranchChangeEntry[]
  }
}
```

- [ ] **Step 5: 运行测试通过**

Run: `pnpm test plugins/git-log/src/domain/host-repository.test.ts`
Expected: PASS（3 用例）

- [ ] **Step 6: GitLogView 加 loading/error（try/catch，收 M-2）**

Modify `plugins/git-log/src/ui/GitLogView.tsx`：增 state 与错误处理。
```tsx
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading')
  const [error, setError] = useState<string | null>(null)
```
`load` 体包 try/catch：
```tsx
  const load = useCallback(async () => {
    setStatus('loading')
    setError(null)
    try {
      const snap = await repository.loadSnapshot()
      const next = buildGraphRows(snap)
      setRows(next)
      // …branch/ahead/behind/refs/setSelected 同现状…
      setStatus('ready')
    } catch (e) {
      setError(e instanceof Error ? e.message : 'failed to load git history')
      setStatus('error')
    }
  }, [repository])
```
`loadCommitFiles` 的 `.then` 补 `.catch` 置 `files=[]`（或行内小 error）。渲染：`status==='loading'` 显示 "Loading…"、`status==='error'` 显示 `error`（沿用现 `<Toolbar/>` + 容器结构，列表区条件渲染）。

- [ ] **Step 7: tab-main 注入 HostRepository**

Modify `plugins/git-log/src/ui/tab-main.tsx`：
```tsx
import { createRoot } from 'react-dom/client'
import './theme.css'
import { GitLogView } from './GitLogView'
import { HostRepository } from '../domain/host-repository'

const el = document.getElementById('git-log-root')
if (el) createRoot(el).render(<GitLogView repository={new HostRepository()} />)
```
> `git-log-view.test.tsx` 仍显式注入 `FixtureRepository`（经 `repository` prop），不受 `tab-main` 改动影响；`FixtureRepository` 保留。

- [ ] **Step 8: 全量 UI 回归 + typecheck**

Run: `pnpm test plugins/git-log && pnpm build:git-log && pnpm typecheck`
Expected: PASS（含既有 git-log-view 冒烟；error/loading 不破坏 fixture 测试）

- [ ] **Step 9: Commit**

```bash
git add plugins/git-log/src
git commit -m "feat(git-log): UI 数据层 HostRepository + host-bridge 调用 + 加载/错误态（增量 2a）"
```

---

## Task 5: manifest git:read + 文档/consent

**Files:**
- Modify: `plugins/git-log/ade-plugin.json`
- Create: `plugins/git-log/src/manifest.test.ts`
- Modify: `plugins/git-log/README.md`、`docs/phase4-git-log-plugin-record.md`

**Interfaces:**
- Consumes: `git:read` capability（Task 1/2）。
- Produces: manifest `capabilities:[{kind:'workspace:open-tab'},{kind:'git:read'}]`、`version: 0.2.0`。

- [ ] **Step 1: 写失败测试**

Create `plugins/git-log/src/manifest.test.ts`:

```ts
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'
import { expect, it } from 'vitest'

const manifest = JSON.parse(
  readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../ade-plugin.json'), 'utf8')
)

it('manifest 声明 git:read（增量 2a）', () => {
  const kinds = (manifest.capabilities as Array<{ kind: string }>).map((c) => c.kind)
  expect(kinds).toContain('git:read')
  expect(kinds).toContain('workspace:open-tab')
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm test plugins/git-log/src/manifest.test.ts`
Expected: FAIL（当前仅 workspace:open-tab）

- [ ] **Step 3: 改 manifest**

`plugins/git-log/ade-plugin.json`：`"version": "0.2.0"`；`capabilities` 改为：
```json
  "capabilities": [{ "kind": "workspace:open-tab" }, { "kind": "git:read" }]
```

- [ ] **Step 4: 文档更新**

- `README.md`：dev 步骤 consent 说明改为 "`workspace:open-tab` + `git:read`"；"增量 2" 段改为 "2a 已落真实 HEAD 数据；2b 扩 `--all`/过滤/分页"。
- `docs/phase4-git-log-plugin-record.md`：新增"增量 2a"小节（host 面新增 `git:read`/`git.log`/`git.commitFiles`；`cargo test -p ade-bridge -p ade-plugins` 结果；consent 需重授权 `git:read`）。

- [ ] **Step 5: 运行测试通过**

Run: `pnpm test plugins/git-log/src/manifest.test.ts`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add plugins/git-log/ade-plugin.json plugins/git-log/src/manifest.test.ts plugins/git-log/README.md docs/phase4-git-log-plugin-record.md
git commit -m "feat(git-log): manifest 声明 git:read + README/record 同步（增量 2a）"
```

---

## Task 6: 集成门禁 + 契约对齐 + 手工验收

**Files:**
- 无新建（跑门禁；如暴露问题就近修）；`docs/phase4-git-log-plugin-record.md` 记 2a 终态。

**Interfaces:**
- Consumes: Task 1–5 全部。
- Produces: 全量门禁证据 + 跨端契约确认 + dev 真仓库手工清单。

- [ ] **Step 1: 全量自动化门禁**

Run: `cargo test --workspace && pnpm typecheck && pnpm build:web && pnpm build:git-log && pnpm test`
Expected: 全绿。若全量 `pnpm test` 再现 3 个与本增量无关的时序测试满并行超时（增量 1 已判 flaky），单跑相关目录复验并在 record 记 flaky；不阻塞。

- [ ] **Step 2: 跨端契约对齐确认**

核：`ade_git::history` 的 `GitHistoryResult`（Rust `#[serde(rename_all="camelCase")]`）字段 == TS `@shared/git-history-types` `GitHistoryResult`（`items/currentRef/remoteRef/baseRef/mergeBase/hasIncomingChanges/hasOutgoingChanges/hasMore/limit`，item 含 `id/parentIds/subject/message/author/authorEmail/timestamp/statistics/references`）；`GitBranchChangeEntry`（`path/status/oldPath?/added?/removed?`）两侧一致。Task 3 的 `git_log_..._camel_case` 断言 + 增量 1 `buildGraphRows(HostRepository 结果)` 消费即契约证据；如缺，补一条 Rust ade-bridge 断言 `value["items"][0].get("parentIds")`（Task 3 已含）。

- [ ] **Step 3: 后端隔离/新增面自检**

Run: `git diff --name-only main...HEAD`
Expected: Rust 侧仅 `ade-plugins/{types.rs,host/*,service.rs}`、`ade-bridge/{plugins_service.rs,tests/plugins_service.rs}`；TS 仅 `plugin-capabilities.ts`/`plugin-host-api.ts`/新测试 + `plugins/git-log/**` + docs。**不得**触碰 `ade-git`（0 改动）、现有 git bridge 命令、Source Control UI。

- [ ] **Step 4: 手工验收（tauri dev + 真仓库）**

- [ ] 开插件：consent 出现 `git:read`（因新 capability 需重新授权）→ 允许。
- [ ] launcher 开 Git Log tab → 图谱显示**真实** HEAD 提交（非 fixture）；分支/ref chips 为真。
- [ ] 选中提交 → 详情显示真实 author/date/message/文件列表（`git.commitFiles`）。
- [ ] 非 git worktree / 无 active worktree → UI 进入 error 态显示错误、不崩、无空 tab 白屏。
- [ ] light/dark 与宿主同屏一致；≥真实提交数虚拟滚动流畅。

- [ ] **Step 5: 收尾提交（如以上有就近修复）**

```bash
git add -A
git commit -m "chore(git-log): 增量 2a 集成门禁通过 + record 终态"
```

---

## Self-Review（规格 → 任务覆盖）

**1. Spec coverage（对照增量 2a 规格逐节）：**
- §2.1 实现面 capability/host 方法/gate/trait/impl/path_for/UI/tab/加载错误态/manifest/类型 → Task 1（TS 面）、Task 2（Rust 枚举+MethodSpec+gate+dispatch+trait+桩）、Task 3（ade-bridge 实现+path_for）、Task 4（HostRepository+host-bridge+loading/error+tab-main）、Task 5（manifest+文档）。✓
- §2.2 不做（不扩 ade-git、无 --all/过滤/分页/Date·Path chips、不引 worker、不改现有 git 命令/SC UI）→ 全程"新增为主、ade-git 0 改"；Task 6 Step 3 白名单强制。✓
- §3 能力面（git:read、limit≤200、oid 7..40、active-worktree、panel、read-only）→ Task 1 zod + Task 2 MethodSpec；limit 上限断言在 Task 1 Step 1。✓
- §4 Rust（dispatch/trait/serde camelCase/两处 FakeServices/integration test/错误码）→ Task 2 + Task 3（`no-active-worktree`/`not-a-git-repo`/`unknown-oid`）。✓
- §5 UI（HostRepository 走 orca-panel-action；tab-main 固定 Host；fixture 仅测试）→ Task 4。✓
- §6 测试/门禁（cargo -p ade-plugins/-p ade-bridge、TS、全量、手工）→ 各 Task + Task 6。✓
- §7 风险（表纯新增、限域读、重 consent、契约漂移、依赖注入）→ Global Constraints + Task 6 Step 2/3 + record。✓

**2. Placeholder 扫描：** 无 "TBD/后续补"。Rust `MethodSpec`/`HandlerError`/测试 helper 处标注"沿用该文件既有写法/helper 名"——是定位指引非占位（条目值/函数签名已给全）。`services_with_git_repo()`/`services_with_no_active_worktree()` 是需在 ade-bridge 测试文件按现有 helper 风格新建的测试夹具，Task 3 Step 1 已说明其内容（临时 repo 建 2 commit + 置 lastActiveWorktreeId）；实现者据此落地。

**3. 类型一致性：** `git_log(limit)/git_commit_files(oid)` 在 trait（Task 2 Step 3）、dispatch（Step 4）、ade-bridge 实现（Task 3 Step 4）、FakeServices（Step 5）签名一致（`Result<Value, HostServiceError>`）。TS 方法名 `git.log`/`git.commitFiles` 在 Task 1 spec、Task 2 dispatch/gate、Task 4 `callHost('git.log'|'git.commitFiles')`、Task 5 manifest `git:read` 全链一致。`GitLogSnapshot=GitHistoryResult` 跨增量 1/2 不变。✓

**4. 范围：** 单一子系统（git-log 真实数据），可独立产出可测软件 → 单计划合适。2b 另立计划。
