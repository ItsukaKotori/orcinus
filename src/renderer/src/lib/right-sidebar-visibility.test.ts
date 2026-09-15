import { describe, expect, it } from 'vitest'
import {
  canShowRightSidebarForView,
  rightSidebarShowsPullRequestData
} from './right-sidebar-visibility'
import type { AppState } from '@/store/types'

function makeState(
  overrides: Partial<Parameters<typeof rightSidebarShowsPullRequestData>[0]> = {}
): Parameters<typeof rightSidebarShowsPullRequestData>[0] {
  return {
    activeView: 'terminal',
    activeWorktreeId: 'wt-1',
    repos: [
      {
        id: 'repo-1',
        path: '/repo',
        displayName: 'Repo',
        badgeColor: '#000000',
        addedAt: 1,
        kind: 'git'
      }
    ],
    rightSidebarOpen: true,
    rightSidebarTab: 'checks',
    worktreesByRepo: { 'repo-1': [{ id: 'wt-1', repoId: 'repo-1' }] },
    ...overrides
  } as Parameters<typeof rightSidebarShowsPullRequestData>[0]
}

describe('right sidebar visibility helpers', () => {
  it('suppresses right sidebar controls on full-page views', () => {
    for (const view of [
      'settings',
      'tasks',
      'activity',
      'automations',
      'space',
      'artifacts',
      'plugin-center'
    ]) {
      expect(canShowRightSidebarForView(view as AppState['activeView'])).toBe(false)
    }
  })

  it('suppresses right sidebar controls on plugin-hosted views', () => {
    for (const view of [
      'plugin:database-manager',
      'plugin:database-manager.main',
      'plugin:x'
    ]) {
      expect(canShowRightSidebarForView(view as AppState['activeView'])).toBe(false)
    }
  })

  it('allows right sidebar controls on workspace views', () => {
    expect(canShowRightSidebarForView('terminal')).toBe(true)
  })

  it('does not treat hidden full-page sidebars as visible PR panels', () => {
    expect(rightSidebarShowsPullRequestData(makeState({ activeView: 'tasks' }))).toBe(false)
  })

  it('does not treat hidden folder-repo fallbacks as visible PR panels', () => {
    expect(
      rightSidebarShowsPullRequestData(
        makeState({
          repos: [
            {
              id: 'repo-1',
              path: '/repo',
              displayName: 'Repo',
              badgeColor: '#000000',
              addedAt: 1,
              kind: 'folder'
            }
          ]
        })
      )
    ).toBe(false)
  })

  it('detects visible PR panels in workspace views', () => {
    expect(
      rightSidebarShowsPullRequestData(
        makeState({
          rightSidebarTab: 'source-control'
        })
      )
    ).toBe(true)
  })

  it('tolerates partial store state without activeView', () => {
    // Why: github slice tests build partial stores that omit activeView; the fork's original
    // implementation used only Set.has(activeView) and never dereferenced it.
    // SAFETY: mirrors those partial stores, whose activeView is undefined at runtime.
    const partialState = {
      activeWorktreeId: 'wt-1',
      repos: [],
      rightSidebarOpen: true,
      rightSidebarTab: 'checks',
      worktreesByRepo: {}
    } as unknown as Parameters<typeof rightSidebarShowsPullRequestData>[0]

    expect(rightSidebarShowsPullRequestData(partialState)).toBe(false)
    // SAFETY: same partial-store condition for the view-only helper.
    expect(canShowRightSidebarForView(undefined as unknown as AppState['activeView'])).toBe(true)
  })
})
