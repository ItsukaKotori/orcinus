// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { GitLogView } from './GitLogView'
import { CommitDetail } from './CommitDetail'
import { HostCallError } from './host-bridge'
import type { GitHistoryItem, GitHistoryResult } from '@shared/git-history-types'
import type { GitLogRepository } from '../domain/types'
import type { GitLogFilter } from '../domain/query'
import { FixtureRepository } from '../fixtures/fixture-repository'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

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

const flush = async () => { await act(async () => { await Promise.resolve() }) }

// 不依赖 @tanstack/react-virtual 的行测量（happy-dom 无布局）：只断言始终渲染在
// 虚拟列表之外的头部/分支；过滤与选中行为由 node 环境的 select-rows.test.ts 覆盖。
describe('GitLogView', () => {
  it('挂载后显示标题与当前分支', async () => {
    await act(async () => { root.render(<GitLogView repository={new FixtureRepository()} />) })
    await flush()
    expect(host.textContent).toContain('Git Log')
    expect(host.textContent).toContain('main')
  })

  it('渲染后端过滤控件与范围切换（Current / All + chips）', async () => {
    await act(async () => { root.render(<GitLogView repository={new FixtureRepository()} />) })
    await flush()
    expect(host.querySelector('[data-testid="filter-author"]')).toBeTruthy()
    expect(host.querySelector('[data-testid="filter-path"]')).toBeTruthy()
    expect(host.textContent).toContain('Current')
    expect(host.textContent).toContain('All')
  })

  it('挂载首屏只请求一次（debounce 不误触第二次 reload）', async () => {
    const empty: GitHistoryResult = {
      items: [],
      hasIncomingChanges: false,
      hasOutgoingChanges: false,
      hasMore: false,
      limit: 100,
    }
    const calls: { filter: GitLogFilter; skip: number }[] = []
    const repo: GitLogRepository = {
      loadSnapshot: (filter = {}, skip = 0) => {
        calls.push({ filter, skip })
        return Promise.resolve(empty)
      },
      loadRefs: () => Promise.resolve([]),
      loadCommitFiles: () => Promise.resolve([]),
    }
    await act(async () => { root.render(<GitLogView repository={repo} />) })
    // Cross the 300 ms debounce window: an equal filter must NOT re-trigger reload.
    await act(async () => { await new Promise((r) => setTimeout(r, 400)) })
    expect(calls).toHaveLength(1)
    expect(calls[0]?.skip).toBe(0)
  })

  const twoItems = (skip: number): GitHistoryItem[] => [
    { id: `c${skip}a`, parentIds: [], subject: `s${skip}a`, message: 'm' },
    { id: `c${skip}b`, parentIds: [], subject: `s${skip}b`, message: 'm' },
  ]
  const pageOf = (skip: number): GitHistoryResult => ({
    items: twoItems(skip),
    hasIncomingChanges: false,
    hasOutgoingChanges: false,
    hasMore: true,
    limit: 2,
  })

  it('加载更多按已消费条数累加 offset（page3+ 不回退到 page2）', async () => {
    const skips: number[] = []
    const repo: GitLogRepository = {
      loadSnapshot: (_f = {}, skip = 0) => {
        skips.push(skip)
        return Promise.resolve(pageOf(skip))
      },
      loadRefs: () => Promise.resolve([]),
      loadCommitFiles: () => Promise.resolve([]),
    }
    await act(async () => { root.render(<GitLogView repository={repo} />) })
    await flush()
    const clickMore = async () => {
      const btn = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Load more')
      await act(async () => {
        btn?.click()
        await Promise.resolve()
      })
    }
    await clickMore()
    await clickMore()
    // First page consumed 2 → next offset 2; second → 4. (Old `setSkip(next.limit)` would repeat 2.)
    expect(skips).toEqual([0, 2, 4])
  })

  it('再查失败（如 unknown-oid）时保留列表并内联分类文案', async () => {
    let n = 0
    const repo: GitLogRepository = {
      loadSnapshot: () =>
        ++n === 1
          ? Promise.resolve(pageOf(0))
          : Promise.reject(new HostCallError('unknown-oid', 'unknown-oid: x')),
      loadRefs: () => Promise.resolve([]),
      loadCommitFiles: () => Promise.resolve([]),
    }
    await act(async () => { root.render(<GitLogView repository={repo} />) })
    await flush()
    // Trigger a re-query via Refresh; it now rejects.
    const refresh = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Refresh')
    await act(async () => {
      refresh?.click()
      await Promise.resolve()
    })
    // List region (RefPanel Current/All) survives; no full-screen error; friendly copy shown.
    expect(host.textContent).toContain('Current')
    expect(host.textContent).not.toContain('Git history unavailable')
    expect(host.textContent).toContain('That branch or revision does not exist.')
  })

  it('reload 在飞时点击加载更多不发陈旧请求', async () => {
    const calls: number[] = []
    let resolvePending: ((v: GitHistoryResult) => void) | null = null
    let n = 0
    const repo: GitLogRepository = {
      loadSnapshot: (_f = {}, skip = 0) => {
        n++
        calls.push(skip)
        if (n === 1) return Promise.resolve(pageOf(0))
        return new Promise<GitHistoryResult>((res) => {
          resolvePending = res
        })
      },
      loadRefs: () => Promise.resolve([]),
      loadCommitFiles: () => Promise.resolve([]),
    }
    await act(async () => { root.render(<GitLogView repository={repo} />) })
    await flush()
    // Start a re-query (Refresh) and hold its response pending.
    const refresh = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Refresh')
    await act(async () => { refresh?.click() })
    // While that reload is in flight, a "Load more" click must be gated (no stale page request).
    const more = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Load more')
    await act(async () => { more?.click() })
    expect(calls).toEqual([0, 0]) // mount + refresh only; loadMore blocked by reloadInFlight
    await act(async () => {
      resolvePending?.(pageOf(0))
      await Promise.resolve()
    })
  })
})

describe('CommitDetail', () => {
  it('渲染选中提交的 subject 与文件列表', async () => {
    const item: GitHistoryItem = {
      id: 'a'.repeat(40), parentIds: [], subject: 'Merge branch feature into main', message: 'body',
      author: 'Ada', authorEmail: 'ada@example.com', timestamp: Date.UTC(2026, 9, 18), statistics: { files: 2, insertions: 10, deletions: 3 }
    }
    const files = [{ path: 'src/core/engine.ts', status: 'modified' as const, added: 1, removed: 1 }]
    await act(async () => { root.render(<CommitDetail item={item} files={files} />) })
    expect(host.querySelector('[data-testid="commit-detail"]')?.textContent).toContain('Merge branch feature into main')
    expect(host.textContent).toContain('src/core/engine.ts')
  })
})
