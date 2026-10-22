import { describe, expect, it } from 'vitest'
import type { Tab, TabContentType } from '../../../../shared/tab-types'
import { resolveTabGroupActiveSurface } from './tab-group-active-surface'

function tab(contentType: TabContentType): Tab {
  return {
    id: `unified-${contentType}`,
    entityId: `entity-${contentType}`,
    contentType,
    groupId: 'group-1',
    worktreeId: 'worktree-1',
    label: contentType,
    customLabel: null,
    color: null,
    sortOrder: 0,
    createdAt: 0
  }
}

describe('resolveTabGroupActiveSurface', () => {
  it('maps a plugin tab to the plugin visible type keyed by unified id', () => {
    expect(resolveTabGroupActiveSurface(tab('plugin'))).toEqual({
      activeTabId: 'unified-plugin',
      activeFileId: null,
      activeTabType: 'plugin'
    })
  })

  it('maps a git-log tab to the git-log visible type keyed by unified id', () => {
    // Why: the strip highlights rows off this surface; collapsing git-log to the
    // editor path (activeTabId null) leaves the open Git Log tab unhighlighted.
    expect(resolveTabGroupActiveSurface(tab('git-log'))).toEqual({
      activeTabId: 'unified-git-log',
      activeFileId: null,
      activeTabType: 'git-log'
    })
  })

  it('keeps terminal rows keyed by entity id', () => {
    expect(resolveTabGroupActiveSurface(tab('terminal'))).toEqual({
      activeTabId: 'entity-terminal',
      activeFileId: null,
      activeTabType: 'terminal'
    })
  })

  it('maps agent sessions to their unified id and editor family to activeFileId', () => {
    expect(resolveTabGroupActiveSurface(tab('agent-session')).activeTabId).toBe(
      'unified-agent-session'
    )
    expect(resolveTabGroupActiveSurface(tab('agent-session')).activeTabType).toBe('agent-session')
    expect(resolveTabGroupActiveSurface(tab('diff')).activeFileId).toBe('unified-diff')
    expect(resolveTabGroupActiveSurface(tab('diff')).activeTabType).toBe('editor')
  })

  it('returns the inert editor defaults with no active tab', () => {
    expect(resolveTabGroupActiveSurface(null)).toEqual({
      activeTabId: null,
      activeFileId: null,
      activeTabType: 'editor'
    })
  })
})
