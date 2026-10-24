import { describe, expect, it, vi } from 'vitest'
import type { Tab } from '../../../../../shared/tab-types'
import { createTestStore, seedStore } from '../store-test-helpers'
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

describe('git-log tab close / recently closed', () => {
  const WT = 'repo1::/workspace'

  it('records the git-log kind on close and reopen skips it instead of misrouting to the editor stack', () => {
    const store = createTestStore()
    seedStore(store, { activeWorktreeId: WT })
    // Seed the editor reopen stack so misrouting would visibly reopen the file.
    store.getState().createUnifiedTab(WT, 'editor', {
      id: 'editor-1',
      entityId: 'editor-1',
      label: 'file.ts'
    })
    store.getState().closeUnifiedTab('editor-1')
    const logTab = store.getState().openGitLogTab({ worktreeId: WT })

    store.getState().closeUnifiedTab(logTab.id)

    expect(store.getState().recentlyClosedTabKindsByWorktree[WT]).toEqual(['git-log'])
    // The git-log kind must be drained, not served from the editor stack.
    expect(store.getState().reopenClosedTab(WT)).toBe(false)
    expect(store.getState().recentlyClosedTabKindsByWorktree[WT]).toEqual([])
  })
})

describe('git-log tab reconciliation', () => {
  const WT = 'repo1::/workspace'

  it('keeps the git-log tab through reconcileWorktreeTabModel', () => {
    // Why: without a dedicated branch the tab falls into the editor-liveness
    // fallback and reconcile silently closes every Git Log tab right after open.
    const store = createTestStore()
    seedStore(store, { activeWorktreeId: WT })
    const tab = store.getState().openGitLogTab({ worktreeId: WT })

    store.getState().reconcileWorktreeTabModel(WT)

    expect(store.getState().getTab(tab.id)).not.toBeNull()
    expect(store.getState().unifiedTabsByWorktree[WT]).toHaveLength(1)
    expect(store.getState().groupsByWorktree[WT]?.[0]?.tabOrder).toEqual([tab.id])
  })
})
