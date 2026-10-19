// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { GitLogView } from './GitLogView'
import { CommitDetail } from './CommitDetail'
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
