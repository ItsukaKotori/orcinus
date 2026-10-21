import { describe, expect, it, vi } from 'vitest'
import type { Tab } from '../../../../../shared/tab-types'
import { createTabsGitLogActions } from './tabs-git-log-actions'

type FakeTab = Pick<Tab, 'id' | 'groupId' | 'contentType' | 'gitLog'>

function harness() {
  const tabs: FakeTab[] = []
  const get = () =>
    ({
      unifiedTabsByWorktree: { w1: tabs },
      activeWorktreeId: 'w1',
      activateTab: vi.fn(),
      createUnifiedTab: (_worktreeId: string, contentType: string, init: Record<string, unknown>) => {
        const tab: FakeTab = {
          id: init.id as string,
          groupId: 'g1',
          contentType: contentType as Tab['contentType'],
          gitLog: init.gitLog as Tab['gitLog']
        }
        tabs.push(tab)
        return tab
      }
    }) as never
  const set = (_fn: unknown) => {}
  return { actions: createTabsGitLogActions(set as never, get as never), tabs }
}

describe('openGitLogTab', () => {
  it('creates one git-log tab per worktree and dedupes on reopen', () => {
    const { actions, tabs } = harness()
    const first = actions.openGitLogTab({ worktreeId: 'w1' })
    expect(first.contentType).toBe('git-log')
    expect(first.gitLog).toEqual({ worktreeId: 'w1' })
    const second = actions.openGitLogTab({ worktreeId: 'w1' })
    expect(second.id).toBe(first.id)
    expect(tabs).toHaveLength(1)
  })
})
