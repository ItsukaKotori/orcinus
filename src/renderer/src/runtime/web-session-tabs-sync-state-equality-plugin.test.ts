import { describe, expect, it } from 'vitest'
import type { Tab } from '../../../shared/tab-types'
import { toVisibleTabType } from './web-session-tabs-sync/state-equality-files'

function tab(overrides: Partial<Tab>): Tab {
  return {
    id: 'tab-1',
    entityId: 'tab-1',
    groupId: 'group-1',
    worktreeId: 'wt-1',
    contentType: 'terminal',
    label: 'Tab',
    customLabel: null,
    color: null,
    sortOrder: 0,
    createdAt: 0,
    ...overrides
  }
}

describe('web-session tabs sync visible-type projection', () => {
  it('projects plugin tabs to plugin instead of collapsing to editor', () => {
    expect(toVisibleTabType(tab({ contentType: 'plugin' }))).toBe('plugin')
  })

  it('keeps the existing kind mappings intact', () => {
    expect(toVisibleTabType(tab({ contentType: 'terminal' }))).toBe('terminal')
    expect(toVisibleTabType(tab({ contentType: 'browser' }))).toBe('browser')
    expect(toVisibleTabType(tab({ contentType: 'agent-session' }))).toBe('agent-session')
    expect(toVisibleTabType(tab({ contentType: 'editor' }))).toBe('editor')
    expect(toVisibleTabType(tab({ contentType: 'diff' }))).toBe('editor')
  })
})
