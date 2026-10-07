# Phase 2C diff 注释与评审（本地注释全链路）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让本地 diff 注释跨刷新/重启存活，并让「发送未发送注释给运行中本地 agent」链路真正可用。

**Architecture:** 两处最小修复：① Rust `Worktree` 投影回读 `diffComments`（存储层已持久化，只差回读）；② 在 `callRuntimeRpc` 的 local 分支挂一个 TS 本地终端 RPC 适配器，用真实 `pty.*` 原语 + 2B agent 状态实现 `terminal.list/agentStatus/isRunningAgent/wait/send`，渲染层发送栈零改动。

**Tech Stack:** Rust（ade-core/ade-bridge/ade-store、Specta bindings）、TypeScript（Vitest、zustand store、Tauri IPC bridge）。

**Spec:** `docs/superpowers/specs/2026-10-07-phase2c-diff-annotations-review-design.md`（执行者须同时阅读）

## Global Constraints

- 不新增任何 npm / cargo 依赖。
- 所有门禁最终须绿：`cargo test --workspace`、`pnpm typecheck`、`pnpm build:web`、`pnpm test`（`browser-history-match.performance.test.ts` 为已知负载抖动，可单独复跑）。
- bindings 唯一生成方式：`cargo run -p ade-bridge --bin export-bindings`；`bindings_are_fresh` 测试会校验。
- 所有提交信息用中文 conventional 格式，并附 `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- 测试命令统一从仓库根 `/Users/itsuka/CodeSpace/ade` 执行。
- 现有真实桥域清单（`src/bridge/create-api.ts`）：`agentStatus app folderWorkspaces fs git notifications onboarding platform preflight projectGroups projects pty repos session settings ui worktrees`；`runtime` 域仍是 mock——本计划只对 local 的 5 个 terminal 方法做本地实现，其余方法维持 mock 回退。
- 术语：handle = 本地 ptyId；paneKey = `${tabId}:${leafId}`（store `agentStatusByPaneKey` 的键）。

---

### Task 1: Rust — Worktree 投影回读 diffComments

**Files:**
- Modify: `src-tauri/crates/ade-core/src/models/worktree.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/worktrees.rs`（`apply_worktree_meta`，约 70-117 行）
- Test: `src-tauri/crates/ade-bridge/tests/worktree_lifecycle.rs`
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: `WorktreeMetaStore` 白名单已含 `diffComments`（`ade-store/src/worktree_meta_store.rs:52`），`worktrees_update_meta` → `update_meta_impl` 已把 `updates` merge 进存储。
- Produces: `ade_core::models::worktree::Worktree` 新字段 `diff_comments: Option<serde_json::Value>`；`worktrees_list` / `worktrees_update_meta` 的返回行携带 `diffComments`（camelCase）。

- [x] **Step 1: 写失败测试**

在 `src-tauri/crates/ade-bridge/tests/worktree_lifecycle.rs` 末尾追加（模仿同文件 `update_meta_merges_whitelist_and_projection_reflects` 的构造方式；该文件的 import 已含 `json!`、`WorktreeMetaStore`、`FsService`、`TestDir`、`init_git_repo`、`repo_row`、`settings`、`create_args`、`create_worktree_impl`、`update_meta_impl`、`read_worktree`）：

```rust
#[test]
fn update_meta_round_trips_diff_comments() {
    let _env = hermetic_env();
    let dir = TestDir::new("update-meta-diff-comments");
    let repo = init_git_repo(&dir, "my-repo");
    let meta = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let fs = FsService::new();
    let repo_value = repo_row(&repo);
    let worktree = create_worktree_impl(
        &repo_value,
        &settings(&dir),
        &meta,
        &fs,
        &create_args(&repo_value, "fix-auth"),
    )
    .unwrap();

    let comments = json!([{
        "id": "c1",
        "worktreeId": worktree.id,
        "filePath": "src/a.ts",
        "lineNumber": 3,
        "body": "tighten this",
        "createdAt": 1_700_000_000_000u64,
        "updatedAt": 1_700_000_000_000u64,
        "side": "modified"
    }]);

    let updated = update_meta_impl(
        &repo_value,
        &[],
        &meta,
        &fs,
        &worktree.id,
        &json!({ "diffComments": comments }),
    )
    .unwrap();
    assert_eq!(updated.diff_comments, Some(comments.clone()));

    // 新进程视角：重新 load 同一文件后列表仍带回注释。
    let reloaded = WorktreeMetaStore::load(dir.file("worktrees.json"));
    let listed = read_worktree(&repo_value, &reloaded, &fs, &worktree.id);
    assert_eq!(listed["diffComments"], comments);
}
```

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p ade-bridge --test worktree_lifecycle update_meta_round_trips_diff_comments`
Expected: 编译失败——`Worktree` 无 `diff_comments` 字段（E0609）。

- [x] **Step 3: 最小实现**

`src-tauri/crates/ade-core/src/models/worktree.rs`：`Worktree` struct 末尾加字段（结构体已有 `#[serde(rename_all = "camelCase")]`）：

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_comments: Option<serde_json::Value>,
```

两个构造器 `for_git_entry` / `for_folder_workspace` 的 `Self { ... }` 中补 `diff_comments: None,`（放在 `workspace_status` 之后）。

`src-tauri/crates/ade-bridge/src/commands/worktrees.rs` 的 `apply_worktree_meta` 末尾（`workspaceStatus` 分支之后）加：

```rust
    if let Some(diff_comments) = meta.get("diffComments") {
        if diff_comments.is_array() {
            worktree.diff_comments = Some(diff_comments.clone());
        }
    }
```

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p ade-bridge --test worktree_lifecycle update_meta_round_trips_diff_comments`
Expected: PASS。

- [x] **Step 5: 跑该 crate 全量测试**

Run: `cargo test -p ade-core -p ade-bridge`
Expected: 全绿（`worktree.rs` 内 `serializes_camel_case_with_null_links` 因 `skip_serializing_if` 不受影响；若它失败说明序列化行为不符，修实现而不是改该测试）。

- [x] **Step 6: 重生成 bindings 并验证新鲜度**

Run: `cargo run -p ade-bridge --bin export-bindings`
Run: `cargo test -p ade-bridge bindings_are_fresh`
Expected: 生成的 `Worktree` 类型出现 `diffComments`（`Json | null`），`bindings_are_fresh` PASS。

- [x] **Step 7: 提交**

```bash
git add src-tauri/crates/ade-core/src/models/worktree.rs \
  src-tauri/crates/ade-bridge/src/commands/worktrees.rs \
  src-tauri/crates/ade-bridge/tests/worktree_lifecycle.rs \
  src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): Worktree 投影回读 diffComments（注释跨刷新/重启存活）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: 渲染层 — fetch 合并保留 diffComments（回归锁定）

**Files:**
- Test: `src/renderer/src/store/slices/worktrees-fetch-listing-merge.test.ts`

**Interfaces:**
- Consumes: Task 1 后端列表行带 `diffComments`；`fetchWorktrees` → `mergeFetchedWorktrees` → `toVisibleWorktrees`（`worktree-host-ownership.ts:105`）。
- Produces: 一条回归测试，锁定「fetch 返回的注释不会在合并链路上被剥掉」。

- [x] **Step 1: 写测试**

在 `worktrees-fetch-listing-merge.test.ts` 的 `describe('fetchWorktrees', ...)` 内追加（文件已 import `makeWorktree`、`makeDetectedResult`、`createTestStore`、`mockApi`、`AppState`）：

```ts
  it('keeps persisted diff comments from the fetched catalog', async () => {
    const store = createTestStore()
    const worktreeId = 'repo1::/path/wt1'
    const existing = makeWorktree({ id: worktreeId, repoId: 'repo1', path: '/path/wt1' })
    const comments = [
      {
        id: 'c1',
        worktreeId,
        filePath: 'src/a.ts',
        lineNumber: 3,
        body: 'tighten this',
        createdAt: 1_700_000_000_000,
        updatedAt: 1_700_000_000_000,
        side: 'modified' as const
      }
    ]
    const fetched = {
      ...makeWorktree({ id: worktreeId, repoId: 'repo1', path: '/path/wt1' }),
      diffComments: comments
    }
    const detected = makeDetectedResult('repo1', [fetched])
    mockApi.worktrees.listDetected.mockResolvedValueOnce(detected)
    store.setState({
      worktreesByRepo: { repo1: [existing] },
      detectedWorktreesByRepo: { repo1: detected }
    } as Partial<AppState>)

    await store.getState().fetchWorktrees('repo1')

    expect(store.getState().worktreesByRepo.repo1?.[0]?.diffComments).toEqual(comments)
  })
```

- [x] **Step 2: 跑测试**

Run: `pnpm vitest run src/renderer/src/store/slices/worktrees-fetch-listing-merge.test.ts -t "keeps persisted diff comments"`
Expected: PASS（合并链路是整行替换，不剥字段）。若 FAIL：说明 `toVisibleWorktree`（`worktrees/listing/worktree-catalog-visibility.ts`）或 `withRepoHostOwnership` 构造了新对象丢了字段——在丢字段的那一步补 `...worktree` 展开，然后重跑至 PASS。

- [x] **Step 3: 跑该文件全量并提交**

Run: `pnpm vitest run src/renderer/src/store/slices/worktrees-fetch-listing-merge.test.ts`

```bash
git add src/renderer/src/store/slices/worktrees-fetch-listing-merge.test.ts
git commit -m "test(renderer): 锁定 fetch 合并保留 diffComments

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: local-terminal-rpc 骨架 + 路由接缝 + terminal.list

**Files:**
- Create: `src/renderer/src/runtime/local-terminal-rpc.ts`
- Create: `src/renderer/src/runtime/local-terminal-rpc.test.ts`
- Modify: `src/renderer/src/runtime/runtime-rpc-client.ts`
- Test: `src/renderer/src/runtime/runtime-rpc-client.test.ts`

**Interfaces:**
- Consumes: `window.api.pty.listSessions({connectionId: null})`（真实命令，行含 `id/cwd/title/worktreeId?/agentOwnership`）；store：`ptyIdsByTabId`、`terminalLayoutsByTabId[tabId].ptyIdsByLeafId`、`tabsByWorktree`、`worktreesByRepo`（经 `getIndexedWorktreeById`）；`resolveRuntimePaneTitleForLeaf`（`@/lib/runtime-pane-title-leaf-id`）。
- Produces（后续任务依赖）：
  - `LOCAL_TERMINAL_RPC_METHODS: readonly string[]`（5 个方法名）
  - `isLocalTerminalRpcMethod(method: string): boolean`
  - `callLocalTerminalRpc<TResult>(method: string, params: unknown): Promise<TResult>`
  - `localTerminalFailure(code: string, message: string): RuntimeRpcCallError`
  - 内部类型 `LocalTerminalLocation = { ptyId: string; tabId: string; leafId: string; worktreeId: string }`
  - 内部函数（本任务定义，后续任务复用）：`readTerminalHandle`、`findLocalTerminalLocation(state, ptyId)`、`collectLocalTerminalLocations(state)`、`isPtyLive(ptyId)`、`readPaneTitle(state, loc)`、`findWorktreeIdForTab(state, tabId)`、`findLeafIdForPty(layout, ptyId)`

- [x] **Step 1: 写失败测试**

创建 `src/renderer/src/runtime/local-terminal-rpc.test.ts`：

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppState } from '@/store/types'
import { callLocalTerminalRpc, isLocalTerminalRpcMethod } from './local-terminal-rpc'

const LEAF_ID = '11111111-1111-4111-8111-111111111111'

const testState = vi.hoisted(() => ({
  appState: null as unknown as Partial<AppState>
}))

vi.mock('@/store', () => ({
  useAppStore: Object.assign(
    (selector: (state: Partial<AppState>) => unknown) => selector(testState.appState),
    { getState: () => testState.appState }
  )
}))

const listSessions = vi.fn()
const writeAccepted = vi.fn()

function makeState(overrides: Partial<AppState> = {}): Partial<AppState> {
  return {
    tabsByWorktree: { 'wt-1': [{ id: 'tab-1' }] },
    ptyIdsByTabId: { 'tab-1': ['pty-1'] },
    terminalLayoutsByTabId: {
      'tab-1': { activeLeafId: LEAF_ID, ptyIdsByLeafId: { [LEAF_ID]: 'pty-1' } }
    } as AppState['terminalLayoutsByTabId'],
    runtimePaneTitlesByTabId: {},
    worktreesByRepo: {},
    ...overrides
  } as Partial<AppState>
}

beforeEach(() => {
  testState.appState = makeState()
  listSessions.mockReset()
  writeAccepted.mockReset()
  listSessions.mockResolvedValue([
    { id: 'pty-1', cwd: '/tmp/wt', title: '', worktreeId: 'wt-1', agentOwnership: 'present' }
  ])
  writeAccepted.mockResolvedValue(true)
  vi.stubGlobal('window', { api: { pty: { listSessions, writeAccepted } } })
})

describe('local terminal RPC adapter', () => {
  it('recognizes exactly the five local terminal methods', () => {
    for (const method of [
      'terminal.list',
      'terminal.agentStatus',
      'terminal.isRunningAgent',
      'terminal.wait',
      'terminal.send'
    ]) {
      expect(isLocalTerminalRpcMethod(method)).toBe(true)
    }
    expect(isLocalTerminalRpcMethod('terminal.create')).toBe(false)
    expect(isLocalTerminalRpcMethod('repo.list')).toBe(false)
  })

  it('lists live local terminals mapped to tab/leaf/worktree with ptyId as handle', async () => {
    const result = await callLocalTerminalRpc('terminal.list', {
      worktree: 'id:wt-1',
      limit: 200,
      includeVisualLayouts: false
    })
    expect(result).toEqual({
      terminals: [
        {
          handle: 'pty-1',
          ptyId: 'pty-1',
          worktreeId: 'wt-1',
          worktreePath: '',
          branch: '',
          tabId: 'tab-1',
          leafId: LEAF_ID,
          title: null,
          connected: true,
          writable: true,
          lastOutputAt: null,
          preview: ''
        }
      ],
      totalCount: 1,
      truncated: false
    })
    expect(listSessions).toHaveBeenCalledWith({ connectionId: null })
  })

  it('excludes terminals whose pty is no longer live', async () => {
    listSessions.mockResolvedValue([])
    const result = await callLocalTerminalRpc<{ terminals: unknown[] }>('terminal.list', {})
    expect(result.terminals).toEqual([])
  })

  it('filters by the runtime worktree selector', async () => {
    const result = await callLocalTerminalRpc<{ terminals: unknown[] }>('terminal.list', {
      worktree: 'id:wt-other'
    })
    expect(result.terminals).toEqual([])
  })

  it('rejects unknown methods with method_not_found', async () => {
    await expect(callLocalTerminalRpc('terminal.unknown', {})).rejects.toMatchObject({
      name: 'RuntimeRpcCallError',
      code: 'method_not_found'
    })
  })
})
```

- [x] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: FAIL —— 模块不存在。

- [x] **Step 3: 实现骨架 + terminal.list**

创建 `src/renderer/src/runtime/local-terminal-rpc.ts`：

```ts
import type {
  RuntimeTerminalListResult,
  RuntimeTerminalSummary
} from '../../../shared/runtime-types'
import type { AppState } from '@/store/types'
import { useAppStore } from '@/store'
import { getIndexedWorktreeById } from '@/store/worktree-repo-index'
import { resolveRuntimePaneTitleForLeaf } from '@/lib/runtime-pane-title-leaf-id'
import { RuntimeRpcCallError } from './runtime-rpc-result'

const LOCAL_TERMINAL_RPC_METHODS = [
  'terminal.list',
  'terminal.agentStatus',
  'terminal.isRunningAgent',
  'terminal.wait',
  'terminal.send'
] as const

const TERMINAL_LIST_DEFAULT_LIMIT = 200

export type LocalTerminalLocation = {
  ptyId: string
  tabId: string
  leafId: string
  worktreeId: string
}

export function isLocalTerminalRpcMethod(method: string): boolean {
  return (LOCAL_TERMINAL_RPC_METHODS as readonly string[]).includes(method)
}

export function localTerminalFailure(code: string, message: string): RuntimeRpcCallError {
  return new RuntimeRpcCallError({ id: 'local', ok: false, error: { code, message } })
}

export async function callLocalTerminalRpc<TResult>(
  method: string,
  params: unknown
): Promise<TResult> {
  switch (method) {
    case 'terminal.list':
      return (await listLocalTerminals(params)) as TResult
    default:
      throw localTerminalFailure(
        'method_not_found',
        `Unsupported local terminal method: ${method}`
      )
  }
}

export function readTerminalHandle(params: unknown): string {
  const handle = (params as { terminal?: unknown } | null | undefined)?.terminal
  if (typeof handle !== 'string' || handle.length === 0) {
    throw localTerminalFailure('terminal_handle_stale', 'A terminal handle is required.')
  }
  return handle
}

export function findWorktreeIdForTab(state: AppState, tabId: string): string | null {
  for (const [worktreeId, tabs] of Object.entries(state.tabsByWorktree ?? {})) {
    if (tabs?.some((tab) => tab.id === tabId)) {
      return worktreeId
    }
  }
  return null
}

export function findLeafIdForPty(
  layout: AppState['terminalLayoutsByTabId'][string] | undefined,
  ptyId: string
): string {
  const byLeaf = layout?.ptyIdsByLeafId
  if (byLeaf) {
    for (const [leafId, candidate] of Object.entries(byLeaf)) {
      if (candidate === ptyId) {
        return leafId
      }
    }
  }
  return layout?.activeLeafId ?? ''
}

export function collectLocalTerminalLocations(state: AppState): LocalTerminalLocation[] {
  const locations: LocalTerminalLocation[] = []
  for (const [tabId, ptyIds] of Object.entries(state.ptyIdsByTabId ?? {})) {
    const worktreeId = findWorktreeIdForTab(state, tabId)
    if (!worktreeId) {
      continue
    }
    const layout = state.terminalLayoutsByTabId?.[tabId]
    for (const ptyId of ptyIds ?? []) {
      locations.push({ ptyId, tabId, leafId: findLeafIdForPty(layout, ptyId), worktreeId })
    }
  }
  return locations
}

export function findLocalTerminalLocation(
  state: AppState,
  ptyId: string
): LocalTerminalLocation | null {
  return collectLocalTerminalLocations(state).find((location) => location.ptyId === ptyId) ?? null
}

export async function isPtyLive(ptyId: string): Promise<boolean> {
  const sessions = await window.api.pty.listSessions({ connectionId: null })
  return sessions.some((session) => session.id === ptyId)
}

export function readPaneTitle(state: AppState, location: LocalTerminalLocation): string | null {
  const resolved = resolveRuntimePaneTitleForLeaf(
    state.terminalLayoutsByTabId?.[location.tabId],
    state.runtimePaneTitlesByTabId?.[location.tabId],
    location.leafId
  )
  if (resolved) {
    return resolved
  }
  const tabs = state.tabsByWorktree?.[location.worktreeId]
  const tab = tabs?.find((entry) => entry.id === location.tabId)
  return tab?.title ?? null
}

function readWorktreeSelectorFilter(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }
  const trimmed = value.trim()
  if (!trimmed) {
    return null
  }
  return trimmed.startsWith('id:') ? trimmed.slice(3) : trimmed
}

function readListLimit(value: unknown): number {
  return typeof value === 'number' && Number.isInteger(value) && value > 0
    ? value
    : TERMINAL_LIST_DEFAULT_LIMIT
}

async function listLocalTerminals(params: unknown): Promise<RuntimeTerminalListResult> {
  const args = (params ?? {}) as { worktree?: unknown; limit?: unknown }
  const state = useAppStore.getState()
  const worktreeFilter = readWorktreeSelectorFilter(args.worktree)
  const limit = readListLimit(args.limit)
  const sessions = await window.api.pty.listSessions({ connectionId: null })
  const liveIds = new Set(sessions.map((session) => session.id))
  const terminals: RuntimeTerminalSummary[] = []
  for (const location of collectLocalTerminalLocations(state)) {
    if (worktreeFilter !== null && location.worktreeId !== worktreeFilter) {
      continue
    }
    if (!liveIds.has(location.ptyId)) {
      continue
    }
    const worktree = getIndexedWorktreeById(state.worktreesByRepo ?? {}, location.worktreeId)
    terminals.push({
      handle: location.ptyId,
      ptyId: location.ptyId,
      worktreeId: location.worktreeId,
      worktreePath: worktree?.path ?? '',
      branch: worktree?.branch ?? '',
      tabId: location.tabId,
      leafId: location.leafId,
      title: readPaneTitle(state, location),
      connected: true,
      writable: true,
      lastOutputAt: null,
      preview: ''
    })
  }
  return {
    terminals: terminals.slice(0, limit),
    totalCount: terminals.length,
    truncated: terminals.length > limit
  }
}
```

在 `runtime-rpc-client.ts` 中接入接缝：

```ts
// import 区新增
import { callLocalTerminalRpc, isLocalTerminalRpcMethod } from './local-terminal-rpc'
```

`callRuntimeRpc` 内、`const response = ...` 之前插入：

```ts
  if (target.kind === 'local' && isLocalTerminalRpcMethod(method)) {
    return await callLocalTerminalRpc<TResult>(method, nextParams)
  }
```

- [x] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: PASS（5 条）。

- [x] **Step 5: 路由接缝测试**

在 `runtime-rpc-client.test.ts` 顶部 mock 适配器（避免测试触碰真实 store）：

```ts
const localTerminalRpc = vi.hoisted(() => ({
  callLocalTerminalRpc: vi.fn(),
  isLocalTerminalRpcMethod: (method: string) => method.startsWith('terminal.')
}))

vi.mock('./local-terminal-rpc', () => localTerminalRpc)
```

在 `describe('runtime RPC client routing', ...)` 内追加两条测试：

```ts
  it('routes local terminal methods to the local adapter without touching window.api.runtime.call', async () => {
    localTerminalRpc.callLocalTerminalRpc.mockResolvedValue({ terminals: [] })
    await expect(
      callRuntimeRpc({ kind: 'local' }, 'terminal.list', { limit: 5 })
    ).resolves.toEqual({ terminals: [] })
    expect(localTerminalRpc.callLocalTerminalRpc).toHaveBeenCalledWith('terminal.list', {
      limit: 5
    })
    expect(runtimeCall).not.toHaveBeenCalled()
  })

  it('keeps non-terminal local methods on window.api.runtime.call', async () => {
    runtimeCall.mockResolvedValue({
      id: 'local',
      ok: true,
      result: [],
      _meta: { runtimeId: 'local-runtime' }
    })
    await callRuntimeRpc({ kind: 'local' }, 'repo.list')
    expect(runtimeCall).toHaveBeenCalledWith({ method: 'repo.list', params: undefined })
    expect(localTerminalRpc.callLocalTerminalRpc).not.toHaveBeenCalled()
  })
```

`beforeEach` 里补 `localTerminalRpc.callLocalTerminalRpc.mockReset()`。

Run: `pnpm vitest run src/renderer/src/runtime/runtime-rpc-client.test.ts`
Expected: PASS（既有「routes local runtime calls through window.api.runtime.call」用 `repo.list`，不受影响）。

- [x] **Step 6: 提交**

```bash
git add src/renderer/src/runtime/local-terminal-rpc.ts \
  src/renderer/src/runtime/local-terminal-rpc.test.ts \
  src/renderer/src/runtime/runtime-rpc-client.ts \
  src/renderer/src/runtime/runtime-rpc-client.test.ts
git commit -m "feat(renderer): 本地 terminal RPC 适配器骨架与 terminal.list

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: terminal.agentStatus / terminal.isRunningAgent（hook 映射 + 标题证据）

**Files:**
- Modify: `src/renderer/src/runtime/local-terminal-rpc.ts`
- Test: `src/renderer/src/runtime/local-terminal-rpc.test.ts`

**Interfaces:**
- Consumes: Task 3 的 `findLocalTerminalLocation`、`readTerminalHandle`、`readPaneTitle`；store `agentStatusByPaneKey`；`isExplicitAgentStatusFresh` / `classifyTitleActivity` / `resolveTitleActivityLabel`（`@/lib/pane-agent-evidence`）、`AGENT_STATUS_STALE_AFTER_MS`（`shared/agent-status-types`）；测试工厂 `makeAgentStatusEntry`（`@/runtime/sync-runtime-graph-test-harness`）。
- Produces:
  - `mapAgentStatusState(state: AgentStatusState): RuntimeTerminalAgentStatusState`（导出供测试）
  - `readLocalAgentStatus(state: AppState, location: LocalTerminalLocation): RuntimeTerminalAgentStatusState`
  - `hasAgentTitleEvidence(state: AppState, location: LocalTerminalLocation): boolean`
  - `hasIdleTitleEvidence(state: AppState, location: LocalTerminalLocation): boolean`
  - `callLocalTerminalRpc` 支持 `terminal.agentStatus`、`terminal.isRunningAgent`

- [x] **Step 1: 写失败测试**

在 `local-terminal-rpc.test.ts` 追加（import `makeAgentStatusEntry`）：

```ts
import { makeAgentStatusEntry } from './sync-runtime-graph-test-harness'

// 追加到 describe 内
  it('maps fresh hook states to the runtime contract', async () => {
    testState.appState = makeState({
      agentStatusByPaneKey: {
        [`tab-1:${LEAF_ID}`]: makeAgentStatusEntry({
          state: 'blocked',
          updatedAt: Date.now()
        })
      }
    } as Partial<AppState>)
    await expect(
      callLocalTerminalRpc('terminal.agentStatus', { terminal: 'pty-1' })
    ).resolves.toEqual({
      agentStatus: { handle: 'pty-1', isRunningAgent: true, status: 'permission' }
    })
  })

  it('falls back to agent title evidence when no fresh hook entry exists', async () => {
    testState.appState = makeState({
      runtimePaneTitlesByTabId: { 'tab-1': { 0: '✳ Claude Code' } }
    } as Partial<AppState>)
    await expect(
      callLocalTerminalRpc('terminal.agentStatus', { terminal: 'pty-1' })
    ).resolves.toEqual({
      agentStatus: { handle: 'pty-1', isRunningAgent: true, status: 'idle' }
    })
  })

  it('reports no agent when neither hook nor title evidence exists', async () => {
    await expect(
      callLocalTerminalRpc('terminal.isRunningAgent', { terminal: 'pty-1' })
    ).resolves.toEqual({ isRunningAgent: false })
  })

  it('rejects stale handles', async () => {
    await expect(
      callLocalTerminalRpc('terminal.agentStatus', { terminal: 'pty-gone' })
    ).rejects.toMatchObject({ name: 'RuntimeRpcCallError', code: 'terminal_handle_stale' })
  })
```

注：标题证据用 `classifyTitleActivity` 的真实解析（`'✳ Claude Code'` 的 `classify` 结果在 `pane-agent-evidence` 里定义为 idle/working 之一）；实现后若该字符串不被识别，改用 `pane-agent-evidence.test.ts` 中已断言可识别的标题样例（同文件内可查到）。`runtimePaneTitlesByTabId` 的键为数字索引，`resolveRuntimePaneTitleForLeaf` 按 leaf 在布局中的顺序取值——本测试布局只有一个叶子，键 `0` 即该叶子。

- [x] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: FAIL —— `terminal.agentStatus` 落入 default 抛 `method_not_found`。

- [x] **Step 3: 实现**

`local-terminal-rpc.ts` 顶部补 import：

```ts
import type {
  RuntimeTerminalAgentStatus,
  RuntimeTerminalAgentStatusState,
  RuntimeTerminalListResult,
  RuntimeTerminalSummary
} from '../../../shared/runtime-types'
import type { AgentStatusState } from '../../../shared/agent-status-types'
import { AGENT_STATUS_STALE_AFTER_MS } from '../../../shared/agent-status-types'
import {
  classifyTitleActivity,
  isExplicitAgentStatusFresh,
  resolveTitleActivityLabel
} from '@/lib/pane-agent-evidence'
```

新增函数：

```ts
export function mapAgentStatusState(state: AgentStatusState): RuntimeTerminalAgentStatusState {
  switch (state) {
    case 'working':
      return 'working'
    case 'blocked':
      return 'permission'
    case 'waiting':
    case 'done':
      return 'idle'
  }
}

export function readLocalAgentStatus(
  state: AppState,
  location: LocalTerminalLocation
): RuntimeTerminalAgentStatusState {
  const entry = state.agentStatusByPaneKey?.[`${location.tabId}:${location.leafId}`]
  if (entry && isExplicitAgentStatusFresh(entry, Date.now(), AGENT_STATUS_STALE_AFTER_MS)) {
    return mapAgentStatusState(entry.state)
  }
  return null
}

export function hasAgentTitleEvidence(state: AppState, location: LocalTerminalLocation): boolean {
  const title = readPaneTitle(state, location)
  return title !== null && classifyTitleActivity(title) !== null && resolveTitleActivityLabel(title) !== null
}

export function hasIdleTitleEvidence(state: AppState, location: LocalTerminalLocation): boolean {
  const title = readPaneTitle(state, location)
  return title !== null && classifyTitleActivity(title) === 'idle'
}

async function getLocalAgentStatus(params: unknown): Promise<{ agentStatus: RuntimeTerminalAgentStatus }> {
  const terminal = readTerminalHandle(params)
  const state = useAppStore.getState()
  const location = findLocalTerminalLocation(state, terminal)
  if (!location) {
    throw localTerminalFailure('terminal_handle_stale', `Unknown terminal: ${terminal}`)
  }
  const hookStatus = readLocalAgentStatus(state, location)
  if (hookStatus !== null) {
    return { agentStatus: { handle: terminal, isRunningAgent: true, status: hookStatus } }
  }
  const titleEvidence = hasAgentTitleEvidence(state, location)
  return {
    agentStatus: { handle: terminal, isRunningAgent: titleEvidence, status: titleEvidence ? 'idle' : null }
  }
}
```

`callLocalTerminalRpc` switch 补：

```ts
    case 'terminal.agentStatus':
      return (await getLocalAgentStatus(params)) as TResult
    case 'terminal.isRunningAgent': {
      const { agentStatus } = await getLocalAgentStatus(params)
      return { isRunningAgent: agentStatus.isRunningAgent } as TResult
    }
```

- [x] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: PASS。若标题样例不被识别，替换为 `pane-agent-evidence.test.ts` 中已覆盖的 idle 标题字符串后重跑。

- [x] **Step 5: 提交**

```bash
git add src/renderer/src/runtime/local-terminal-rpc.ts \
  src/renderer/src/runtime/local-terminal-rpc.test.ts
git commit -m "feat(renderer): terminal.agentStatus/isRunningAgent 本地映射

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: terminal.wait（tui-idle：状态轮询 + 输出静默窗）

**Files:**
- Modify: `src/renderer/src/runtime/local-terminal-rpc.ts`
- Test: `src/renderer/src/runtime/local-terminal-rpc.test.ts`

**Interfaces:**
- Consumes: Task 4 的 `readLocalAgentStatus`、`hasIdleTitleEvidence`、`hasAgentTitleEvidence`；`subscribeToPtyData`（`@/components/terminal-pane/pty-data-sidecar-subscriptions`）；`isPtyLive`。
- Produces: `callLocalTerminalRpc` 支持 `terminal.wait`，返回 `{ wait: RuntimeTerminalWait }`：
  - `condition: 'tui-idle'`：`satisfied:true`（状态 idle，或 hook 缺失时标题 idle/1500ms 输出静默）；`blockedReason:'agent-approval-prompt'`（状态 permission）；`satisfied:false`（working 到超时）
  - `condition: 'exit'`：pty 消失时 `{satisfied:true, status:'exited'}`；超时 `{satisfied:false, status:'running'}`
  - pty 消失：`{satisfied:false, status:'exited'}`（tui-idle）

- [x] **Step 1: 写失败测试**

在 `local-terminal-rpc.test.ts` 追加（顶部 `vi.useRealTimers()` 默认；wait 轮询间隔实现为常量，测试用 `timeoutMs: 50` 加速超时路径）：

```ts
import { makeAgentStatusEntry } from './sync-runtime-graph-test-harness'

  it('waits until the agent is idle', async () => {
    testState.appState = makeState({
      agentStatusByPaneKey: {
        [`tab-1:${LEAF_ID}`]: makeAgentStatusEntry({ state: 'waiting', updatedAt: Date.now() })
      }
    } as Partial<AppState>)
    await expect(
      callLocalTerminalRpc('terminal.wait', { terminal: 'pty-1', for: 'tui-idle', timeoutMs: 500 })
    ).resolves.toEqual({
      wait: {
        handle: 'pty-1',
        condition: 'tui-idle',
        satisfied: true,
        status: 'running',
        exitCode: null
      }
    })
  })

  it('reports a permission prompt as a blocked wait', async () => {
    testState.appState = makeState({
      agentStatusByPaneKey: {
        [`tab-1:${LEAF_ID}`]: makeAgentStatusEntry({ state: 'blocked', updatedAt: Date.now() })
      }
    } as Partial<AppState>)
    await expect(
      callLocalTerminalRpc('terminal.wait', { terminal: 'pty-1', for: 'tui-idle', timeoutMs: 500 })
    ).resolves.toMatchObject({
      wait: { satisfied: false, blockedReason: 'agent-approval-prompt', status: 'running' }
    })
  })

  it('times out while the agent keeps working', async () => {
    testState.appState = makeState({
      agentStatusByPaneKey: {
        [`tab-1:${LEAF_ID}`]: makeAgentStatusEntry({ state: 'working', updatedAt: Date.now() })
      }
    } as Partial<AppState>)
    await expect(
      callLocalTerminalRpc('terminal.wait', { terminal: 'pty-1', for: 'tui-idle', timeoutMs: 30 })
    ).resolves.toMatchObject({ wait: { satisfied: false, status: 'running' } })
  })

  it('reports an exited terminal', async () => {
    listSessions.mockResolvedValue([])
    await expect(
      callLocalTerminalRpc('terminal.wait', { terminal: 'pty-1', for: 'tui-idle', timeoutMs: 500 })
    ).resolves.toMatchObject({ wait: { satisfied: false, status: 'exited' } })
  })
```

（`subscribeToPtyData` 需要 pty dispatcher；在测试的 `window` stub 里补 `onData: vi.fn(() => () => {})`，与 `pty` 其余所需方法。若 dispatcher 还要求 `onExit/onSpawned`，一并补 no-op。）

- [x] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: FAIL —— `terminal.wait` 走 default。

- [x] **Step 3: 实现**

`local-terminal-rpc.ts` 补 import：

```ts
import type {
  RuntimeTerminalWait,
  RuntimeTerminalWaitCondition
} from '../../../shared/runtime-types'
import { subscribeToPtyData } from '@/components/terminal-pane/pty-data-sidecar-subscriptions'
```

新增：

```ts
const WAIT_POLL_MS = 250
const WAIT_OUTPUT_QUIET_MS = 1500
const WAIT_TIMEOUT_DEFAULT_MS = 15_000
const WAIT_TIMEOUT_MAX_MS = 60_000

function readWaitTimeout(value: unknown): number {
  const timeout = typeof value === 'number' && Number.isFinite(value) ? value : WAIT_TIMEOUT_DEFAULT_MS
  return Math.min(Math.max(timeout, WAIT_POLL_MS), WAIT_TIMEOUT_MAX_MS)
}

function makeWaitResult(
  handle: string,
  condition: RuntimeTerminalWaitCondition,
  fields: Pick<RuntimeTerminalWait, 'satisfied' | 'status'> &
    Partial<Pick<RuntimeTerminalWait, 'blockedReason'>>
): { wait: RuntimeTerminalWait } {
  return {
    wait: { handle, condition, exitCode: null, ...fields }
  }
}

async function waitLocalTerminal(params: unknown): Promise<{ wait: RuntimeTerminalWait }> {
  const terminal = readTerminalHandle(params)
  const args = (params ?? {}) as { for?: unknown; timeoutMs?: unknown }
  const condition: RuntimeTerminalWaitCondition = args.for === 'exit' ? 'exit' : 'tui-idle'
  const timeoutMs = readWaitTimeout(args.timeoutMs)
  const deadline = Date.now() + timeoutMs
  const state = useAppStore.getState()
  const location = findLocalTerminalLocation(state, terminal)
  if (!location) {
    throw localTerminalFailure('terminal_handle_stale', `Unknown terminal: ${terminal}`)
  }
  let lastOutputAt = Date.now()
  const unsubscribe =
    condition === 'tui-idle' ? subscribeToPtyData(terminal, () => (lastOutputAt = Date.now())) : () => {}
  try {
    for (;;) {
      if (!(await isPtyLive(terminal))) {
        return makeWaitResult(terminal, condition, {
          satisfied: condition === 'exit',
          status: 'exited'
        })
      }
      if (condition === 'tui-idle') {
        const liveState = useAppStore.getState()
        const status = readLocalAgentStatus(liveState, location)
        if (status === 'permission') {
          return makeWaitResult(terminal, condition, {
            satisfied: false,
            status: 'running',
            blockedReason: 'agent-approval-prompt'
          })
        }
        if (status === 'idle') {
          return makeWaitResult(terminal, condition, { satisfied: true, status: 'running' })
        }
        if (
          status === null &&
          (hasIdleTitleEvidence(liveState, location) ||
            Date.now() - lastOutputAt >= WAIT_OUTPUT_QUIET_MS)
        ) {
          return makeWaitResult(terminal, condition, { satisfied: true, status: 'running' })
        }
      }
      if (Date.now() >= deadline) {
        return makeWaitResult(terminal, condition, { satisfied: false, status: 'running' })
      }
      await new Promise<void>((resolve) => window.setTimeout(resolve, WAIT_POLL_MS))
    }
  } finally {
    unsubscribe()
  }
}
```

`callLocalTerminalRpc` switch 补 `case 'terminal.wait': return (await waitLocalTerminal(params)) as TResult`。

- [x] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: PASS（注意超时用例耗时约 30-280ms，属预期）。

- [x] **Step 5: 提交**

```bash
git add src/renderer/src/runtime/local-terminal-rpc.ts \
  src/renderer/src/runtime/local-terminal-rpc.test.ts
git commit -m "feat(renderer): terminal.wait 本地 tui-idle（状态轮询 + 输出静默窗）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: terminal.send（直写 + requireAgentStatus 拒绝语义）

**Files:**
- Modify: `src/renderer/src/runtime/local-terminal-rpc.ts`
- Test: `src/renderer/src/runtime/local-terminal-rpc.test.ts`

**Interfaces:**
- Consumes: Task 3 的 `findLocalTerminalLocation`、`isPtyLive`；Task 4 的 `readLocalAgentStatus`、`hasAgentTitleEvidence`；`window.api.pty.writeAccepted(id, data)`。
- Produces: `callLocalTerminalRpc` 支持 `terminal.send`，返回 `{ send: RuntimeTerminalSend }`：
  - `requireAgentStatus:'sendable'` 时：无 agent → `{accepted:false, refusedReason:'no-agent'}`；permission → `refusedReason:'permission'`
  - handle 未知 → `terminal_handle_stale`；pty 不在 → `terminal_exited`
  - 写入失败（`writeAccepted` false）→ `{accepted:false, bytesWritten:<已写>}`

- [x] **Step 1: 写失败测试**

在 `local-terminal-rpc.test.ts` 追加：

```ts
  it('writes text and Enter through pty.writeAccepted', async () => {
    testState.appState = makeState({
      agentStatusByPaneKey: {
        [`tab-1:${LEAF_ID}`]: makeAgentStatusEntry({ state: 'waiting', updatedAt: Date.now() })
      }
    } as Partial<AppState>)
    await expect(
      callLocalTerminalRpc('terminal.send', {
        terminal: 'pty-1',
        text: 'fix the bug',
        requireAgentStatus: 'sendable'
      })
    ).resolves.toEqual({
      send: { handle: 'pty-1', accepted: true, bytesWritten: 11 }
    })
    expect(writeAccepted).toHaveBeenCalledWith('pty-1', 'fix the bug')

    await expect(
      callLocalTerminalRpc('terminal.send', {
        terminal: 'pty-1',
        enter: true,
        requireAgentStatus: 'sendable'
      })
    ).resolves.toEqual({
      send: { handle: 'pty-1', accepted: true, bytesWritten: 1 }
    })
    expect(writeAccepted).toHaveBeenLastCalledWith('pty-1', '\r')
  })

  it('refuses to send while the agent is blocked on a permission prompt', async () => {
    testState.appState = makeState({
      agentStatusByPaneKey: {
        [`tab-1:${LEAF_ID}`]: makeAgentStatusEntry({ state: 'blocked', updatedAt: Date.now() })
      }
    } as Partial<AppState>)
    await expect(
      callLocalTerminalRpc('terminal.send', {
        terminal: 'pty-1',
        text: 'hello',
        requireAgentStatus: 'sendable'
      })
    ).resolves.toEqual({
      send: { handle: 'pty-1', accepted: false, bytesWritten: 0, refusedReason: 'permission' }
    })
    expect(writeAccepted).not.toHaveBeenCalled()
  })

  it('refuses when no agent owns the terminal', async () => {
    await expect(
      callLocalTerminalRpc('terminal.send', {
        terminal: 'pty-1',
        text: 'hello',
        requireAgentStatus: 'sendable'
      })
    ).resolves.toEqual({
      send: { handle: 'pty-1', accepted: false, bytesWritten: 0, refusedReason: 'no-agent' }
    })
  })

  it('reports an exited terminal as terminal_exited', async () => {
    listSessions.mockResolvedValue([])
    await expect(
      callLocalTerminalRpc('terminal.send', { terminal: 'pty-1', text: 'hello' })
    ).rejects.toMatchObject({ name: 'RuntimeRpcCallError', code: 'terminal_exited' })
  })
```

- [x] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: FAIL —— `terminal.send` 走 default。

- [x] **Step 3: 实现**

`local-terminal-rpc.ts` 补 import：

```ts
import type { RuntimeTerminalSend } from '../../../shared/runtime-types'
```

新增：

```ts
function readSendRefusal(
  state: AppState,
  location: LocalTerminalLocation
): 'no-agent' | 'permission' | null {
  const status = readLocalAgentStatus(state, location)
  if (status === 'permission') {
    return 'permission'
  }
  if (status !== null) {
    return null
  }
  return hasAgentTitleEvidence(state, location) ? null : 'no-agent'
}

async function sendLocalTerminal(params: unknown): Promise<{ send: RuntimeTerminalSend }> {
  const terminal = readTerminalHandle(params)
  const args = (params ?? {}) as {
    text?: unknown
    enter?: unknown
    requireAgentStatus?: unknown
  }
  const state = useAppStore.getState()
  const location = findLocalTerminalLocation(state, terminal)
  if (!location) {
    throw localTerminalFailure('terminal_handle_stale', `Unknown terminal: ${terminal}`)
  }
  if (!(await isPtyLive(terminal))) {
    throw localTerminalFailure('terminal_exited', `Terminal is not running: ${terminal}`)
  }
  if (args.requireAgentStatus === 'sendable') {
    const refusal = readSendRefusal(state, location)
    if (refusal !== null) {
      return { send: { handle: terminal, accepted: false, bytesWritten: 0, refusedReason: refusal } }
    }
  }
  const text = typeof args.text === 'string' ? args.text : ''
  let bytesWritten = 0
  if (text.length > 0) {
    const accepted = await window.api.pty.writeAccepted(terminal, text)
    if (!accepted) {
      return { send: { handle: terminal, accepted: false, bytesWritten: 0 } }
    }
    bytesWritten += text.length
  }
  if (args.enter === true) {
    const accepted = await window.api.pty.writeAccepted(terminal, '\r')
    if (!accepted) {
      return { send: { handle: terminal, accepted: false, bytesWritten } }
    }
    bytesWritten += 1
  }
  return { send: { handle: terminal, accepted: true, bytesWritten } }
}
```

`callLocalTerminalRpc` switch 补 `case 'terminal.send': return (await sendLocalTerminal(params)) as TResult`。

- [x] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-rpc.test.ts`
Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add src/renderer/src/runtime/local-terminal-rpc.ts \
  src/renderer/src/runtime/local-terminal-rpc.test.ts
git commit -m "feat(renderer): terminal.send 本地直写（requireAgentStatus 拒绝语义）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: 发送栈 local 端到端集成测试（真实适配器 + mock pty）

**Files:**
- Create: `src/renderer/src/runtime/local-terminal-send-integration.test.ts`

**Interfaces:**
- Consumes: 真实 `callRuntimeRpc`（Task 3 接缝）、真实 `sendNotesToActiveAgentSession`、`createNoteSendAppState` / `LEAF_ID` / `PASTE_BEGIN` / `PASTE_END`（`@/lib/active-agent-note-send-test-harness`）、`makeAgentStatusEntry`（`./sync-runtime-graph-test-harness`）；mock `window.api.pty`（`listSessions`/`writeAccepted`/`onData`/`onExit`/`onSpawned`）与 `window.api.runtime.call`（断言不被调用）。
- Produces: 端到端证据：菜单发送 → 适配器 5 方法 → 括号粘贴 + 延迟 Enter 的真实写入序列；permission 路径返回 `status:'permission'`。

- [x] **Step 1: 写测试**

创建 `src/renderer/src/runtime/local-terminal-send-integration.test.ts`：

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  sendNotesToActiveAgentSession
} from '@/lib/active-agent-note-send'
import {
  createNoteSendAppState,
  LEAF_ID,
  PASTE_BEGIN,
  PASTE_END,
  type NoteSendAppState
} from '@/lib/active-agent-note-send-test-harness'
import { makeAgentStatusEntry } from './sync-runtime-graph-test-harness'

const testState = vi.hoisted(() => ({
  appState: null as unknown as NoteSendAppState
}))

vi.mock('@/store', () => ({
  useAppStore: Object.assign(
    (selector: (state: NoteSendAppState) => unknown) => selector(testState.appState),
    { getState: () => testState.appState }
  )
}))

const listSessions = vi.fn()
const writeAccepted = vi.fn()
const runtimeCall = vi.fn()

beforeEach(() => {
  testState.appState = createNoteSendAppState()
  listSessions.mockReset()
  writeAccepted.mockReset()
  runtimeCall.mockReset()
  listSessions.mockResolvedValue([
    { id: 'pty-1', cwd: '/tmp/wt', title: '', worktreeId: 'wt-1', agentOwnership: 'present' }
  ])
  writeAccepted.mockResolvedValue(true)
  runtimeCall.mockRejectedValue(new Error('local runtime.call must not be used for terminal methods'))
  vi.stubGlobal('window', {
    api: {
      pty: {
        listSessions,
        writeAccepted,
        onData: vi.fn(() => () => {}),
        onExit: vi.fn(() => () => {}),
        onSpawned: vi.fn(() => () => {})
      },
      runtime: { call: runtimeCall }
    }
  })
})

describe('local notes send end-to-end through the local terminal adapter', () => {
  it('pastes unsent notes into an idle running agent and submits Enter', async () => {
    testState.appState.agentStatusByPaneKey[`tab-1:${LEAF_ID}`] = makeAgentStatusEntry({
      state: 'waiting',
      updatedAt: Date.now()
    })

    const result = await sendNotesToActiveAgentSession({
      worktreeId: 'wt-1',
      prompt: 'fix the bug'
    })

    expect(result).toEqual({ status: 'sent' })
    const writes = writeAccepted.mock.calls.map((call) => call[1] as string)
    expect(writes[0]).toBe(`${PASTE_BEGIN}fix the bug${PASTE_END}`)
    expect(writes[1]).toBe('\r')
    expect(runtimeCall).not.toHaveBeenCalled()
  })

  it('reports permission instead of pasting while the agent is blocked', async () => {
    testState.appState.agentStatusByPaneKey[`tab-1:${LEAF_ID}`] = makeAgentStatusEntry({
      state: 'blocked',
      updatedAt: Date.now()
    })

    const result = await sendNotesToActiveAgentSession({
      worktreeId: 'wt-1',
      prompt: 'fix the bug'
    })

    expect(result).toMatchObject({ status: 'permission' })
    expect(writeAccepted).not.toHaveBeenCalled()
  })
})
```

- [x] **Step 2: 跑测试**

Run: `pnpm vitest run src/renderer/src/runtime/local-terminal-send-integration.test.ts`
Expected: PASS（2 条）。失败时优先检查：harness state 缺 `worktreesByRepo` 是否导致 `getSettingsForWorktreeRuntimeOwner` 抛错（现有发送栈测试证明不会）；`pty dispatcher` 还要求别的 `window.api.pty` 方法时补齐 no-op stub。

- [x] **Step 3: 新 agent 路径验证（既有覆盖，只跑不改）**

Run: `pnpm vitest run src/renderer/src/lib/agent-paste-draft.test.ts src/renderer/src/lib/agent-draft-readiness.test.ts`
Expected: PASS —— 证明 `launchAgentInNewTab → pasteDraftWhenAgentReady` 的本地直连（`pty.onData` 就绪 + `pty.write`）仍然成立；本阶段不改该路径。

- [x] **Step 4: 提交**

```bash
git add src/renderer/src/runtime/local-terminal-send-integration.test.ts
git commit -m "test(renderer): 发送栈 local 端到端集成（真实适配器 + mock pty）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: 门禁复跑 + 收尾记录

**Files:**
- Create: `docs/phase2c-diff-annotations-review-record.md`
- Modify: `docs/superpowers/plans/2026-10-07-phase2c-diff-annotations-review.md`（勾选本计划复选框）

**Interfaces:**
- Consumes: 全部前序任务；spec §6 门禁与手工验收清单。
- Produces: 收尾记录（范围、提交列表、门禁证据、手工验收清单、风险披露）。

- [x] **Step 1: 全量门禁**

Run: `cargo test --workspace`
Expected: 全绿。

Run: `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`
Expected: exit 0（仅既有 chunk-size 警告）。

Run: `pnpm test`
Expected: 全绿；若仅 `browser-history-match.performance.test.ts` 失败，单独复跑：`pnpm vitest run src/renderer/src/components/browser-history-match.performance.test.ts`，通过即记录为环境抖动。

- [x] **Step 2: 写收尾记录**

创建 `docs/phase2c-diff-annotations-review-record.md`，结构照 `docs/phase2b1-notifications-native-closeout-record.md`：§1 范围与验收（自动 + 手工）、§2 提交清单（hash + 标题）、§3 门禁证据（命令 + 结果数字）、§4 手工验收清单（下列 6 项，标注「待用户复核」）、§5 偏差与边界备案（照抄 spec §7 五条 + 实现中发现的新偏差）、§6 已知边界与后续（远端 runtime terminal.*、sentAt/已解决模型、PR 评论等）。

手工验收清单（写入记录，供用户执行）：
1. 在 worktree diff 上添加注释 → 完全重启 app → 注释仍在原文件/行；
2. 添加未发送注释 → 发送菜单选运行中的 claude → 文本出现在输入框并提交；
3. 发送菜单选「新 agent」→ 新终端启动后提示词自动粘贴并提交；
4. markdown 文件的注释（源码/预览任一）添加 → 重启仍在 → 可发送；
5. folder workspace 注释添加 → 重启仍在；
6. 侧栏 Notes 架：点击定位、复制、清除（单个/全部）回归。

- [x] **Step 3: 提交**

```bash
git add docs/phase2c-diff-annotations-review-record.md \
  docs/superpowers/plans/2026-10-07-phase2c-diff-annotations-review.md
git commit -m "docs: Phase 2C 实施记录与门禁证据（本地注释全链路）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- §3.1 持久化修复 → Task 1（Rust 投影 + bindings + store reload 测试）、Task 2（渲染层回归）。
- §3.2 本地终端 RPC 适配器 → Task 3（骨架 + 接缝 + list）、Task 4（agentStatus/isRunningAgent）、Task 5（wait）、Task 6（send）。
- §3.3 新 agent 路径 → Task 7 Step 3（既有测试验证，不改）。
- §3.4 发送后语义 → 未改代码（既有 `clearDeliveredDiffComments` 生效）；Task 7 集成测试覆盖发送成功路径。
- §4 映射表/数据流 → Task 4 实现 + 测试。
- §5 错误处理 → Task 3（handle_stale/method_not_found）、Task 5（exited）、Task 6（terminal_exited/refusedReason）。
- §6 测试与门禁 → 各任务 + Task 8。
- §7 风险披露 → Task 8 记录 §5。

**Placeholder scan:** 无 TBD/TODO；所有代码步骤含完整代码。

**Type consistency:** `LocalTerminalLocation`、`readLocalAgentStatus`、`hasAgentTitleEvidence`、`hasIdleTitleEvidence`、`localTerminalFailure`、`isPtyLive`、`readTerminalHandle`、`findLocalTerminalLocation`、`collectLocalTerminalLocations` 在 Task 3/4/5/6 中签名一致；`callLocalTerminalRpc` 的 switch 在各任务中只增 case；`RuntimeTerminalSend.bytesWritten`、`RuntimeTerminalWait.exitCode` 等字段与 `shared/runtime-terminal-contracts.ts` 一致。
