// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow, BranchWorkspaceOwner } from './branch-tree-model'
import type { BranchActionContext, BranchActionId } from './branch-actions'

// Radix Context/Dropdown 在 happy-dom 下 portal 抖动；沿用仓库惯例（见 checkout-confirm-dialog.test）
// mock 成内联渲染，好直接断「菜单项集合 = allowedActionIds ∧ visible」与「F1 项 disabled」——
// 这两条是 branch-menu.tsx 自己的装配线，纯谓词测试（branch-actions.test）覆盖不到。
vi.mock('@/components/ui/context-menu', () => ({
  ContextMenu: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ContextMenuContent: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="ctx-content">{children}</div>
  ),
  ContextMenuItem: ({
    children,
    disabled,
    onSelect,
  }: {
    children: React.ReactNode
    disabled?: boolean
    onSelect?: () => void
  }) => (
    <button
      type="button"
      data-testid="ctx-item"
      data-disabled={disabled ? 'true' : 'false'}
      onClick={onSelect}
    >
      {children}
    </button>
  ),
}))
vi.mock('@/components/ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuContent: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="dd-content">{children}</div>
  ),
  DropdownMenuItem: ({
    children,
    disabled,
    onSelect,
  }: {
    children: React.ReactNode
    disabled?: boolean
    onSelect?: () => void
  }) => (
    <button type="button" data-disabled={disabled ? 'true' : 'false'} onClick={onSelect}>
      {children}
    </button>
  ),
}))

import { BranchRowMenu } from './branch-menu'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

function refEntry(over: Partial<GitRefEntry> & { name: string }): GitRefEntry {
  return {
    fullName: `refs/heads/${over.name}`,
    category: 'local',
    oid: '0'.repeat(40),
    isHead: false,
    ...over,
  }
}

function refRow(ref: GitRefEntry, checkedOutBy: BranchWorkspaceOwner | null = null): BranchTreeRow {
  return { kind: 'ref', id: `ref:${ref.fullName}`, depth: 1, ref, isUpstreamOfHead: false, checkedOutBy }
}

const OWNER: BranchWorkspaceOwner = { worktreeId: 'r::/b', displayName: 'B', path: '/b' }

function makeCtx(over: Partial<BranchActionContext> = {}): BranchActionContext {
  return {
    headBranch: 'main',
    ownerOf: () => null,
    activateWorkspace: vi.fn(),
    newWorkspaceFrom: vi.fn(),
    viewInGitLog: vi.fn(),
    checkout: vi.fn(),
    ...over,
  }
}

let host: HTMLDivElement
let root: Root

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
})

const flush = async (): Promise<void> => {
  await act(async () => {
    await Promise.resolve()
  })
}

function render(row: BranchTreeRow, ctx: BranchActionContext, allowed: BranchActionId[]): void {
  act(() => {
    root.render(
      <BranchRowMenu row={row} ctx={ctx} allowedActionIds={new Set(allowed)}>
        <button type="button" data-testid="row-body">
          {row.kind === 'ref' ? row.ref.name : 'x'}
        </button>
      </BranchRowMenu>
    )
  })
}

const ctxItems = (): string[] =>
  [...document.querySelectorAll('[data-testid="ctx-item"]')].map((el) => el.textContent ?? '')
const hasMore = (): boolean => document.querySelector('[data-testid="branch-row-more"]') !== null

describe('BranchRowMenu', () => {
  it('菜单项 = allowedActionIds ∧ visible：allowed 里的 checkout/new-workspace 都出现', async () => {
    render(refRow(refEntry({ name: 'dev' })), makeCtx(), ['checkout', 'new-workspace'])
    await flush()
    expect(ctxItems().sort()).toEqual(['Checkout', 'New Workspace from Branch…'].sort())
    expect(hasMore()).toBe(true)
  })

  it('裁剪有牙：checkout 不在 allowed → 即便 ctx 声明了 checkout 也不进菜单', async () => {
    render(refRow(refEntry({ name: 'dev' })), makeCtx(), ['new-workspace'])
    await flush()
    expect(ctxItems()).toEqual(['New Workspace from Branch…'])
    expect(ctxItems()).not.toContain('Checkout')
  })

  it('F1 占用行：Checkout 项渲染为 disabled（置灰不隐藏），New Workspace 可用', async () => {
    const row = refRow(refEntry({ name: 'dev' }), OWNER)
    render(row, makeCtx(), ['checkout', 'new-workspace'])
    await flush()
    const items = [...document.querySelectorAll('[data-testid="ctx-item"]')]
    const checkoutItem = items.find((el) => el.textContent === 'Checkout')
    expect(checkoutItem?.getAttribute('data-disabled')).toBe('true')
    const nwItem = items.find((el) => el.textContent === 'New Workspace from Branch…')
    expect(nwItem?.getAttribute('data-disabled')).toBe('false')
  })

  it('无可见项时不渲染 ⋯（空菜单比没有更糟），children 原样返回', async () => {
    render(refRow(refEntry({ name: 'main', isHead: true })), makeCtx(), ['checkout'])
    await flush()
    expect(hasMore()).toBe(false)
    expect(document.querySelector('[data-testid="ctx-content"]')).toBeNull()
    expect(document.querySelector('[data-testid="row-body"]')).not.toBeNull()
  })

  it('ctx 未声明 checkout → checkout.visible=false，allowed 仅 checkout 时菜单为空不渲染 ⋯', async () => {
    render(refRow(refEntry({ name: 'dev' })), makeCtx({ checkout: undefined }), ['checkout'])
    await flush()
    expect(hasMore()).toBe(false)
  })
})
