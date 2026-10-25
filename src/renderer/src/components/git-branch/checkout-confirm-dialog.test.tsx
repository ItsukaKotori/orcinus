// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { CheckoutConfirmDialog, needsCheckoutConfirmation } from './checkout-confirm-dialog'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// Radix Dialog 在 happy-dom 下 portal + 焦点陷阱抖动；沿用仓库惯例（见
// DeleteWorktreeDialog.test 等）把 dialog 原语 mock 成内联渲染，只测本组件的三段装配。
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ children, open }: { children: React.ReactNode; open?: boolean }) =>
    open ? <div role="dialog">{children}</div> : null,
  DialogContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogHeader: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogTitle: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogDescription: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogFooter: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}))

const local: GitRefEntry = { name: 'dev', fullName: 'refs/heads/dev', category: 'local', oid: '0'.repeat(40), isHead: false }
const remote: GitRefEntry = {
  name: 'origin/dev',
  fullName: 'refs/remotes/origin/dev',
  category: 'remote',
  remote: 'origin',
  oid: '0'.repeat(40),
  isHead: false,
}

describe('needsCheckoutConfirmation（产品决策 1）', () => {
  it('干净且无会话 → 不弹', () => expect(needsCheckoutConfirmation({ dirtyCount: 0, sessionCount: 0 })).toBe(false))
  it('有脏改动 → 弹', () => expect(needsCheckoutConfirmation({ dirtyCount: 3, sessionCount: 0 })).toBe(true))
  it('有终端/会话 → 弹', () => expect(needsCheckoutConfirmation({ dirtyCount: 0, sessionCount: 2 })).toBe(true))
})

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

function renderDialog(input: Parameters<typeof CheckoutConfirmDialog>[0]['input']): string {
  act(() => {
    root.render(<CheckoutConfirmDialog input={input} open onConfirm={vi.fn()} onCancel={vi.fn()} />)
  })
  return document.body.textContent ?? ''
}

describe('CheckoutConfirmDialog 三段（spec §13.7）', () => {
  it('本地目标：含「Will check out: dev」，无跟踪行；脏段列出计数与路径', () => {
    const text = renderDialog({ target: local, dirtyCount: 2, dirtyPaths: ['a.ts', 'b.ts'], sessionCount: 0 })
    expect(text).toContain('Will check out: dev')
    expect(text).not.toContain('tracking')
    expect(text).toContain('2 uncommitted')
    expect(text).toContain('a.ts')
  })

  it('远程目标：额外出现「create local branch dev tracking origin/dev」', () => {
    const text = renderDialog({ target: remote, dirtyCount: 0, dirtyPaths: [], sessionCount: 0 })
    expect(text).toContain('create local branch dev tracking origin/dev')
    // 脏段与会话段都为空 → 不出现
    expect(text).not.toMatch(/uncommitted/i)
    expect(text).not.toMatch(/terminal/i)
  })

  it('会话计数合并：文案为 terminal(s)/session(s)', () => {
    const text = renderDialog({ target: local, dirtyCount: 0, dirtyPaths: [], sessionCount: 3 })
    expect(text).toMatch(/3 running terminal\(s\)\/session\(s\)/)
  })

  it('脏路径只列前 K（5）条', () => {
    const many = Array.from({ length: 8 }, (_, i) => `f${i}.ts`)
    const text = renderDialog({ target: local, dirtyCount: 8, dirtyPaths: many, sessionCount: 0 })
    expect(text).toContain('f4.ts')
    expect(text).not.toContain('f5.ts')
  })
})
