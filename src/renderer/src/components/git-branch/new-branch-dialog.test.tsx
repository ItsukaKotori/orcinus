// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { NewBranchDialog } from './new-branch-dialog'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

// 沿用仓库惯例：mock Radix Dialog 原语为内联渲染，测本组件的输入/校验/回调。
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ children, open }: { children: React.ReactNode; open?: boolean }) =>
    open ? <div role="dialog">{children}</div> : null,
  DialogContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogHeader: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogTitle: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogDescription: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogFooter: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}))

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
const flush = () => act(async () => { await Promise.resolve() })

const createBtn = (): HTMLButtonElement | null =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Create branch') ?? null

function render(props: { startPointLabel?: string; onCreate: (n: string) => void; onCancel: () => void }) {
  act(() => {
    root.render(
      <NewBranchDialog
        open
        startPointLabel={props.startPointLabel ?? 'main'}
        onCreate={props.onCreate}
        onCancel={props.onCancel}
      />
    )
  })
}

async function type(value: string): Promise<void> {
  const input = document.querySelector('[data-testid="new-branch-name-input"]') as HTMLInputElement
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
  await act(async () => {
    setter?.call(input, value)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  await flush()
}

describe('NewBranchDialog', () => {
  it('空名禁用创建按钮；显示 start-point 提示', async () => {
    const onCreate = vi.fn()
    render({ startPointLabel: 'origin/dev', onCreate, onCancel: vi.fn() })
    await flush()
    expect(document.body.textContent).toContain('origin/dev')
    expect(createBtn()?.disabled).toBe(true)
  })

  it('键入合法名 → 启用并可提交（onCreate 收到 trim 后的值）', async () => {
    const onCreate = vi.fn()
    render({ onCreate, onCancel: vi.fn() })
    await type('  feature/x  ')
    const btn = createBtn()
    expect(btn?.disabled).toBe(false)
    await act(async () => { btn?.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    await flush()
    expect(onCreate).toHaveBeenCalledWith('feature/x')
  })

  it('只有空格 → 仍禁用（trim 后为空）', async () => {
    const onCreate = vi.fn()
    render({ onCreate, onCancel: vi.fn() })
    await type('   ')
    expect(createBtn()?.disabled).toBe(true)
  })

  it('取消回调', async () => {
    const onCancel = vi.fn()
    render({ onCreate: vi.fn(), onCancel })
    await flush()
    const cancel = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Cancel')
    await act(async () => { cancel?.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(onCancel).toHaveBeenCalled()
  })
})
