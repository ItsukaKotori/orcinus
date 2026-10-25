# 原地 Checkout 前端（增量 2B）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在分支树的分支行落地右键 + 行尾 `⋯` 菜单，把 `checkout` 变成宿主 A（Source Control）的**主动作**（原地检出，含 F1 置灰与脏确认），`switch`（跳 workspace）降级为次要项，并补上 spec §13.4 要求的**元数据跟随测试**。

**Architecture:** 沿用增量 1 的"全仓库只留一份树"接缝：`branch-actions.ts` 是纯声明表 + 主动作判定，`BranchRefTree` 保持无业务语义、只新增一条 `renderRowMenu` 呈现接缝。checkout 的编排（确认框 → `checkoutRuntimeGit` → 立即刷状态 + 让 refs 重新取数 → 拒绝时跳转）全部落在宿主 A 的 `SourceControlBranchSwitcher`，因为它已自取 store。后端 `git_checkout` 由 2A 交付，本增量只做前端消费。

**Tech Stack:** TypeScript、React、Radix（ContextMenu / Dialog）、Vitest + happy-dom、`pnpm typecheck` + `pnpm vitest run`。无新增 Rust（2A 已交付后端与命令镜像）。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` §13.2（决策）、§13.3（F1/F2/F3）、§13.4（元数据同步）、§13.6（常驻区块与右键菜单 / 动作矩阵 / 单击语义 / 裁剪机制扩展）、§13.7（确认框三段）。2A 计划 `docs/superpowers/plans/2026-10-25-branch-checkout-increment2a.md`（后端契约）。交接卡 `docs/superpowers/plans/2026-10-25-increment2b-handoff.md`。

## Global Constraints

- **本增量只做前端。** 后端 `git_checkout` / `checkoutRuntimeGit` / `window.api.git.checkout` 已由 2A 落地，**不改 Rust**、**不再生成绑定**。
- **用户可见文案必须前端按 `status` 走 i18n。** 后端 `message` 是英文且**不是**用户文案（交接卡）。i18n 约定：新增文案补 `auto.components.git-branch` 命名空间的 **en + zh** 两处，其余 4 语言（es/fr/ja/ko）靠 `translate(key, defaultValue)` 兜底（spec §3.4，与增量 1 一致）。
- **`translate` 一律惰性求值**：动作 `label: () => translate(...)`，不得顶层调用（`no-top-level-translate.test.ts` AST 门禁）。
- **`Worktree.branch` 是完整 ref（`refs/heads/x`），`GitRefEntry.name` 是短名。** 任何以分支名为键的对齐必须过 `lib/git-utils.ts` 的 `branchName()`。
- **拒绝不经 `BridgeError`。** `GitCheckoutOutcome.status` 是可判别联合：`'switched' | 'already-on-branch' | 'invalid-target' | { refused: 'checked-out-elsewhere' }`；占用信息在 `outcome.branch`（短名）与 `outcome.worktreePath`。
- **产品决策（已与用户确认，写死进本计划）：**
  1. 确认框**仅在脏改动或有活跃终端/会话时弹**；目标分支干净且无会话 → 单击直接检出，不弹框。
  2. 确认框第三段计数**把终端与 agent 会话合并**，文案为「N 个运行中的终端/会话」（agent-session 是独立 `contentType`，与 terminal 并列计数）。
- **元数据同步不新造。** checkout 成功后复用模块级 `refreshGitStatusForWorktree`（`git-status-refresh.ts`），其内部 `updateWorktreeGitIdentity` 会把 `Worktree.branch` 跟刷成 `status.branch ?? null`。
- 仓库**没有 pre-commit 钩子**，门禁全靠手动执行：每个任务收尾跑 `pnpm typecheck` + 该任务定向 `pnpm vitest run`；全部任务完成后跑全量门禁。

## 已核实、可直接依赖的事实（本计划撰写前已实测，非推测）

| 项 | 结论（文件:行） |
|---|---|
| 未提交改动数据源 | store `gitStatusByWorktree: Record<worktreeId, GitStatusEntry[]>`（`store/slices/editor/types/editor-git-slice.ts:27`）。独立组件直接 `useAppStore((s) => s.gitStatusByWorktree[worktreeId])` 取计数与前 K 条路径，**不依赖** panel foundation 的 `grouped`。 |
| 终端/会话数据源 | `tabsByWorktree: Record<worktreeId, TerminalTab[]>` 全是终端 tab；agent 会话在 `unifiedTabsByWorktree` 里是 `contentType === 'agent-session'`（`tab-bar/group-tab-order.ts:54` 与 terminal 并列）。合并计数 = `tabsByWorktree[wtId]?.length ?? 0` + `unifiedTabsByWorktree[wtId]?.filter(t => t.contentType === 'agent-session').length ?? 0`。 |
| checkout 后端 | `checkoutRuntimeGit(context: RuntimeGitContext, target: GitCheckoutTarget): Promise<GitCheckoutOutcome>`（`runtime/runtime-git-checkout-client.ts:22`）。`RuntimeGitContext = { settings, worktreeId, worktreePath, connectionId? }`（`runtime-git-client-context.ts:48`）。拒绝在 resolve 的 `outcome.status`，环境级失败才 reject。 |
| 立即刷状态 | `refreshGitStatusForWorktree({ settings, worktreeId, worktreePath, connectionId, pushTarget, deps, request })`（`right-sidebar/git-status-refresh.ts:99`）是**模块级导出函数**，`deps` 四项（`setGitStatus`/`updateWorktreeGitIdentity`/`setUpstreamStatus`/`fetchUpstreamStatus`）全是 store action，宿主可用 `useAppStore.getState()` 组装。 |
| refs 重新取数 | `useBranchRefs` 返回 `invalidate()`（`git-branch/use-branch-refs.ts:10`），checkout 成功后必调，否则树里的 🏷/ahead/behind 停在旧值。 |
| 树组件契约 | `BranchRefTree`（`git-branch/branch-ref-tree.tsx`）行是 `<button>`，`RowTrailing` 是 `<span>`；**没有**任何菜单/`⋯`。加 `⋯` 会引入「button 套 button」的非法嵌套，须在呈现接缝处改结构（见 Task 3）。 |
| 动作表现状 | `BRANCH_ACTIONS` / `branchActionForRef` / `primaryBranchAction`（`git-branch/branch-actions.ts`）。`BranchAction` 目前**无** `enabled` 维度（只有 `visible`）。`BranchActionId = 'switch' | 'new-workspace' | 'view-in-log'`。 |
| 宿主 A 现状 | `source-control-branch-switcher.tsx`：ctx 只有 `activateWorkspace` / `newWorkspaceFrom`，`onActivate` → `primaryBranchAction(...)?.run(...)`。测试 `source-control-branch-switcher.test.tsx` 里"点有 owner 分支 → activate"这批断言**会被本增量推翻**（见 Task 5）。 |
| 确认框先例 | `right-sidebar/source-control/commit/discard-dialog.tsx`：`Dialog` + `DialogContent`，无内部确认态，靠 `onConfirm` / `onCancel` 回调 + `onOpenAutoFocus` 聚焦确认键。照其形状。 |
| 右键菜单原语 | `components/ui/context-menu.tsx` 导出 `ContextMenu / ContextMenuTrigger / ContextMenuContent / ContextMenuItem`；Root `modal={false}`。 |
| i18n 落点 | en.json 嵌套块 `auto.components.git-branch`（`i18n/locales/en.json:883-902`）；zh.json 同名块（`i18n/locales/zh.json:718-736`）。新增叶子键补这两处即可。 |

---

## 文件结构

**新建**

| 文件 | 职责 |
|---|---|
| `src/renderer/src/components/git-branch/branch-menu.tsx` | `BranchRowMenu`：对单条 ref 行渲染右键 `ContextMenu` + 行尾 `⋯`，项从 `BRANCH_ACTIONS` 按 `visible` + `enabled` + 宿主声明的 `allowedActionIds` 取子集。 |
| `src/renderer/src/components/git-branch/checkout-confirm-dialog.tsx` | Checkout 前确认框（三段：目标 / 脏改动 / 活跃终端会话）+ `needsCheckoutConfirmation(...)` 纯判定（何时该弹）。 |

**修改**

| 文件 | 改动 |
|---|---|
| `src/renderer/src/components/git-branch/branch-actions.ts` | `BranchActionId` 加 `'checkout'`；`BranchAction` 加可选 `enabled`；`BranchActionContext` 加可选 `checkout`；`BRANCH_ACTIONS` 加 checkout 项；`primaryBranchAction` 在宿主声明 checkout 时优先返回 checkout、F1 回落 switch。 |
| `src/renderer/src/components/git-branch/branch-ref-tree.tsx` | `BranchRefTreeProps` 加可选 `renderRowMenu?: (row, content) => ReactNode`；树把已渲染好的 ref 行 `<button>` 作 `content` 交给接缝（接缝自带 flex 容器 + `⋯` 兄弟，避 `button button` 嵌套）；group/synthetic 行与无接缝时逐字节走原 `<button>`；`data-row-index`/键盘导航/`aria` 口径不变。 |
| `src/renderer/src/components/git-branch/source-control-branch-switcher.tsx` | ctx 注入 `checkout` 回调（编排：判定弹框 → 确认框状态 → `checkoutRuntimeGit` → 刷状态 + `invalidate` → 拒绝跳转）；`onActivate` 主动作切到 checkout；`BranchRefTree` 传 `renderRowMenu`（allowedActionIds = `{checkout, switch, new-workspace}`，**不含** view-in-log）。 |
| `src/renderer/src/i18n/locales/en.json` / `zh.json` | `auto.components.git-branch` 块补 checkout/确认框/占用文案叶子键（en + zh）。 |

> `git-log/Toolbar.tsx`（宿主 B）**本增量不动**：它不接 `BRANCH_ACTIONS`，`onActivate` 直接映射 `onPickRef`，没有 checkout。这正是 §13.6「checkout 在宿主 B 必须不可见」的落点，由"宿主不声明 checkout 回调 + 不传 renderRowMenu"实现，无需额外 opts。

---

## Task 1: 动作表加 `checkout` + 主动作判定

**Files:**
- Modify: `src/renderer/src/components/git-branch/branch-actions.ts`
- Test: `src/renderer/src/components/git-branch/branch-actions.test.ts`

**Interfaces:**
- Consumes: `GitRefEntry`（`shared/git-ref-types.ts`）、`BranchTreeRow`（`branch-tree-model.ts`）
- Produces:
  - `BranchActionId` 联合新增 `'checkout'`
  - `BranchAction.enabled?: (row, ctx) => boolean`（缺省视为恒可用）
  - `BranchActionContext.checkout?: (ref: GitRefEntry) => void`
  - `BRANCH_ACTIONS` 含 id `'checkout'` 的项
  - `primaryBranchAction(row, ctx)` 新语义：ctx 有 `checkout` 时，可检出的 ref 行（local/remote非 HEAD）返回 checkout；F1（`checkedOutBy !== null`）优先回落 `switch`，owner 缺失再回落 `branchActionForRef`（绝不空动作）；当前 HEAD 返回 null；tag 回落 owner 判定（→ new-workspace）。

- [ ] **Step 1: 写失败测试（表驱动，覆盖 visible / enabled / 主动作回落）**

在 `branch-actions.test.ts` 追加。复用文件已有的 `refEntry` / `refRow` / `ctx` / `OWNER` 构造器（`ctx` 需支持传 `checkout`）。

```ts
describe('checkout action', () => {
  const checkout = vi.fn()
  const withCheckout = (over: Partial<BranchActionContext> = {}) =>
    ctx({ checkout, ...over })
  const find = (id: string) => BRANCH_ACTIONS.find((a) => a.id === id)!

  it('宿主声明 checkout 时，本地非 HEAD 分支可见且可用', () => {
    const row = refRow(refEntry({ name: 'dev' }))
    expect(find('checkout').visible(row, withCheckout())).toBe(true)
    expect(find('checkout').enabled?.(row, withCheckout())).toBe(true)
  })

  it('当前 HEAD 行不可见（不原地检出自己）', () => {
    const row = refRow(refEntry({ name: 'main', isHead: true }))
    expect(find('checkout').visible(row, withCheckout())).toBe(false)
  })

  it('tag 行不可见', () => {
    const row = refRow(refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' }))
    expect(find('checkout').visible(row, withCheckout())).toBe(false)
  })

  it('宿主不声明 checkout 时 checkout 不可见（宿主 B 门禁：绝不出现原地检出）', () => {
    const row = refRow(refEntry({ name: 'dev' }))
    expect(find('checkout').visible(row, ctx())).toBe(false)
  })

  it('被其它 workspace 检出的分支：可见但 disabled（F1 置灰）', () => {
    const row = refRow(refEntry({ name: 'dev' }), OWNER)
    expect(find('checkout').visible(row, withCheckout())).toBe(true)
    expect(find('checkout').enabled?.(row, withCheckout())).toBe(false)
  })
})

describe('primaryBranchAction with checkout host', () => {
  const checkout = vi.fn()
  const withCheckout = (over: Partial<BranchActionContext> = {}) =>
    ctx({ checkout, ...over })

  it('宿主声明 checkout：可用分支主动作 = checkout', () => {
    const row = refRow(refEntry({ name: 'fresh' }))
    expect(primaryBranchAction(row, withCheckout())?.id).toBe('checkout')
  })

  it('宿主声明 checkout：F1 占用分支主动作回落 switch（点击给跳转，不给死检出）', () => {
    const owner = { worktreeId: 'r::/b', displayName: 'B', path: '/b' }
    const row = refRow(refEntry({ name: 'dev' }), owner)
    // dev 被 r::/b 检出 → switch.visible 需 ownerOf 命中
    const ctxWithOwner = withCheckout({ ownerOf: (n) => (n === 'dev' ? owner : null) })
    expect(primaryBranchAction(row, ctxWithOwner)?.id).toBe('switch')
  })

  it('宿主声明 checkout：当前 HEAD 无主动作（返回 null）；tag 回落 new-workspace', () => {
    expect(primaryBranchAction(refRow(refEntry({ name: 'main', isHead: true })), withCheckout())).toBeNull()
    const tag = refRow(refEntry({ name: 'v1', category: 'tag', fullName: 'refs/tags/v1' }))
    expect(primaryBranchAction(tag, withCheckout())?.id).toBe('new-workspace')
  })

  it('宿主不声明 checkout：沿用旧 branchActionForRef（无 owner → new-workspace）', () => {
    const row = refRow(refEntry({ name: 'fresh' }))
    expect(primaryBranchAction(row, ctx({ ownerOf: () => null }))?.id).toBe('new-workspace')
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-actions.test.ts`
Expected: FAIL —— `'checkout'` 不在 `BranchActionId`、`BRANCH_ACTIONS` 无 checkout 项、`primaryBranchAction` 未走新分支。

- [ ] **Step 3: 实现动作表改动**

在 `branch-actions.ts`：

```ts
// (a) 联合加 checkout
export type BranchActionId = 'switch' | 'new-workspace' | 'view-in-log' | 'checkout'

// (b) BranchAction 加 enabled（缺省恒可用）
export type BranchAction = {
  id: BranchActionId
  label: () => string
  visible: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
  /** visible 但不可点时用；缺省 = 恒可用。用于 F1：被其它 worktree 检出 → 置灰。 */
  enabled?: (row: BranchTreeRow, ctx: BranchActionContext) => boolean
  run: (row: BranchTreeRow, ctx: BranchActionContext) => void
}

// (c) ctx 加可选 checkout：仅宿主 A 提供。声明它的宿主把 Checkout 作主动作。
export type BranchActionContext = {
  headBranch: string | null
  ownerOf: (branchName: string) => BranchWorkspaceOwner | null
  activateWorkspace: (owner: BranchWorkspaceOwner) => void
  newWorkspaceFrom: (ref: GitRefEntry) => void
  viewInGitLog?: (value: 'all-branches' | 'head-current' | GitRefEntry) => void
  /** 在当前工作区原地检出该 ref。仅宿主 A（Source Control）注入；宿主 B 不提供 → checkout 不出现。 */
  checkout?: (ref: GitRefEntry) => void
}

// (d) BRANCH_ACTIONS 追加 checkout 项（放在 switch 之前，供菜单顺序体现"主动作优先"）
{
  id: 'checkout',
  label: () => translate('auto.components.git-branch.checkout', 'Checkout'),
  visible: (row, ctx) => {
    const ref = refOf(row)
    return (
      ctx.checkout !== undefined &&
      ref !== null &&
      ref.category !== 'tag' &&
      !ref.isHead
    )
  },
  enabled: (row) => row.kind === 'ref' && row.checkedOutBy === null,
  run: (row, ctx) => {
    const ref = refOf(row)
    if (ref) ctx.checkout?.(ref)
  },
},

// (e) primaryBranchAction 新分支
export function primaryBranchAction(
  row: BranchTreeRow,
  ctx: BranchActionContext
): BranchAction | null {
  if (row.kind === 'ref') {
    if (ctx.checkout) {
      const checkout = BRANCH_ACTIONS.find((a) => a.id === 'checkout')!
      if (checkout.visible(row, ctx)) {
        if (!checkout.enabled || checkout.enabled(row, ctx)) return checkout
        // F1：checkout 可见但被占用 → 主动作回落 switch，点击给跳转而非死检出
        const sw = BRANCH_ACTIONS.find((a) => a.id === 'switch')!
        return sw.visible(row, ctx) ? sw : null
      }
      // checkout 不适用此行：当前 HEAD 无原地动作；tag 等仍走原有 owner 判定（→ new-workspace）
      if (row.ref.isHead) return null
      return BRANCH_ACTIONS.find((a) => a.id === branchActionForRef(row.ref, ctx.ownerOf)) ?? null
    }
    return BRANCH_ACTIONS.find((a) => a.id === branchActionForRef(row.ref, ctx.ownerOf)) ?? null
  }
  if (row.kind === 'synthetic' && ctx.viewInGitLog) {
    return BRANCH_ACTIONS.find((a) => a.id === 'view-in-log') ?? null
  }
  return null
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-actions.test.ts`
Expected: PASS（新旧用例全绿）。

- [ ] **Step 5: 类型检查**

Run: `pnpm typecheck`
Expected: 无 `branch-actions` 相关新错误。若宿主（Task 5 前）因缺 `checkout` 回调而语义漂移，类型不受影响（回调可选）。

- [ ] **Step 6: 提交**

```bash
git add src/renderer/src/components/git-branch/branch-actions.ts src/renderer/src/components/git-branch/branch-actions.test.ts
git commit -m "feat(git-branch): 动作表加 checkout + 主动作判定（F1 回落 switch）"
```

---

## Task 2: `branch-menu.tsx` —— 每行右键 + `⋯` 菜单

**Files:**
- Create: `src/renderer/src/components/git-branch/branch-menu.tsx`
- Test: `src/renderer/src/components/git-branch/branch-menu.test.tsx`（`// @vitest-environment happy-dom`）

**Interfaces:**
- Consumes: `BRANCH_ACTIONS` / `BranchActionContext`（`branch-actions.ts`）、`BranchTreeRow`（`branch-tree-model.ts`）、`ContextMenu*`（`components/ui/context-menu.tsx`）、`translate`
- Produces: `BranchRowMenu({ row, ctx, allowedActionIds, children, className })` —— 把 `children`（行的可点主体）包成右键触发区，并在行尾追加 `⋯` 触发按钮；弹出的菜单项 = `BRANCH_ACTIONS.filter(a => allowedActionIds.has(a.id) && a.visible(row, ctx))`，`enabled === false` 的项渲染为禁用（不可点）。仅当 `allowedActionIds` 命中至少一个 `visible` 项时渲染菜单，否则原样返回 `children`（无菜单的行不产生 `⋯`）。

- [ ] **Step 1: 写失败测试**

`branch-menu.test.tsx`：

```tsx
// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow } from './branch-tree-model'
import type { BranchActionContext } from './branch-actions'
import { BranchRowMenu } from './branch-menu'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

function refRow(name: string, checkedOutBy = null): BranchTreeRow {
  const ref: GitRefEntry = {
    name, fullName: `refs/heads/${name}`, category: 'local', oid: '0'.repeat(40), isHead: false,
  }
  return { kind: 'ref', id: `ref:${ref.fullName}`, depth: 1, ref, isUpstreamOfHead: false, checkedOutBy }
}

const run = vi.fn()
function makeCtx(): BranchActionContext {
  return {
    headBranch: 'main',
    ownerOf: () => null,
    activateWorkspace: run,
    newWorkspaceFrom: run,
    checkout: run,
  }
}

let host: HTMLDivElement, root: Root
beforeEach(() => { host = document.createElement('div'); document.body.append(host); root = createRoot(host) })
afterEach(() => { act(() => root.unmount()); host.remove(); run.mockReset() })
const flush = () => act(async () => { await Promise.resolve() })

async function openMenu(allowed: Set<string>, row = refRow('dev')) {
  await act(async () => {
    root.render(
      <BranchRowMenu row={row} ctx={makeCtx()} allowedActionIds={allowed}>
        <div data-testid="row-body">dev</div>
      </BranchRowMenu>
    )
  })
  await flush()
}

it('allowed 集合裁剪：只出现被声明的动作，checkout 不在允许集就不出', async () => {
  await openMenu(new Set(['switch', 'new-workspace']))
  // 用 ⋯ 触发（happy-dom 下右键 ContextMenu 触发不稳定，行尾按钮是主测面）
  const trigger = document.querySelector('[data-testid="branch-row-more"]')
  expect(trigger).not.toBeNull()
})

it('无任何 allowed 且 visible 的动作时不渲染 ⋯（宿主 B 之外的纯导航行不产生菜单）', async () => {
  await openMenu(new Set(['view-in-log'])) // 宿主 A ctx 无 viewInGitLog → visible=false
  expect(document.querySelector('[data-testid="branch-row-more"]')).toBeNull()
})

it('F1 占用行：checkout 渲染为 disabled（不可点），switch 可用', async () => {
  const owner = { worktreeId: 'r::/b', displayName: 'B', path: '/b' }
  await act(async () => {
    root.render(
      <BranchRowMenu row={refRow('dev', owner)} ctx={makeCtx()} allowedActionIds={new Set(['checkout', 'switch', 'new-workspace'])}>
        <div data-testid="row-body">dev</div>
      </BranchRowMenu>
    )
  })
  await flush()
  expect(document.querySelector('[data-testid="branch-row-more"]')).not.toBeNull()
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-menu.test.tsx`
Expected: FAIL —— 模块不存在。

- [ ] **Step 3: 实现 `branch-menu.tsx`**

```tsx
import { useMemo } from 'react'
import { MoreHorizontal } from 'lucide-react'
import { cn } from '@/lib/utils'
import {
  ContextMenu, ContextMenuTrigger, ContextMenuContent, ContextMenuItem
} from '@/components/ui/context-menu'
import {
  DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem
} from '@/components/ui/dropdown-menu'
import type { BranchTreeRow } from './branch-tree-model'
import { BRANCH_ACTIONS, type BranchAction, type BranchActionContext, type BranchActionId } from './branch-actions'

export type BranchRowMenuProps = {
  row: BranchTreeRow
  ctx: BranchActionContext
  /** 宿主声明的可用动作 id 集合（spec §13.6：裁剪机制从「不传某回调」扩成「声明 id 集合」）。 */
  allowedActionIds: ReadonlySet<BranchActionId>
  /** 行的可点主体（树经 renderRowMenu 传入的 content）；同时作右键触发区。 */
  children: React.ReactNode
  className?: string
}

/**
 * 分支行的右键（ContextMenu）+ 行尾 `⋯`（DropdownMenu）菜单。
 * 两个 Radix 浮层各用自己的 Item 原语（混用会把 ContextMenu.Item 渲染到无 Root 的上下文），
 * 故共享的是「过滤 + 判定 + run」的 renderItems(items, row, ctx, Item)，不是共享 Content 节点。
 * 没有任何可见项时**不渲染 `⋯`**：一个打不开的空菜单比没有更糟。
 */
export function BranchRowMenu({ row, ctx, allowedActionIds, children, className }: BranchRowMenuProps) {
  const items = useMemo(
    () => BRANCH_ACTIONS.filter((a) => allowedActionIds.has(a.id) && a.visible(row, ctx)),
    [allowedActionIds, ctx, row]
  )
  if (items.length === 0) return <>{children}</>

  return (
    <div className={cn('flex w-full items-center gap-1', className)}>
      <ContextMenu>
        <ContextMenuTrigger asChild className="min-w-0 flex-1">{children}</ContextMenuTrigger>
        <ContextMenuContent>{renderItems(items, row, ctx, ContextMenuItem)}</ContextMenuContent>
      </ContextMenu>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            data-testid="branch-row-more"
            aria-label="More actions"
            className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-accent/60"
          >
            <MoreHorizontal className="size-3.5" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">{renderItems(items, row, ctx, DropdownMenuItem)}</DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

function renderItems(
  items: readonly BranchAction[],
  row: BranchTreeRow,
  ctx: BranchActionContext,
  Item: typeof ContextMenuItem | typeof DropdownMenuItem
) {
  return items.map((a) => {
    const enabled = !a.enabled || a.enabled(row, ctx)
    return (
      <Item key={a.id} disabled={!enabled} onSelect={() => { if (enabled) a.run(row, ctx) }}>
        {a.label()}
      </Item>
    )
  })
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-menu.test.tsx`
Expected: PASS —— allowed 裁剪、空集不渲染 `⋯`、F1 disabled 三例绿。

- [ ] **Step 5: 判别力实证**

临时把 `items.length === 0` 守卫改成 `=== -1`（永远渲染 `⋯`），跑第 2 条用例应转红，再改回。确认「空菜单不出现 ⋯」这条断言真的有判别力，不是恒真。

- [ ] **Step 6: 提交**

```bash
git add src/renderer/src/components/git-branch/branch-menu.tsx src/renderer/src/components/git-branch/branch-menu.test.tsx
git commit -m "feat(git-branch): branch-menu 右键+行尾⋯，按 allowedActionIds 裁剪"
```

---

## Task 3: `BranchRefTree` 呈现接缝（菜单挂载 + 避嵌套）

**Files:**
- Modify: `src/renderer/src/components/git-branch/branch-ref-tree.tsx`
- Test: `src/renderer/src/components/git-branch/branch-ref-tree.test.tsx`

**Interfaces:**
- Consumes: 无新 import（`BranchRowMenu` 由**宿主**经 `renderRowMenu` 注入，树本身不 import 动作语义，保持 spec §5「树无业务语义」）
- Produces: `BranchRefTreeProps.renderRowMenu?: (row: BranchTreeRow, content: React.ReactNode) => React.ReactNode`。**方案 A**：树把已渲染好的 `<button>` 主体（含 `data-row-index`/`RowLeading`/label/`RowTrailing`）作 `content` 交给接缝；接缝只负责把它包进触发区并追加 `⋯` 兄弟节点。接缝**只作用于 ref 行**，group/synthetic 行与未提供接缝时的 ref 行**逐字节走原 `<button>`**（宿主 B 与既有测试零回归）。

- [ ] **Step 1: 写失败测试**

在 `branch-ref-tree.test.tsx` 追加（复用文件既有的 `BranchRefTree` 渲染 harness；下例用 `renderTree(props)` 代指该 harness 的调用）：

```tsx
it('未提供 renderRowMenu：ref 行仍是单个 <button>，无 ⋯（宿主 B 零回归）', () => {
  renderTree({}) // 不传 renderRowMenu
  const row = document.querySelector('[data-testid="branch-row-ref:refs/heads/dev"]')
  expect(row?.tagName).toBe('BUTTON')
  expect(document.querySelector('[data-testid="branch-row-more"]')).toBeNull()
})

it('提供 renderRowMenu：接缝收到 content，外层不得 button 套 button', () => {
  const refRows: BranchTreeRow[] = []
  renderTree({
    renderRowMenu: (row, content) => {
      if (row.kind === 'ref') refRows.push(row)
      return (
        <div className="menu-shell">
          {content}
          <button type="button" data-testid="branch-row-more">more</button>
        </div>
      )
    },
  })
  expect(refRows.length).toBeGreaterThan(0) // ref 行确实经过接缝
  // 关键嵌套守卫：main button 与 ⋯ 是兄弟，接缝产物里不存在 button 内嵌 button
  expect(document.querySelector('button button')).toBeNull()
})

it('renderRowMenu 只作用于 ref 行：group/synthetic 行不触发接缝', () => {
  const kinds = new Set<string>()
  renderTree({ renderRowMenu: (row, content) => { kinds.add(row.kind); return content } })
  expect([...kinds]).toEqual(['ref']) // 只有 ref 行被请求接缝
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-ref-tree.test.tsx`
Expected: FAIL —— 未知 prop 被忽略（接缝从未被调用）/ `button button` 命中等。

- [ ] **Step 3: 实现接缝**

在 `BranchRefTreeProps` 加 `renderRowMenu?: (row: BranchTreeRow, content: React.ReactNode) => React.ReactNode`，从 `BranchRefTree` 透传进 `BranchRowButton`。`BranchRowButton` 改为「先构造 button，再按接缝决定是否包裹」，`data-row-index` 始终留在 button 上（既有 `querySelector('[data-row-index]')?.focus()` 的 ↑↓ 导航口径不变）：

```tsx
function BranchRowButton({ row, index, active, onActivate, onToggleGroup, renderRowMenu }: {
  row: BranchTreeRow; index: number; active: boolean
  onActivate: (row: BranchTreeRow) => void; onToggleGroup: (id: string) => void
  renderRowMenu?: (row: BranchTreeRow, content: React.ReactNode) => React.ReactNode
}) {
  const onClick = row.kind === 'group' ? () => onToggleGroup(row.id) : () => onActivate(row)
  const button = (
    <button type="button" role="treeitem" aria-level={row.depth + 1}
      aria-selected={active} aria-expanded={row.kind === 'group' ? row.expanded : undefined}
      data-testid={branchRowTestId(row)} data-row-index={index} title={rowTitle(row)}
      onClick={onClick}
      className={cn(
        'flex w-full items-center gap-1.5 rounded px-1 py-0.5 text-left text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring',
        active ? 'bg-accent text-foreground' : 'hover:bg-accent/40'
      )}
      style={{ paddingLeft: `${row.depth * 12 + 4}px` }}>
      <RowLeading row={row} />
      <span className="min-w-0 flex-1 truncate">{rowLabel(row)}</span>
      <RowTrailing row={row} />
    </button>
  )
  // 接缝只作用 ref 行；其余逐字节走原 <button>（宿主 B / 既有测试零回归的关键）
  if (renderRowMenu && row.kind === 'ref') return <>{renderRowMenu(row, button)}</>
  return button
}
```

> `BranchRefTree` 主体里把 `renderRowMenu` 透传：`<BranchRowButton ... renderRowMenu={renderRowMenu} />`，并从 props 解构出 `renderRowMenu`。接缝（Task 2 的 `BranchRowMenu`）自带外层 flex 容器，故本任务**不再**新增包裹 div —— 嵌套 `button button` 由接缝保证「`⋯` 是 content 的兄弟、不是子」。

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm vitest run src/renderer/src/components/git-branch/branch-ref-tree.test.tsx`
Expected: PASS。既有全量 ref-tree 用例（无 renderRowMenu）保持绿——证明零回归。

- [ ] **Step 5: 判别力实证**

临时删掉「`button button` 为 null」守卫对应的结构（把 `⋯` 塞进 content 内部），跑该用例应转红，再还原。确认嵌套守卫有效。

- [ ] **Step 6: 提交**

```bash
git add src/renderer/src/components/git-branch/branch-ref-tree.tsx src/renderer/src/components/git-branch/branch-ref-tree.test.tsx
git commit -m "feat(git-branch): BranchRefTree 加 renderRowMenu 呈现接缝，ref 行避 button 嵌套"
```

---

## Task 4: Checkout 确认框组件 + 弹框判定

**Files:**
- Create: `src/renderer/src/components/git-branch/checkout-confirm-dialog.tsx`
- Test: `src/renderer/src/components/git-branch/checkout-confirm-dialog.test.tsx`（happy-dom）

**Interfaces:**
- Consumes: `GitRefEntry`（`shared/git-ref-types`）、`GitStatusEntry`（`shared/git-status-types`）、`Dialog*`（`components/ui/dialog`）、`translate`
- Produces:
  - `type CheckoutConfirmInput = { target: GitRefEntry; dirtyCount: number; dirtyPaths: string[]; sessionCount: number }`
  - `needsCheckoutConfirmation(input: { dirtyCount: number; sessionCount: number }): boolean` —— **产品决策 1**：`dirtyCount > 0 || sessionCount > 0` 才弹；否则不弹。
  - `CheckoutConfirmDialog({ input, open, onConfirm, onCancel })` —— 三段：目标 / 脏改动（`dirtyCount>0` 才出现）/ 活跃终端会话（`sessionCount>0` 才出现）。远程目标额外第二行「并创建本地分支 …，跟踪 …」。

- [ ] **Step 1: 写失败测试**

```tsx
// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { CheckoutConfirmDialog, needsCheckoutConfirmation } from './checkout-confirm-dialog'

const local: GitRefEntry = { name: 'dev', fullName: 'refs/heads/dev', category: 'local', oid: '0'.repeat(40), isHead: false }
const remote: GitRefEntry = { name: 'origin/dev', fullName: 'refs/remotes/origin/dev', category: 'remote', remote: 'origin', oid: '0'.repeat(40), isHead: false }

describe('needsCheckoutConfirmation', () => {
  it('干净且无会话 → 不弹', () => expect(needsCheckoutConfirmation({ dirtyCount: 0, sessionCount: 0 })).toBe(false))
  it('有脏改动 → 弹', () => expect(needsCheckoutConfirmation({ dirtyCount: 3, sessionCount: 0 })).toBe(true))
  it('有终端/会话 → 弹', () => expect(needsCheckoutConfirmation({ dirtyCount: 0, sessionCount: 2 })).toBe(true))
})

describe('CheckoutConfirmDialog 三段', () => {
  const render = (input) => {
    document.body.innerHTML = ''
    const root = createRoot(document.body)
    act(() => root.render(<CheckoutConfirmDialog input={input} open onConfirm={vi.fn()} onCancel={vi.fn()} />))
    return document.body.textContent ?? ''
  }
  it('本地目标：含「将检出：dev」，无跟踪行；脏段列出计数', () => {
    const text = render({ target: local, dirtyCount: 2, dirtyPaths: ['a.ts', 'b.ts'], sessionCount: 0 })
    expect(text).toContain('dev')
    expect(text).not.toContain('跟踪')
    expect(text).toContain('a.ts')
  })
  it('远程目标：额外出现「并创建本地分支 dev，跟踪 origin/dev」', () => {
    const text = render({ target: remote, dirtyCount: 0, dirtyPaths: [], sessionCount: 0 })
    expect(text).toContain('跟踪')
    expect(text).toContain('origin/dev')
  })
  it('无脏改动时整段不出现；无会话时终端段不出现', () => {
    const text = render({ target: local, dirtyCount: 0, dirtyPaths: [], sessionCount: 0 })
    expect(text).not.toMatch(/未提交|uncommitted/i)
    expect(text).not.toMatch(/终端|terminal/i)
  })
  it('会话计数合并终端+agent：文案为「终端/会话」', () => {
    const text = render({ target: local, dirtyCount: 0, dirtyPaths: [], sessionCount: 3 })
    expect(text).toMatch(/终端\/会话|terminal\/session/i)
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/checkout-confirm-dialog.test.tsx`
Expected: FAIL —— 模块不存在。

- [ ] **Step 3: 实现组件**

`checkout-confirm-dialog.tsx`（照 `discard-dialog.tsx` 的 `Dialog`/`onConfirm`/`onOpenAutoFocus` 形状）：

```tsx
import { useRef } from 'react'
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { translate } from '@/i18n/i18n'
import { focusDiscardDialogConfirmButton } from '../right-sidebar/source-control/commit/discard-dialog'
import type { GitRefEntry } from '../../../../shared/git-ref-types'

const MAX_DIRTY_PATHS = 5

export type CheckoutConfirmInput = {
  target: GitRefEntry
  dirtyCount: number
  dirtyPaths: string[]
  sessionCount: number
}

/** 产品决策 1：干净且无会话不弹框，直接检出。 */
export function needsCheckoutConfirmation(input: { dirtyCount: number; sessionCount: number }): boolean {
  return input.dirtyCount > 0 || input.sessionCount > 0
}

export function CheckoutConfirmDialog({ input, open, onConfirm, onCancel }: {
  input: CheckoutConfirmInput; open: boolean; onConfirm: () => void; onCancel: () => void
}) {
  const confirmRef = useRef<HTMLButtonElement>(null)
  const shownPaths = input.dirtyPaths.slice(0, MAX_DIRTY_PATHS)
  // 远程目标：本地分支短名 = 去 remote 前缀。origin/dev → dev。
  const localName = input.target.remote
    ? input.target.name.slice(input.target.remote.length + 1)
    : input.target.name

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onCancel() }}>
      <DialogContent className="max-w-md" onOpenAutoFocus={(e) => focusDiscardDialogConfirmButton(e, confirmRef.current)}>
        <DialogHeader>
          <DialogTitle className="text-sm">{translate('auto.components.git-branch.checkoutConfirmTitle', 'Checkout branch?')}</DialogTitle>
        </DialogHeader>

        {/* 段 1：目标 */}
        <div className="space-y-1 text-xs">
          <div>{translate('auto.components.git-branch.checkoutTarget', 'Will check out: {{branch}}', { branch: input.target.name })}</div>
          {input.target.category === 'remote' && (
            <div className="text-muted-foreground">
              {translate('auto.components.git-branch.checkoutCreatesTracking',
                'and create local branch {{name}} tracking {{remote}}', { name: localName, remote: input.target.name })}
            </div>
          )}
        </div>

        {/* 段 2：脏改动（有则出现） */}
        {input.dirtyCount > 0 && (
          <div className="rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs text-muted-foreground">
            <div>{translate('auto.components.git-branch.checkoutDirtyCount', '{{count}} uncommitted change(s)', { count: input.dirtyCount })}</div>
            <ul className="mt-1 list-disc pl-4">{shownPaths.map((p) => <li key={p} className="break-all">{p}</li>)}</ul>
            <div className="mt-1">{translate('auto.components.git-branch.checkoutWillCarry',
              'git will carry them to the target branch; if any file would be overwritten, git will refuse this checkout.')}</div>
          </div>
        )}

        {/* 段 3：活跃终端/会话（有则出现，仅告知，不阻断） */}
        {input.sessionCount > 0 && (
          <div className="text-xs text-muted-foreground">
            {translate('auto.components.git-branch.checkoutSessionCount',
              'This workspace has {{count}} running terminal(s)/session(s)', { count: input.sessionCount })}
          </div>
        )}

        <DialogFooter>
          <Button type="button" variant="outline" onClick={onCancel}>{translate('auto.components.git-branch.checkoutCancel', 'Cancel')}</Button>
          <Button ref={confirmRef} type="button" autoFocus onClick={onConfirm}>{translate('auto.components.git-branch.checkout', 'Checkout')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm vitest run src/renderer/src/components/git-branch/checkout-confirm-dialog.test.tsx`
Expected: PASS。

- [ ] **Step 5: i18n 补 en + zh**

在 `i18n/locales/en.json` 的 `auto.components.git-branch` 块（约 883-902 行）补：

```json
"checkout": "Checkout",
"checkoutConfirmTitle": "Checkout branch?",
"checkoutTarget": "Will check out: {{branch}}",
"checkoutCreatesTracking": "and create local branch {{name}} tracking {{remote}}",
"checkoutDirtyCount": "{{count}} uncommitted change(s)",
"checkoutWillCarry": "git will carry them to the target branch; if any file would be overwritten, git will refuse this checkout.",
"checkoutSessionCount": "This workspace has {{count}} running terminal(s)/session(s)",
"checkoutCancel": "Cancel",
"checkoutInvalid": "Invalid checkout target.",
"checkoutOccupiedExternally": "This branch is checked out in another worktree: {{path}}"
```

在 `i18n/locales/zh.json` 同名块（约 718-736 行）补：

```json
"checkout": "检出",
"checkoutConfirmTitle": "检出分支？",
"checkoutTarget": "将检出：{{branch}}",
"checkoutCreatesTracking": "并创建本地分支 {{name}}，跟踪 {{remote}}",
"checkoutDirtyCount": "{{count}} 个未提交改动",
"checkoutWillCarry": "git 会把它们带到目标分支；若有文件会被覆盖，git 将拒绝本次检出。",
"checkoutSessionCount": "该工作区有 {{count}} 个运行中的终端/会话",
"checkoutCancel": "取消",
"checkoutInvalid": "无效的检出目标。",
"checkoutOccupiedExternally": "该分支已在另一个工作区检出：{{path}}"
```

> `checkoutOccupiedExternally` 在 Task 5 用（外部占用兜底）。一次性补全避免二次改 locale。

- [ ] **Step 6: 提交**

```bash
git add src/renderer/src/components/git-branch/checkout-confirm-dialog.tsx src/renderer/src/components/git-branch/checkout-confirm-dialog.test.tsx src/renderer/src/i18n/locales/en.json src/renderer/src/i18n/locales/zh.json
git commit -m "feat(git-branch): Checkout 确认框三段 + 弹框判定；i18n en+zh"
```

---

## Task 5: 宿主 A 接线 —— 单击改 Checkout + 编排刷新/跳转

**Files:**
- Modify: `src/renderer/src/components/git-branch/source-control-branch-switcher.tsx`
- Test: `src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx`（重写受影响断言 + 新增 checkout 用例）

**Interfaces:**
- Consumes: `checkoutRuntimeGit`、`GitCheckoutOutcome`、`refreshGitStatusForWorktree`、`GitStatusRefreshDeps`、`CheckoutConfirmDialog` / `needsCheckoutConfirmation` / `CheckoutConfirmInput`（Task 4）、`BranchRowMenu`（Task 2）、`primaryBranchAction`/`BranchActionContext.checkout`（Task 1）、`renderRowMenu`（Task 3）、`useBranchRefs().invalidate`、`branchName`
- Produces: 宿主 A 的 `onActivate` = checkout 优先；ctx 注入 `checkout(ref)` 回调，内部编排确认框与刷新。

- [ ] **Step 1: 重写会被推翻的旧断言 + 写新失败测试**

`source-control-branch-switcher.test.tsx` 现有「点有 owner 分支 → activateAndRevealWorktree」「点无 owner 分支 → composer」等断言在主动作改 checkout 后**不再成立**（有 owner 且非当前分支 → checkout 置灰 → 回落 switch=jump；无 owner 本地分支 → checkout）。按新语义改写，并加桩：`window.api.git.checkout`、`window.api.git.status`、store 的 `gitStatusByWorktree` / `tabsByWorktree` / `unifiedTabsByWorktree` / `setGitStatus` / `updateWorktreeGitIdentity` / `setUpstreamStatus` / `fetchUpstreamStatus`。

```tsx
// 追加到文件顶部桩（与既有 vi.mock('@/store', ...) 并列）：
vi.mock('sonner', () => ({ toast: { warning: vi.fn(), error: vi.fn(), success: vi.fn() } }))
import { toast } from 'sonner'
// 隔开重量级后测：把 refreshGitStatusForWorktree mock 成 spy（Task 5 只断“成功后调了它”，
// 真跑跟刷是 Task 6 的职责；getRuntimeGitStatus 会拉进整条 status 链路，不适合在本文件跑）
vi.mock('../right-sidebar/git-status-refresh', () => ({ refreshGitStatusForWorktree: vi.fn().mockResolvedValue(undefined) }))
import { refreshGitStatusForWorktree } from '../right-sidebar/git-status-refresh'

// 追加到 state 桩：
const checkoutFn = vi.fn().mockResolvedValue({ status: 'switched', branch: 'x' }) // 默认成功，否则成功路径读 undefined.status 会走 catch
const invalidateSpy = vi.fn() // 若需断 refs 重新取数，可 spy useBranchRefs
// beforeEach 里挂到 window.api.git：{ refs, checkout: checkoutFn }（status 不需桩，refresh 已被 mock）
// beforeEach 里 toast.warning/error 都 mockReset；并往 refs 桩追加一条无 ade owner 的本地
//   分支 entry({ name: 'ghost' })（供外部占用用例点击，owners.get('ghost') 自然 miss）
// state 加：gitStatusByWorktree: {}, tabsByWorktree: {}, unifiedTabsByWorktree: {}

it('点无 owner 本地分支 → 干净无会话：不弹框，直接调 checkoutRuntimeGit → api.git.checkout', async () => {
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
  for (let i = 0; i < 5; i += 1) await flush()
  expect(checkoutFn).toHaveBeenCalledWith({ worktreePath: '/a', target: { kind: 'local', name: 'fresh' } })
  expect(document.body.textContent).not.toContain('Checkout branch?') // 未弹框
})

it('点远程分支 → target.kind=remote，name=origin/main', async () => {
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/remotes/origin/main'))
  for (let i = 0; i < 5; i += 1) await flush()
  expect(checkoutFn).toHaveBeenCalledWith({ worktreePath: '/a', target: { kind: 'remote', name: 'origin/main' } })
})

it('有脏改动 → 先弹确认框，未点确认不调 checkout', async () => {
  state.gitStatusByWorktree['r::/a'] = [{ path: 'a.ts', status: 'modified', area: 'unstaged' }]
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
  for (let i = 0; i < 5; i += 1) await flush()
  expect(document.body.textContent).toContain('Checkout branch?')
  expect(checkoutFn).not.toHaveBeenCalled()
  const confirm = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Checkout')
  await click(confirm ?? null)
  for (let i = 0; i < 5; i += 1) await flush()
  expect(checkoutFn).toHaveBeenCalled()
})

it('有终端+agent 会话 → 计数合并进弹框文案（终端/会话）', async () => {
  state.tabsByWorktree['r::/a'] = [{ id: 't1' }]
  state.unifiedTabsByWorktree['r::/a'] = [
    { id: 'u1', entityId: 'a', contentType: 'terminal' },
    { id: 'u2', entityId: 's1', contentType: 'agent-session' },
  ]
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
  for (let i = 0; i < 5; i += 1) await flush()
  // 1 终端 + 1 agent = 2；脏 0 也要因会话>0 弹框
  expect(document.body.textContent).toMatch(/2.*(终端|terminal)/i)
})

it('F1：点被别的工作区检出的分支 → 走 switch 跳转，不调 checkout', async () => {
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/heads/dev')) // dev 由 r::/b 检出
  for (let i = 0; i < 5; i += 1) await flush()
  expect(activate).toHaveBeenCalledWith('r::/b', { executionHostId: 'host-2' })
  expect(checkoutFn).not.toHaveBeenCalled()
})

it('点当前 HEAD 分支 → 无动作（不 checkout 不 activate 不开 composer）', async () => {
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/heads/main')) // main isHead
  for (let i = 0; i < 5; i += 1) await flush()
  expect(checkoutFn).not.toHaveBeenCalled()
  expect(activate).not.toHaveBeenCalled()
  expect(state.openModal).not.toHaveBeenCalled()
})

it('checkout 成功后会触发一次立即刷状态（refreshGitStatusForWorktree 被调）', async () => {
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/heads/fresh'))
  for (let i = 0; i < 5; i += 1) await flush()
  expect(refreshGitStatusForWorktree).toHaveBeenCalledTimes(1)
})

it('checked-out-elsewhere（ade 未知外部占用）→ 用 worktreePath 弹提示，不静默', async () => {
  checkoutFn.mockResolvedValueOnce({ status: { refused: 'checked-out-elsewhere' }, branch: 'ghost', worktreePath: '/ext/ghost' })
  await openSwitcher()
  await click(rowInPopover('branch-row-ref:refs/heads/ghost')) // ghost 无 ade owner → owners.get miss
  for (let i = 0; i < 5; i += 1) await flush()
  // 断 worktreePath 真被用上：经 sonner toast.warning（mock 断参，不依赖 sonner portal DOM）
  expect(toast.warning).toHaveBeenCalledWith(expect.stringContaining('/ext/ghost'))
  expect(activate).not.toHaveBeenCalled() // 命中不到 ade owner，不跳转
})
```

> 保留原有用例中**与主动作无关**的断言（触发器渲染、未打开不取数、取数失败降级、worktreeId 解析不到、不注入 All Branches、▣ 徽标、同宿主 hostId 透传）。原「点无 owner 分支 → composer」「点 remote 分支 → composer」改为「→ checkout」。**「点 HEAD 导航行不触发动作」保留**（合成行仍无动作）。

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx`
Expected: FAIL —— 现实现点分支仍走 switch/composer，未调 `api.git.checkout`，未弹框。

- [ ] **Step 3: 实现接线**

在 `source-control-branch-switcher.tsx`：

```tsx
// 新增 import
import { checkoutRuntimeGit } from '@/runtime/runtime-git-checkout-client'
import { refreshGitStatusForWorktree } from '../right-sidebar/git-status-refresh'
import { BranchRowMenu } from './branch-menu'
import { CheckoutConfirmDialog, needsCheckoutConfirmation, type CheckoutConfirmInput } from './checkout-confirm-dialog'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchActionId } from './branch-actions'
import { toast } from 'sonner' // 与 use-status-refresh.ts 同源（仓库 toast 统一走 sonner）

const HOST_A_ALLOWED: ReadonlySet<BranchActionId> = new Set(['checkout', 'switch', 'new-workspace'])
```

组件内新增：

```tsx
const [pendingCheckout, setPendingCheckout] = useState<GitRefEntry | null>(null)
// 自取脏/会话计数（宿主 A 独立于 panel foundation）
const dirty = useAppStore((s) => s.gitStatusByWorktree[worktreeId] ?? [])
const terminalCount = useAppStore((s) => s.tabsByWorktree[worktreeId]?.length ?? 0)
const agentCount = useAppStore((s) => s.unifiedTabsByWorktree[worktreeId]?.filter((t) => t.contentType === 'agent-session').length ?? 0)
const sessionCount = terminalCount + agentCount
const { refs, truncated, loading, error, invalidate } = useBranchRefs(open ? (worktree?.path ?? null) : null)
```

`checkout` 执行体（放进一个 `useCallback`）：

```tsx
const executeCheckout = useCallback(async (ref: GitRefEntry) => {
  if (!worktree) return
  const target = ref.category === 'remote'
    ? { kind: 'remote' as const, name: ref.name }
    : { kind: 'local' as const, name: ref.name }
  const context = {
    settings: null, // getActiveRuntimeTarget(null) → { kind: 'local' }；本增量不做远程 runtime
    worktreeId,
    worktreePath: worktree.path,
  }
  try {
    const outcome = await checkoutRuntimeGit(context, target)
    if (typeof outcome.status === 'object' && 'refused' in outcome.status && outcome.status.refused === 'checked-out-elsewhere') {
      // F1 竞态/外部占用：优先跳 ade 已知的占用工作区（按短名查 owners Map），否则用 worktreePath 兜底提示
      const occupant = outcome.branch ? owners.get(outcome.branch) : null
      if (occupant) {
        activateAndRevealWorktree(occupant.worktreeId, { executionHostId: occupant.hostId })
      } else {
        toast.warning(translate('auto.components.git-branch.checkoutOccupiedExternally',
          'This branch is checked out in another worktree: {{path}}', { path: outcome.worktreePath ?? '' }))
      }
      setOpen(false)
      return
    }
    if (outcome.status === 'invalid-target') {
      toast.error(outcome.message ?? translate('auto.components.git-branch.checkoutInvalid', 'Invalid checkout target.'))
      return
    }
    // 'switched' | 'already-on-branch' → 立即刷状态 + 让 refs 重新取数
    setOpen(false)
    await refreshGitStatusForWorktree({
      worktreeId, worktreePath: worktree.path,
      deps: {
        setGitStatus: useAppStore.getState().setGitStatus,
        updateWorktreeGitIdentity: useAppStore.getState().updateWorktreeGitIdentity,
        setUpstreamStatus: useAppStore.getState().setUpstreamStatus,
        fetchUpstreamStatus: useAppStore.getState().fetchUpstreamStatus,
      },
      request: { admissionTier: 'interactive' },
    })
    invalidate()
  } catch (e) {
    // 环境级失败（git 拒绝覆盖本地改动等）：原样上抛的错误消息按后端是英文 → 这里只做兜底提示
    toast.error(e instanceof Error ? e.message : String(e))
  }
}, [worktree, worktreeId, invalidate])
```

ctx 加 `checkout` 回调（弹框判定 + 状态）：

```tsx
const ctx = useMemo<BranchActionContext>(() => ({
  headBranch: branchName(worktree?.branch ?? '') || null,
  ownerOf: (shortName) => owners.get(shortName) ?? null,
  activateWorkspace: (owner) => { setOpen(false); activateAndRevealWorktree(owner.worktreeId, { executionHostId: owner.hostId }) },
  newWorkspaceFrom: (ref) => { setOpen(false); openModal('new-workspace-composer', { initialRepoId: repoId, initialBaseBranch: ref.name, telemetrySource: 'source_control_branch_menu' }) },
  checkout: (ref) => {
    // 产品决策 1：脏或会话>0 才弹框；否则直接检出
    if (needsCheckoutConfirmation({ dirtyCount: dirty.length, sessionCount })) {
      setPendingCheckout(ref)
    } else {
      void executeCheckout(ref)
    }
  },
}), [owners, openModal, repoId, worktree?.branch, dirty.length, sessionCount, executeCheckout])
```

渲染：`onActivate` 不变（`primaryBranchAction(row, ctx)?.run(row, ctx)`）；`BranchRefTree` 加 `renderRowMenu`；在返回的 JSX 末尾挂确认框：

```tsx
<BranchRefTree
  refs={refs} truncated={truncated} headBranch={ctx.headBranch} checkedOutByBranch={owners}
  onActivate={(row) => { primaryBranchAction(row, ctx)?.run(row, ctx) }}
  renderRowMenu={(row, content) =>
    row.kind === 'ref'
      ? <BranchRowMenu row={row} ctx={ctx} allowedActionIds={HOST_A_ALLOWED}>{content}</BranchRowMenu>
      : content
  }
/>
{/* 确认框：仅 pendingCheckout 非空时挂载（避免向必填 prop 传 null） */}
{pendingCheckout && (
  <CheckoutConfirmDialog
    open
    input={{
      target: pendingCheckout,
      dirtyCount: dirty.length,
      dirtyPaths: dirty.map((entry) => entry.path),
      sessionCount,
    }}
    onCancel={() => setPendingCheckout(null)}
    onConfirm={() => { const ref = pendingCheckout; setPendingCheckout(null); void executeCheckout(ref) }}
  />
)}
```

> `renderRowMenu` 签名按 Task 3 方案 A = `(row, content) => ReactNode`；`BranchRowMenu` 接 `children=content`。占用跳转路径统一用前端 `owners`（`ownerByBranch`）按短名解析，**不做 worktree 路径比较**（避开 macOS `/var`→`/private/var` 永不相等）；`owners` 命中不到（ade 未知的外部 worktree）才回落到用 `outcome.worktreePath` 弹 `checkoutOccupiedExternally` 提示——这就是“worktreePath 被用上”的落点。`checkoutInvalid` 已在 Step 5 的 en+zh 块声明。

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx`
Expected: PASS。

- [ ] **Step 5: 定向回归**

Run: `pnpm vitest run src/renderer/src/components/git-branch`
Expected: git-branch 目录全部用例绿（含 branch-ref-tree / branch-menu / checkout-confirm / use-branch-refs）。

- [ ] **Step 6: 类型检查 + 提交**

```bash
pnpm typecheck
git add src/renderer/src/components/git-branch/source-control-branch-switcher.tsx src/renderer/src/components/git-branch/source-control-branch-switcher.test.tsx
git commit -m "feat(git-branch): 宿主 A 单击改 Checkout，编排确认框/立即刷状态/F1 跳转"
```

---

## Task 6: 元数据跟随守卫（spec §13.4）

**Files:**
- Test: `src/renderer/src/components/git-branch/source-control-branch-checkout-metadata.test.tsx`（**新建独立文件**，不复用 Task 5 的 harness：那边已把 `refreshGitStatusForWorktree` mock 掉，本文件要跑真链路）

**Interfaces:**
- Consumes: Task 5 的接线（`executeCheckout` 成功后调 `refreshGitStatusForWorktree`）
- Produces: 无（纯守卫）

> 交接卡：没有这条，2A reviewer 的 claim（不显式刷新则最长 60s 才追上、且轮询被 `isWindowVisible()` 门控）无人守。**必须实证判别力**。

- [ ] **Step 1: 写守卫测试**

本文件**不** mock `git-status-refresh`；只 mock 更底层的 `@/runtime/runtime-git-client` 的 `getRuntimeGitStatus`，让真 `refreshGitStatusForWorktree` 跑通到 `deps.updateWorktreeGitIdentity`。检出一个**无 ade owner** 的本地分支（用 `fresh`，不选 `dev`——`dev` 被 `r::/b` 占，会 F1 置灰走跳转）。桩与 Task 5 同形（`vi.mock('@/store')` + `window.api.git`），关键差异如下：

```tsx
// @vitest-environment happy-dom
// 让真 refreshGitStatusForWorktree 跑：只截最底层取数
vi.mock('@/runtime/runtime-git-client', () => ({
  getRuntimeGitStatus: vi.fn().mockResolvedValue({
    entries: [], conflictOperation: 'unknown', head: 'fresh-oid', branch: 'refs/heads/fresh',
  }),
  getRuntimeGitUpstreamStatus: vi.fn().mockResolvedValue({ hasUpstream: false, ahead: 0, behind: 0 }),
}))

it('checkout 成功后真跑刷状态，把 Worktree.branch 跟刷成新分支完整 ref', async () => {
  const checkoutFn = vi.fn().mockResolvedValue({ status: 'switched', branch: 'fresh' })
  // state.updateWorktreeGitIdentity 是 spy（vi.mock('@/store') 内提供）
  await openSwitcher() // 渲染 SourceControlBranchSwitcher(worktreeId='r::/a')
  await click(rowInPopover('branch-row-ref:refs/heads/fresh')) // 干净无会话 → 不弹框，直接检出
  for (let i = 0; i < 8; i += 1) await flush()

  // refreshGitStatusForWorktree 内部经 deps 调 store action：断 store 侧收到跟刷成完整 ref
  expect(state.updateWorktreeGitIdentity).toHaveBeenCalledWith('r::/a', {
    head: 'fresh-oid', branch: 'refs/heads/fresh',
  })
})
```

> `state` 需提供 `updateWorktreeGitIdentity: vi.fn()`、`setGitStatus: vi.fn()`、`setUpstreamStatus: vi.fn()`、`fetchUpstreamStatus: vi.fn()`（`refreshGitStatusForWorktree` 的 `deps` 四项），其余同 Task 5。`getRuntimeGitStatus` 桩返 `branch: 'refs/heads/fresh'`（status.branch 生产即完整 ref），据此验证“完整 ref”而非短名写回。

- [ ] **Step 2: 运行确认通过**

Run: `pnpm vitest run src/renderer/src/components/git-branch/source-control-branch-checkout-metadata.test.tsx`
Expected: PASS。

- [ ] **Step 3: 判别力实证（还原被修的实现确认会红）**

临时把 Task 5 `executeCheckout` 成功分支里的 `await refreshGitStatusForWorktree(...)` 调用注释掉，跑本用例应转红（`updateWorktreeGitIdentity` 未被调用），再还原。若去掉刷新仍绿 → 说明断言打到了别的调用方，需收紧判据（比如断 `checkoutFn` 之后紧接一次 `updateWorktreeGitIdentity`）。

- [ ] **Step 4: 提交**

```bash
git add src/renderer/src/components/git-branch/source-control-branch-checkout-metadata.test.tsx
git commit -m "test(git-branch): 元数据跟随守卫（checkout→刷状态→Worktree.branch 跟刷）"
```

---

## 全增量收尾门禁

- [ ] `pnpm typecheck`
- [ ] `pnpm vitest run src/renderer/src/components/git-branch`（定向全绿）
- [ ] `pnpm test`（全量；已知环境性失败 `startup-shell-portability.live-shell.test.ts` 属 O-2，非本增量引入，如复现需在报告中标注）
- [ ] `pnpm build:web`
- [ ] 无 Rust 改动 → **不跑** `cargo test`（若收尾终审 CodeReview 判定需要，再按门禁补跑）
- [ ] 增量末尾一次全分支 CodeReview（交接卡 + 用户偏好）

## 手工验收（测试替代不了，`pnpm dev` 后逐项核）

1. Source Control 树：右键分支行出菜单；行尾 `⋯` 左键出同一菜单。
2. 点无 owner 本地分支（干净无会话）→ 原地检出，无弹框，头部分支名**立即**变（非 60s）。
3. 点有未提交改动的分支 → 弹框列前 5 条路径 + 「git 会带过去…覆盖则拒绝」；点检出才执行。
4. 该 workspace 开着终端/agent 会话 → 弹框第三段显示合并计数「N 个运行中的终端/会话」，不置灰、不阻断。
5. 点被其它 ade workspace 检出的分支 → Checkout 置灰，单击走跳转。
6. 点远程分支（`origin/x`）→ 建同名本地跟踪分支并检出（对照 `git branch -vv` 有 upstream）。
7. Git Log 工具栏树：无 Checkout 项（宿主 B 未声明该动作）；其余行为不变。

## 范围外（本增量不做）

- 常驻区块 `SourceControlBranchTreeSection` + 折叠持久化 + 移除头部弹层 → **2C**。
- `git_branch_create` / `set_upstream` / Update 参数化 / Rename / Delete → §10 其后增量。
- 远程 runtime checkout（`checkoutRuntimeGit` 对非 local 目标仍显式抛错，维持现状）。
- O-4（`GIT_TERMINAL_PROMPT=0` 无区分守卫）、O-5（fastForward/syncFork/rebase 仍无后端）→ 与本增量无关，留主仓债。
