# 常驻分支树区块 + 移除头部弹层（增量 2C）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把分支树从「点头部分支名弹一个 Popover」改为常驻在 Source Control 面板 header 之下的**可折叠区块**（默认展开、折叠态持久化到 localStorage），并**移除头部弹层入口**（`HeadIdentity` 回退成纯展示）——同时把 2B 交付的原地 Checkout 编排迁进常驻区块、保住其全部守卫。

**Architecture:** 复用 2B 的共享树接缝（`BranchRefTree` + `renderRowMenu` + `branch-actions` 的 checkout/switch/new-workspace + `BranchRowMenu` + `CheckoutConfirmDialog`）。把 `SourceControlBranchSwitcher` 的**动作装配**（executeCheckout / ctx / 确认框 / 菜单接线）平移进新的 `SourceControlBranchTreeSection`，删掉其 Popover 外壳。常驻区块在面板内 → 从 `model` 取 `refreshActiveGitStatusAfterMutation` 与 `activeRepoSettings`（比 2B 独立 switcher 的 `settings:null` 自取更正确），且因不关弹层 → **成功检出后必须调 `useBranchRefs().invalidate()`**（2B 留下的、届时必须补回的那条）。

**Tech Stack:** TypeScript、React、Radix（仅 Popover 被移除）、Vitest + happy-dom。无新增 Rust（checkout 后端 2A、前端编排 2B 已就位）。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` §13.2（决策 1 落点 / 决策 4 旧入口）、§13.6（新区块 `SourceControlBranchTreeSection` 命名与位置、折叠持久化）、§13.8（2C 定义）、§13.4（元数据同步，守卫须随之迁移不得丢）。前置：2A 后端、2B 计划 `docs/superpowers/plans/2026-10-25-branch-checkout-increment2b.md`。

## Global Constraints

- **不新增 `RightSidebarTab` 变体**（spec §13.2 决策 1，避开闭合联合的路由级联）。区块是 Source Control 面板内部的一块。
- **命名**：新块 `SourceControlBranchTreeSection`。⚠️ `listing/branch-section.tsx` 已导出 `SourceControlBranchSection`（分支**比较**变更列表），二者不可混。
- **落点**：`panel-ready.tsx` 中 `SourceControlHeaderToolbar` 之下、`SourceControlNotesShelf` 之上（spec §13.6 原文）。默认展开、可折叠、占高有上限、超出内部滚动（`BranchRefTree` 自带 `max-h-72 overflow-auto` + 搜索框）。
- **折叠态持久化 localStorage**，沿用仓库既有写法（见 `floating-terminal-panel-view-state.ts`：`getWindowStorage()` 守卫 + `try/catch` + JSON + 逐字段回落，key 形如 `'orca-…-v1'`）。这是面板**自身**的持久化，与 `use-panel-view-state.ts` 里 `collapsedSections`（内存态、切 worktree 即重置）**不是同一套**——分支区块的折叠不得复用 `collapsedSections`。
- **移除头部弹层入口**：`HeadIdentity` 恒渲染纯展示（Tooltip span），不再 `aria-haspopup="tree"`、不再 `button`、不再包 `SourceControlBranchSwitcher`（spec §13.2 决策 4）。随之删除 `switcherWorktreeId` / `branchSwitcherWorktreeId` 沿 `branch-context-row → header-toolbar → panel-ready` 的传递链。
- **动作装配不丢**：2B 的 checkout / F1 置灰+跳转 / 脏+会话确认框 / 立即刷状态 / 元数据跟随守卫必须在常驻区块里等价保留。`SourceControlBranchSwitcher` 退役；其测试与 `source-control-branch-checkout-metadata.test.tsx` 迁移到常驻区块。
- **面板内刷新走 foundation**：成功检出后调 `model.refreshActiveGitStatusAfterMutation()`（正确路由 settings/connectionId），再调 `invalidate()` 让常驻树重新取数。`activeRepoSettings` 透传给 `checkoutRuntimeGit`（替换 2B 的 `settings:null` hack；非 local 目标仍由 checkoutRuntimeGit 显式拒绝）。
- **最大回归风险（2B 血泪）**：面板测试以**部分 store 桩**渲染 `panel-ready`。新区块挂载会读 `gitStatusByWorktree`/`tabsByWorktree`/`unifiedTabsByWorktree` 并发 `window.api.git.refs` 请求。→ 所有 store 选择器**必须可选链**；`useBranchRefs` 必须**容忍 `window.api.git.refs` 缺失**（不发请求、降级空列表），否则重演"一片 SourceControl 测试崩"。
- **用户可见文案走前端 i18n**：新增区块标题等只补 `auto.components.git-branch` 的 **en+zh**，其余 4 语言靠 `translate(key, defaultValue)` 兜底；`translate` 一律惰性求值（`no-top-level-translate` AST 门禁）。
- 仓库**无 pre-commit 钩子**，门禁手动：`pnpm typecheck` + 定向 `pnpm vitest run`；收尾全量（live-shell O-2 除外）。

## 已核实、可直接依赖的事实（本计划撰写前已实测）

| 项 | 结论（文件:行） |
|---|---|
| panel-ready 结构 | `SourceControlHeaderToolbar` → 条件 `SourceControlNotesShelf` → `flex-1 overflow-auto` 容器内 `SourceControlPanelContent` → `BulkActionBar`（`panel/panel-ready.tsx:48-111`）。新区块插在 HeaderToolbar 之后、NotesShelf 之前。 |
| 头部弹层链 | `panel-ready.tsx:72 branchSwitcherWorktreeId={activeWorktreeId}` → `header-toolbar.tsx:32/68/196` → `branch-context-row.tsx` `SourceControlBranchContextRow` → `HeadIdentity switcherWorktreeId`（`:271/:317`）→ `:139-151` 包 `SourceControlBranchSwitcher`+`button aria-haspopup="tree"`。纯展示分支（无 switcher）路径已在 `:112-133` 存在（Tooltip span，`data-testid="source-control-head-identity"`）。 |
| switcher 消费面 | `SourceControlBranchSwitcher` 仅 `branch-context-row.tsx:10`（非测试）引用；测试：`source-control-branch-switcher.test.tsx` + `source-control-branch-checkout-metadata.test.tsx`。退役面可控。 |
| model 暴露刷新/宿主 | `use-panel-model.ts` 返回 `{...foundation,...}`，含 `refreshActiveGitStatusAfterMutation`（经 foundation L42）、`activeRepoSettings`（L19）、`activeWorktreeId`、`worktreePath`、`entries`、`grouped`。常驻区块从 `model`（= `SourceControlPanelReadyProps.model`）取。 |
| 面板视图态持久化边界 | `use-panel-view-state.ts` 的 `collapsedSections`/`toggleSection` 是内存态，切 worktree 重置（L57-66）；**不**满足 spec「折叠态持久化」→ 分支区块另建持久模块。 |
| localStorage 蓝本 | `floating-terminal/floating-terminal-panel-view-state.ts`：`getWindowStorage()`、`readPersisted…()`（try/catch + JSON.parse + 逐字段 `=== true`）、`persist…()`（try/catch 静默）。 |
| 2B 编排可平移 | `source-control-branch-switcher.tsx` 内 `executeCheckout`（checkoutRuntimeGit → checked-out-elsewhere 按 `owners` 短名跳转、否则 `worktreePath` 提示 → invalid-target/catch → 成功 `setOpen(false)`+`refreshGitStatusForWorktree`(内层 try)+invalidate 已移除）+ `ctx.checkout`（`needsCheckoutConfirmation` 才挂 `CheckoutConfirmDialog`）+ `renderRowMenu`(HOST_A_ALLOWED)。`HOST_A_ALLOWED = {checkout,switch,new-workspace}`。 |
| invalidate 现状 | 2B 在 switcher 里移除了 invalidate（弹层一关即 null path，空转）。**常驻区块不关 → 必须重新调 `invalidate()`**（2B 已记 deferred）。 |
| useBranchRefs | `window.api.git.refs({worktreePath})`（`git-branch/use-branch-refs.ts`），`worktreePath` 为 null 时不取数；返回 `{refs,truncated,loading,error,invalidate}`。当前对 `window.api.git.refs` 缺失**不容忍**（同步抛）→ 2C 需加固。 |

---

## 文件结构

**新建**

| 文件 | 职责 |
|---|---|
| `src/renderer/src/components/git-branch/source-control-branch-section-view-state.ts` | 分支区块折叠态的 localStorage 读写（`readPersistedBranchSectionCollapsed()` / `persistBranchSectionCollapsed(bool)`），照 floating-terminal 蓝本。默认展开（读不到 = false）。 |
| `src/renderer/src/components/git-branch/source-control-branch-tree-section.tsx` | `SourceControlBranchTreeSection`：常驻可折叠区块，渲染折叠头 + `BranchRefTree`（host A 动作 + 菜单接缝 + 确认框）。承接 switcher 的动作装配，去掉 Popover。 |

**修改**

| 文件 | 改动 |
|---|---|
| `src/renderer/src/components/git-branch/use-branch-refs.ts` | 取数前守卫 `window.api?.git?.refs` 非函数时不发请求、置空列表（不抛）。 |
| `src/renderer/src/components/right-sidebar/source-control/panel/panel-ready.tsx` | 在 HeaderToolbar 后插入 `<SourceControlBranchTreeSection …/>`（传 `worktreeId=activeWorktreeId`、`settings=activeRepoSettings`、`refreshActiveGitStatusAfterMutation`）；删除 HeaderToolbar 的 `branchSwitcherWorktreeId` 传参。 |
| `src/renderer/src/components/right-sidebar/source-control/panel/header-toolbar.tsx` | 删 `branchSwitcherWorktreeId` prop（声明、默认、向 BranchContextRow 的转发）。 |
| `src/renderer/src/components/right-sidebar/source-control/panel/branch-context-row.tsx` | `HeadIdentity` 删弹层分支与 `switcherWorktreeId` 参数（恒纯展示）；`SourceControlBranchContextRow` 删 `branchSwitcherWorktreeId` prop；删 `SourceControlBranchSwitcher` import。 |
| `src/renderer/src/i18n/locales/en.json` / `zh.json` | `auto.components.git-branch` 补区块标题键（en+zh）。 |

**删除**

| 文件 | 处置 |
|---|---|
| `src/renderer/src/components/git-branch/source-control-branch-switcher.tsx` | 退役。动作装配迁到 `source-control-branch-tree-section.tsx`。 |
| `src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx` | 迁移为 `source-control-branch-tree-section.test.tsx`（常驻、无开弹层前置；保留 checkout/confirm/F1/occupant/远程 target/合并计数 等断言）。 |
| `src/renderer/src/components/git-branch/source-control-branch-checkout-metadata.test.tsx` | 改指向 `SourceControlBranchTreeSection`（断言仍为 checkout 成功 → `updateWorktreeGitIdentity` 收到完整 ref）。 |

> `git-log/Toolbar.tsx`（宿主 B）**不动**：仍走自己的 `onPickRef`，无 checkout、无 renderRowMenu。`branch-ref-tree.tsx` 的 `renderRowMenu` 接缝、`branch-menu.tsx`、`branch-actions.ts`、`checkout-confirm-dialog.tsx`、`git-branch/branch-tree-model.ts` **不动**（被常驻区块直接复用）。

---

## Task 1: 折叠态持久化模块

**Files:**
- Create: `src/renderer/src/components/git-branch/source-control-branch-section-view-state.ts`
- Test: `src/renderer/src/components/git-branch/source-control-branch-section-view-state.test.ts`

**Interfaces:**
- Produces: `BRANCH_SECTION_VIEW_STATE_STORAGE_KEY`、`readPersistedBranchSectionCollapsed(): boolean`（读不到/异常 → `false`=展开）、`persistBranchSectionCollapsed(collapsed: boolean): void`

- [ ] **Step 1: 写失败测试**

```ts
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  BRANCH_SECTION_VIEW_STATE_STORAGE_KEY,
  readPersistedBranchSectionCollapsed,
  persistBranchSectionCollapsed,
} from './source-control-branch-section-view-state'

describe('branch section view state', () => {
  const store: Record<string, string> = {}
  const mockStorage = {
    getItem: (k: string) => store[k] ?? null,
    setItem: (k: string, v: string) => { store[k] = v },
    removeItem: (k: string) => { delete store[k] },
  }
  const install = () => vi.spyOn(window, 'localStorage', 'get').mockReturnValue(mockStorage as Storage)
  afterEach(() => { vi.restoreAllMocks(); for (const k of Object.keys(store)) delete store[k] })

  it('读不到 → 默认展开（false）', () => { install(); expect(readPersistedBranchSectionCollapsed()).toBe(false) })
  it('持久化 collapsed=true 后读回 true', () => { install(); persistBranchSectionCollapsed(true); expect(readPersistedBranchSectionCollapsed()).toBe(true) })
  it('损坏 JSON → 回落 false，不抛', () => { store[BRANCH_SECTION_VIEW_STATE_STORAGE_KEY] = '{ not json'; install(); expect(readPersistedBranchSectionCollapsed()).toBe(false) })
})
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-section-view-state.test.ts`
Expected: FAIL —— 模块不存在。

- [ ] **Step 3: 实现（照 floating-terminal-panel-view-state.ts 形状）**

```ts
export const BRANCH_SECTION_VIEW_STATE_STORAGE_KEY = 'orca-source-control-branch-section-v1'

function getWindowStorage(): Storage | null {
  return typeof window === 'undefined' ? null : window.localStorage
}

// { collapsed: boolean } —— 默认展开：读不到一律 false。
export function readPersistedBranchSectionCollapsed(): boolean {
  try {
    const raw = getWindowStorage()?.getItem(BRANCH_SECTION_VIEW_STATE_STORAGE_KEY)
    if (!raw) return false
    const parsed: unknown = JSON.parse(raw)
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return false
    return (parsed as Record<string, unknown>).collapsed === true
  } catch {
    return false
  }
}

export function persistBranchSectionCollapsed(collapsed: boolean): void {
  try {
    getWindowStorage()?.setItem(
      BRANCH_SECTION_VIEW_STATE_STORAGE_KEY,
      JSON.stringify({ collapsed })
    )
  } catch {
    // 存储不可用/满：丢一次恢复状态不值得崩面板。
  }
}
```

> 与蓝本 `floating-terminal-panel-view-state.ts:9-11` 的 `getWindowStorage()` 逐字一致（`typeof window === 'undefined' ? null : window.localStorage`）。

- [ ] **Step 4: 运行确认通过**

Run: `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-section-view-state.test.ts`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src/renderer/src/components/git-branch/source-control-branch-section-view-state.ts src/renderer/src/components/git-branch/source-control-branch-section-view-state.test.ts
git commit -m "feat(git-branch): 分支区块折叠态 localStorage 读写（默认展开，照 floating-terminal 蓝本）"
```

---

## Task 2: 加固 useBranchRefs 容忍 api 缺失（防面板崩桩的前置）

**Files:**
- Modify: `src/renderer/src/components/git-branch/use-branch-refs.ts`
- Test: `src/renderer/src/components/git-branch/use-branch-refs.test.tsx`

**Interfaces:**
- Produces: `useBranchRefs(path)` 在 `path` 非空但 `window.api?.git?.refs` 不是函数时**不发请求**、置 `refs=[]`、`error=null`、`loading=false`（而不是同步抛 TypeError）。

- [ ] **Step 1: 写失败测试**（追加到 use-branch-refs.test.tsx，复用其既有 `Probe`/`latest` harness，非 `renderHook`）

该测试文件已有：`function Probe({ path }) { latest = useBranchRefs(path); return ... }`、模块级 `latest: Result`、`beforeEach` 里 `window.api = { git: { refs } }`。新用例临时把 api 换成缺 `refs` 的桩并渲染 Probe：

```tsx
it('window.api.git.refs 缺失时不抛，降级空列表', async () => {
  const prevApi = (window as unknown as { api: unknown }).api
  ;(window as unknown as { api: unknown }).api = { git: {} } // 无 refs
  try {
    await act(async () => { root.render(<Probe path="/some/path" />) })
    await act(async () => { await Promise.resolve() })
    expect(latest.refs).toEqual([])
    expect(latest.error).toBeNull()
    expect(latest.loading).toBe(false)
  } finally {
    ;(window as unknown as { api: unknown }).api = prevApi
  }
})
```

- [ ] **Step 2: 运行确认失败**（当前 `window.api.git.refs({...})` 对 `{git:{}}` 同步抛）。

- [ ] **Step 3: 实现守卫**（在取数 effect 内、调用前）

```ts
useEffect(() => {
  if (!worktreePath) { setLoading(false); return }
  const refsFn = (window as { api?: { git?: { refs?: unknown } } })?.api?.git?.refs
  if (typeof refsFn !== 'function') {
    // 面板测试/非 git worktree 可能没有 refs 能力：降级空列表而不是抛，避免崩掉整个面板渲染。
    setRefs([]); setTruncated(false); setError(null); setLoading(false); return
  }
  let alive = true
  setLoading(true)
  ;(refsFn as (a: { worktreePath: string }) => Promise<{ refs: GitRefEntry[]; truncated: boolean }>)({ worktreePath })
    .then(/* 原逻辑 */)
    .catch(/* 原逻辑 */)
  // 依赖数组维持 [worktreePath, version]
}, [worktreePath, version])
```

- [ ] **Step 4: 运行确认通过** —— 既有用例仍绿 + 新例绿。
- [ ] **Step 5: 提交** `git commit -m "fix(git-branch): useBranchRefs 容忍 window.api.git.refs 缺失，降级空列表不抛"`

---

## Task 3: `SourceControlBranchTreeSection` —— 迁移动作装配 + 常驻可折叠

**Files:**
- Create: `src/renderer/src/components/git-branch/source-control-branch-tree-section.tsx`
- Test: `src/renderer/src/components/git-branch/source-control-branch-tree-section.test.tsx`（由 switcher 测试迁移）

**Interfaces:**
- Consumes: `BranchRefTree`（含 `renderRowMenu`）、`BranchRowMenu`、`primaryBranchAction`/`BranchActionContext`/`BranchActionId`、`CheckoutConfirmDialog`/`needsCheckoutConfirmation`、`checkoutRuntimeGit`、`useBranchRefs`（Task 2 加固）、`ownerByBranch`、`branchName`、折叠态模块（Task 1）、`toast`（sonner）、`activateAndRevealWorktree`、`findWorktreeById`
- Produces:
```ts
export type SourceControlBranchTreeSectionProps = {
  worktreeId: string
  /** 面板内可用时优先用 foundation 的刷新；缺省回落到模块级 refreshGitStatusForWorktree。 */
  refreshActiveGitStatusAfterMutation?: () => Promise<void>
  /** foundation 的 activeRepoSettings；缺省 null（本地）。 */
  settings?: RuntimeGitContext['settings']
}
export function SourceControlBranchTreeSection(props): React.JSX.Element | null
```

- [ ] **Step 1: 迁移并改测试（先写失败测试）**

以 `source-control-branch-switcher.test.tsx` 为底本，产出 `source-control-branch-tree-section.test.tsx`：
- 渲染入口从「`<SourceControlBranchSwitcher><button>main</button></SourceControlBranchSwitcher>` + `openSwitcher()` 点开弹层」改为「直接 `<SourceControlBranchTreeSection worktreeId="r::/a" refreshActiveGitStatusAfterMutation={spy} settings={null} />`」，树默认展开即在 DOM，无需开弹层步骤（删 `openSwitcher` 里的点 trigger 逻辑，保留等 refs 落定的 flush）。
- 保留并按需改写的断言（全部沿用 2B 的 store/`window.api`/`sonner`/`git-status-refresh` 桩）：
  - checkout 本地/远程 target、干净不弹框、脏→弹框→确认后 checkout、终端+agent 合并计数（2终端+1agent+1editor→3）、F1→switch 跳转不 checkout、点 HEAD 无动作、owners 命中跳转、外部占用→worktreePath 提示、invalid-target→toast.error 不刷、环境失败→toast.error 不刷、不注入 All Branches、跨仓 main 归属、worktree 解析不到→渲染 null、取数失败→一行文案。
  - **新增/改动两条关键断言**（常驻语义）：
    - `it('检出成功 → 调 refreshActiveGitStatusAfterMutation（不再走模块 refresh）')`：传入 spy，断被调 1 次（证明走 foundation 路径）。
    - `it('检出成功 → 常驻树 invalidate 重新取数（refs 二次请求）')`：`refs` mock 计数从 1 增到 2（证明 invalidate 生效，这是 2B 在弹层里没做到的）。
  - `it('折叠态持久化：点折叠头写 localStorage，重挂仍折叠')`：断 `localStorage.setItem` 被以 `{collapsed:true}` 调用；模拟已折叠 storage 重挂 → 树不渲染（折叠即不可见）。
- 迁移后 `git rm` 旧 `source-control-branch-switcher.test.tsx`。

- [ ] **Step 2: 运行确认失败**（section 组件尚不存在）。

- [ ] **Step 3: 实现 `source-control-branch-tree-section.tsx`**

结构：把 switcher 的 `executeCheckout`/`ctx`/`pendingCheckout`/`renderRowMenu` 平移过来，改动点：
1. **去掉 Popover 壳**：不 import `Popover*`；`open` state 换成折叠 `collapsed`（初值 `readPersistedBranchSectionCollapsed()`，`toggleCollapse` 调 `persistBranchSectionCollapsed(next)`）。
2. **取数**：`useBranchRefs(!collapsed && worktree ? worktree.path : null)` —— 折叠时传 null（不请求），展开时请求。**保留 invalidate**：成功路径 `await refresh(); invalidate()`。
3. **刷新用 foundation**：`executeCheckout` 成功路径改为：
```ts
if (refreshActiveGitStatusAfterMutation) {
  try { await refreshActiveGitStatusAfterMutation() } catch (e) { console.warn('[git-branch] post-checkout refresh failed', e) }
} else {
  try { await refreshGitStatusForWorktree({ worktreeId, worktreePath: worktree.path, deps: {…store getState…}, request: { admissionTier: 'interactive' } }) }
  catch (e) { console.warn(...) }
}
invalidate()
```
（`checkoutRuntimeGit` 的 context 用 `{ settings: settings ?? null, worktreeId, worktreePath: worktree.path }`——面板传入 activeRepoSettings 时即正确路由。）
4. **确认框数据源**：dirty/session 仍从 store 可选链自取（`gitStatusByWorktree?.[id]`、`tabsByWorktree?.[id]?.length`、`unifiedTabsByWorktree?.[id]?.filter(agent-session)`），与 2B 一致，注释保留"不双计"。
5. **渲染**：`if (!worktree) return null`（解析不到 worktree 整块不渲染）；否则
```tsx
<section data-testid="source-control-branch-tree-section" className="flex flex-col border-b border-border/60">
  <button type="button" data-testid="branch-section-toggle" aria-expanded={!collapsed}
    onClick={toggleCollapse} className="… 区块头（chevron + 标题）…">
    {collapsed ? '›' : '⌄'} {translate('auto.components.git-branch.branchesSection', 'Branches')}
  </button>
  {!collapsed && (
    loading && refs.length===0 ? <p>…loading…</p>
    : error && refs.length===0 ? <p>…失败一行…</p>
    : <BranchRefTree refs={refs} truncated headBranch={ctx.headBranch} checkedOutByBranch={owners}
        onActivate={(row) => primaryBranchAction(row, ctx)?.run(row, ctx)}
        renderRowMenu={(row, content) => row.kind==='ref' ? <BranchRowMenu row={row} ctx={ctx} allowedActionIds={HOST_A_ALLOWED}>{content}</BranchRowMenu> : content} />
  )}
  {pendingCheckout && <CheckoutConfirmDialog open input={{…}} onCancel onConfirm/>}
</section>
```
（`HOST_A_ALLOWED` 常量从 switcher 搬来；`EMPTY_ENTRIES`、`checkoutInFlightRef` 防并发一并搬入，去掉 switcher 里"本增量只服务本地"的 `settings:null` 硬编，改 `settings ?? null`。）

- [ ] **Step 4: 运行确认通过** `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-tree-section.test.tsx`。
- [ ] **Step 5: 判别力实证**（临时把 `invalidate()` 去掉 → "refs 二次请求"用例应转红；把成功路径 `refreshActiveGitStatusAfterMutation()` 调用去掉 → "调 refresh"用例转红；还原）。
- [ ] **Step 6: 提交**（含 section 组件 + 迁移测试 + 删旧 switcher 测试；旧 switcher 源文件留待 Task 4 删）。

---

## Task 4: 退役 SourceControlBranchSwitcher + 接线 panel-ready + 移除头部弹层

**Files:**
- Delete: `src/renderer/src/components/git-branch/source-control-branch-switcher.tsx`
- Modify: `panel/panel-ready.tsx`、`panel/header-toolbar.tsx`、`panel/branch-context-row.tsx`
- Test: `panel/SourceControl*.test.tsx`（若断言头部弹层触发器 `aria-haspopup="tree"` 或 `branchSwitcherWorktreeId`，相应改）、`branch-context-row` 相关测试

**Interfaces:**
- Consumes: `SourceControlBranchTreeSection`（Task 3）
- Produces: 面板渲染常驻区块；`HeadIdentity` 纯展示；switcher 彻底移除。

- [ ] **Step 1: 写/改失败测试**
- `panel-ready` 渲染测试（若有）：断 `document.querySelector('[data-testid="source-control-branch-tree-section"]')` 存在。
- 头部：断 `HeadIdentity` 渲染的 `source-control-head-identity` **不再是 button**、**无 `aria-haspopup`**（纯 span/Tooltip）。先 grep 现有断言 `aria-haspopup` / `branchSwitcherWorktreeId` 的测试并改正向断言。

- [ ] **Step 2: 运行确认失败。**

- [ ] **Step 3: 实现接线**
1. `panel-ready.tsx`：删 `branchSwitcherWorktreeId={activeWorktreeId}`；在 `<SourceControlHeaderToolbar …/>` 之后插入
```tsx
{activeWorktreeId && (
  <SourceControlBranchTreeSection
    worktreeId={activeWorktreeId}
    settings={model.activeRepoSettings}
    refreshActiveGitStatusAfterMutation={model.refreshActiveGitStatusAfterMutation}
  />
)}
```
（`model` 已在 panel-ready 解构处可用；补 `activeRepoSettings`/`refreshActiveGitStatusAfterMutation` 到解构。import section。）
2. `header-toolbar.tsx`：删 `branchSwitcherWorktreeId` prop 声明(L32)、默认(L68)、向 `SourceControlBranchContextRow` 的转发(L196)。
3. `branch-context-row.tsx`：删 `import { SourceControlBranchSwitcher }`；`HeadIdentity` 删 `switcherWorktreeId` 参数与 `:136-151` 弹层分支——函数体保留 detached 分支 + 纯展示 Tooltip span（即原 `!switcherWorktreeId` 分支恒走）；`SourceControlBranchContextRow` 删 `branchSwitcherWorktreeId` 参数与两处 `HeadIdentity` 的 `switcherWorktreeId={…}`。
4. 删 `source-control-branch-switcher.tsx`（`git rm`）。
5. i18n：en/zh `auto.components.git-branch` 加 `branchesSection`（"Branches" / "分支"）。

- [ ] **Step 4: 运行确认通过 + 全量定向回归**
`pnpm vitest run src/renderer/src/components/git-branch src/renderer/src/components/right-sidebar` —— **重点**：所有 `SourceControl.*` 面板测试须因 Task 2 加固 + 选择器可选链而保持绿（这是 2B 崩桩的复发防线）。

- [ ] **Step 5: `pnpm typecheck`** 无错（删 prop 后的调用点全清）。
- [ ] **Step 6: 提交** `git commit -m "feat(git-branch): 2C 常驻分支树区块接入面板，移除头部弹层入口，退役 SourceControlBranchSwitcher"`

---

## Task 5: 元数据跟随守卫迁移 + 面板集成测试

**Files:**
- Modify: `src/renderer/src/components/git-branch/source-control-branch-checkout-metadata.test.tsx`（指向 section）
- Test: `panel/SourceControl.branch-tree-section.test.tsx`（新建，面板级）

**Interfaces:** 断 2B 的 spec §13.4 守卫在 2C 迁移后**仍成立且更严**（invalidate 现在真起作用）。

- [ ] **Step 1**: 把 metadata 测试的渲染入口换成 `SourceControlBranchTreeSection`（不 mock `git-status-refresh`，只截 `@/runtime/runtime-git-client` 的 `getRuntimeGitStatus`；检 `fresh` 无 owner）→ 断 `updateWorktreeGitIdentity('r::/a', { head, branch:'refs/heads/fresh' })`；**并加断**：`window.api.git.refs` 被调用 ≥2 次（invalidate 生效，常驻树刷新）。
- [ ] **Step 2**: 新建面板级测试：渲染 `SourceControlPanelReady`（或既有面板 harness），断 `[data-testid="source-control-branch-tree-section"]` 出现、默认展开含 `branch-tree`；折叠后 `branch-tree` 不在 DOM；断头部 `source-control-head-identity` 是 span 非 button。
- [ ] **Step 3: 判别力实证**：去 section 的 `invalidate()` → Step1 refs≥2 断言转红；去 `refreshActiveGitStatusAfterMutation()` 调用 → updateWorktreeGitIdentity 断言转红（若走 foundation spy 路径）；还原。
- [ ] **Step 4: 提交**。

---

## Task 6: 收尾门禁 + 全分支终审

- [ ] `pnpm typecheck`
- [ ] `pnpm vitest run src/renderer/src/components/git-branch src/renderer/src/components/right-sidebar src/renderer/src/components/tab-bar src/renderer/src/store src/renderer/src/i18n src/renderer/src/runtime`（广域回归全绿，尤其 SourceControl.*）
- [ ] `pnpm build:web`
- [ ] 全量 `pnpm test`（live-shell O-2 沙箱会挂，非本次引入，标注）
- [ ] 无 Rust → 不跑 cargo（终审判定需再补）
- [ ] 全分支 CodeReview（范围 `e6b1b825..HEAD`，即 2C 全部提交）

## 手工验收（`pnpm dev` 真窗口，测试替代不了）

1. Source Control 面板 header 下方常驻「分支」区块，默认展开、内含搜索框与树、占高有上限超出内滚。
2. 点分支名（头部）不再弹层（纯展示）；与 Git Log 工具栏那棵树的常驻关系不再是弹层。
3. 折叠区块 → 关开应用/重进面板仍折叠（localStorage）；展开默认。
4. 树内单击可检出分支：干净无会话直接检出、头部/区块内分支标识**立即**更新（非 60s）；脏/有会话先弹确认框（三段，合并计数）。
5. 被别的工作区检出→置灰+跳转；远程→建跟踪分支检出；ade 未知外部占用→提示 worktreePath。
6. 检出后常驻树内 🏷/ahead/behind 立即随 invalidate 刷新（不需收起重开）。

## 范围外

- 头部纯展示后是否给一个"展开/定位到分支区块"的轻量交互 → 不做（spec §13.2 决策 4 只要取消弹层）。
- `git_branch_create`/`set_upstream`/Rename/Delete/Update 参数化、O-5 死路径（fastForward/syncFork/rebase 仍无后端）→ 其后增量。
- 远程 runtime checkout、2A/2B 既有债（O-4 env 守卫）→ 不动。
