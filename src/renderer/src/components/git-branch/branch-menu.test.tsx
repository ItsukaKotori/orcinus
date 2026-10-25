// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import type { BranchTreeRow, BranchWorkspaceOwner } from './branch-tree-model'
import type { BranchActionContext, BranchActionId } from './branch-actions'
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

const hasMore = (): boolean =>
  document.querySelector('[data-testid="branch-row-more"]') !== null

function render(
  row: BranchTreeRow,
  ctx: BranchActionContext,
  allowed: BranchActionId[]
): void {
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

describe('BranchRowMenu', () => {
  it('allowed 命中可见动作时渲染行尾 ⋯，且保留行主体', async () => {
    render(refRow(refEntry({ name: 'dev' })), makeCtx(), ['checkout', 'switch', 'new-workspace'])
    await flush()
    expect(hasMore()).toBe(true)
    expect(document.querySelector('[data-testid="row-body"]')).not.toBeNull()
  })

  it('allowed 为空集 → 无可见项 → 不渲染 ⋯', async () => {
    render(refRow(refEntry({ name: 'dev' })), makeCtx(), [])
    await flush()
    expect(hasMore()).toBe(false)
    // 无菜单时原样返回 children：行主体仍在，且不出现菜单外层
    expect(document.querySelector('[data-testid="row-body"]')).not.toBeNull()
  })

  it('当前 HEAD 行只允许 checkout（visible=false）→ 无可见项 → 不渲染 ⋯', async () => {
    render(refRow(refEntry({ name: 'main', isHead: true })), makeCtx(), ['checkout'])
    await flush()
    expect(hasMore()).toBe(false)
  })

  it('裁剪由 allowed 决定：只允许 checkout 但 ctx 未声明 checkout → 不可见 → 不渲染 ⋯', async () => {
    const ctx = makeCtx({ checkout: undefined })
    render(refRow(refEntry({ name: 'dev' })), ctx, ['checkout'])
    await flush()
    expect(hasMore()).toBe(false)
  })

  it('F1 占用行：checkout 仍 visible（只是 disabled），有可见项 → 渲染 ⋯', async () => {
    render(refRow(refEntry({ name: 'dev' }), OWNER), makeCtx(), ['checkout'])
    await flush()
    expect(hasMore()).toBe(true)
  })
})
