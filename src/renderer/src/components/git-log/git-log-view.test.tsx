// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { GitHistoryResult } from '../../../../shared/git-history-types'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { GitLogView } from './GitLogView'
import type { GitLogFilter } from './query'
import type { GitLogRepository, GitRefs } from './types'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const snapshot = (ids: string[], hasMore: boolean): GitHistoryResult => ({
  items: ids.map((id) => ({ id, parentIds: [], subject: id, message: id })),
  hasIncomingChanges: false,
  hasOutgoingChanges: false,
  hasMore,
  limit: 100,
  // currentRef 是 commit decoration（GitHistoryItemRef），**没有**随 git_refs 换型。
  currentRef: { id: 'refs/heads/main', name: 'main', category: 'branches' },
})

const FEATURE: GitRefEntry = {
  name: 'feature',
  fullName: 'refs/heads/feature',
  category: 'local',
  oid: '0'.repeat(40),
  isHead: false,
}

function fakeRepo() {
  const calls: { filter: GitLogFilter; skip: number }[] = []
  let refsCalls = 0
  const repo: GitLogRepository = {
    loadSnapshot: (filter = {}, skip = 0) => {
      calls.push({ filter, skip })
      return Promise.resolve(snapshot([`c${skip}a`, `c${skip}b`], true))
    },
    loadRefs: () => {
      refsCalls += 1
      const result: GitRefs = { refs: [FEATURE], truncated: false }
      return Promise.resolve(result)
    },
    loadCommitFiles: () => Promise.resolve([]),
  }
  return { repo, calls, refsCalls: () => refsCalls }
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
/**
 * Radix Popover 把内容 portal 到 `document.body`，不在挂载容器子树里
 * （仓库既有的 `ui/popover-wheel-scroll.test.tsx` 靠显式 `portalContainer` 绕开这点）。
 * 所以弹层内的元素必须从 `document` 查，从 `host` 查永远是 null。
 */
const click = async (el: Element | null): Promise<void> => {
  await act(async () => {
    el?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
}
const picker = (): HTMLElement | null => host.querySelector('[data-testid="branch-picker"]')

/** 取弹层里必存在的元素。缺了就直接失败，而不是让后续点击空转。 */
function mustFindInPopover(selector: string): HTMLElement {
  const el = document.querySelector(selector)
  if (!el) throw new Error(`expected popover element for selector ${selector}`)
  return el as HTMLElement
}

/** 打开分支弹层，并断言它真的开了（否则后面的点击全是空转）。 */
async function openPicker(): Promise<void> {
  await click(picker())
  await flush()
  mustFindInPopover('[data-testid="branch-tree"]')
}

const pickRow = async (testId: string): Promise<void> => {
  await click(mustFindInPopover(`[data-testid="${testId}"]`))
  await flush()
}

describe('GitLogView (native)', () => {
  it('mounts: loads page 0 once, loads refs, shows branch + commit rows', async () => {
    const { repo, calls, refsCalls } = fakeRepo()
    await act(async () => {
      root.render(<GitLogView repository={repo} />)
    })
    await flush()
    expect(calls).toHaveLength(1)
    expect(calls[0]).toEqual({ filter: {}, skip: 0 })
    expect(refsCalls()).toBe(1)
    expect(host.textContent).toContain('Git Log')
    // 分支名现在住在工具栏的 picker 触发器里（不再是静态 chip + 左栏列表）。
    expect(picker()?.textContent).toContain('main')
    expect(host.textContent).toContain('c0a')
    // refs 只在弹层打开后才渲染 —— 左栏已被删除。
    expect(document.querySelector('[data-testid="branch-row-ref:refs/heads/feature"]')).toBeNull()
  })

  it('picking All range re-queries with refs=all after the debounce', async () => {
    const { repo, calls } = fakeRepo()
    await act(async () => {
      root.render(<GitLogView repository={repo} />)
    })
    await flush()
    await openPicker()
    await pickRow('branch-row-row:all-branches')
    // Cross the 300ms debounce window.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 400))
    })
    const last = calls[calls.length - 1]
    expect(last?.filter).toEqual({ refs: 'all' })
  })

  it('picking a branch from the popover re-queries with that ref name', async () => {
    const { repo, calls } = fakeRepo()
    await act(async () => {
      root.render(<GitLogView repository={repo} />)
    })
    await flush()
    await openPicker()
    await pickRow('branch-row-ref:refs/heads/feature')
    await act(async () => {
      await new Promise((r) => setTimeout(r, 400))
    })
    expect(calls[calls.length - 1]?.filter).toEqual({ refs: 'feature' })
  })

  it('picking HEAD (Current Branch) re-queries with the default (HEAD) range', async () => {
    const { repo, calls } = fakeRepo()
    await act(async () => {
      root.render(<GitLogView repository={repo} />)
    })
    await flush()
    await openPicker()
    await pickRow('branch-row-ref:refs/heads/feature')
    await act(async () => {
      await new Promise((r) => setTimeout(r, 400))
    })
    await openPicker()
    await pickRow('branch-row-row:head-current')
    await act(async () => {
      await new Promise((r) => setTimeout(r, 400))
    })
    expect(calls[calls.length - 1]?.filter).toEqual({})
  })

  it('弹层打开时高亮当前查看范围对应的行', async () => {
    const { repo } = fakeRepo()
    await act(async () => {
      root.render(<GitLogView repository={repo} />)
    })
    await flush()
    await openPicker()
    // 初始 filter.refs 为 undefined → HEAD (Current Branch) 是高亮项。
    expect(
      mustFindInPopover('[data-testid="branch-row-row:head-current"]').getAttribute('aria-selected')
    ).toBe('true')
  })

  it('左栏已删除：不打开弹层时页面里没有分支树', async () => {
    const { repo } = fakeRepo()
    await act(async () => {
      root.render(<GitLogView repository={repo} />)
    })
    await flush()
    expect(host.querySelector('[data-testid="branch-tree"]')).toBeNull()
    expect(document.querySelector('[data-testid="branch-tree"]')).toBeNull()
    // 反证：同一棵 tree 在弹层打开后必须出现 —— 否则上一条断言只是因为
    // “根本开不了”而恒真。
    await openPicker()
    expect(document.querySelector('[data-testid="branch-tree"]')).not.toBeNull()
  })
})
