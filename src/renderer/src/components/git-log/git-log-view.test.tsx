// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { GitHistoryResult } from '../../../../shared/git-history-types'
import { GitLogView } from './GitLogView'
import { RefPanel } from './RefPanel'
import type { GitLogFilter } from './query'
import type { GitLogRepository, GitRefs } from './types'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const snapshot = (ids: string[], hasMore: boolean): GitHistoryResult => ({
  items: ids.map((id) => ({ id, parentIds: [], subject: id, message: id })),
  hasIncomingChanges: false,
  hasOutgoingChanges: false,
  hasMore,
  limit: 100,
  currentRef: { id: 'refs/heads/main', name: 'main', category: 'branches' },
})

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
      const result: GitRefs = {
        refs: [{ id: 'refs/heads/feature', name: 'feature', category: 'branches' }],
        truncated: false,
      }
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
    expect(host.textContent).toContain('main')
    expect(host.textContent).toContain('feature') // from loadRefs → RefPanel
  })

  it('picking All range re-queries with refs=all after the debounce', async () => {
    const { repo, calls } = fakeRepo()
    await act(async () => {
      root.render(<GitLogView repository={repo} />)
    })
    await flush()
    const all = Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'All')
    await act(async () => {
      all?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    // Cross the 300ms debounce window.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 400))
    })
    const last = calls[calls.length - 1]
    expect(last?.filter).toEqual({ refs: 'all' })
  })

  it('RefPanel search filters the branch list by name', async () => {
    await act(async () => {
      root.render(
        <RefPanel
          refs={[
            { id: 'refs/heads/main', name: 'main', category: 'branches' },
            { id: 'refs/heads/release', name: 'release', category: 'branches' },
          ]}
          truncated={false}
          activeName={null}
          onPickName={() => {}}
        />
      )
    })
    expect(host.textContent).toContain('release')
    const input = host.querySelector('input')!
    const valueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
    await act(async () => {
      valueSetter?.call(input, 'main')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(host.textContent).toContain('main')
    expect(host.textContent).not.toContain('release')
  })
})
