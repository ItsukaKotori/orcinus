import { describe, expect, it } from 'vitest'
import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import { resolveActiveTabOwnerWorktreeId } from './active-tab-owner-worktree'

function tab(id: string, worktreeId: string): TerminalTab {
  return { id, worktreeId, title: id, createdAt: 0, sortOrder: 0 } as unknown as TerminalTab
}

describe('resolveActiveTabOwnerWorktreeId', () => {
  it('returns the sole owner', () => {
    const owner = resolveActiveTabOwnerWorktreeId(
      { 'wt-a': [tab('t1', 'wt-a')], 'wt-b': [tab('t2', 'wt-b')] },
      'wt-a',
      't1'
    )
    expect(owner).toBe('wt-a')
  })

  it('returns null when no worktree owns the tab', () => {
    expect(resolveActiveTabOwnerWorktreeId({ 'wt-a': [tab('t1', 'wt-a')] }, 'wt-a', 'gone')).toBe(
      null
    )
  })

  it('prefers the active worktree over an earlier-scanned duplicate', () => {
    const owner = resolveActiveTabOwnerWorktreeId(
      { 'wt-other': [tab('t1', 'wt-other')], 'wt-active': [tab('t1', 'wt-active')] },
      'wt-active',
      't1'
    )
    expect(owner).toBe('wt-active')
  })

  it('falls back to first match when the active worktree is not an owner', () => {
    const owner = resolveActiveTabOwnerWorktreeId(
      { 'wt-x': [tab('t1', 'wt-x')], 'wt-y': [tab('t1', 'wt-y')] },
      'wt-active',
      't1'
    )
    expect(owner).toBe('wt-x')
  })

  // Why: a truthiness guard on the active id would drop this back to first-match.
  it('prefers a falsy-but-valid active worktree id', () => {
    const owner = resolveActiveTabOwnerWorktreeId(
      { 'wt-other': [tab('t1', 'wt-other')], '': [tab('t1', '')] },
      '',
      't1'
    )
    expect(owner).toBe('')
  })
})
