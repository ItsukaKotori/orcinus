import { describe, expect, it } from 'vitest'

import { resolveModalReturnFocusAction } from './modal-return-focus-action'

describe('resolveModalReturnFocusAction', () => {
  it('returns none when nothing was captured', () => {
    expect(resolveModalReturnFocusAction(null)).toEqual({ kind: 'none' })
  })

  it('restores a terminal tab through the scoped terminal focus path', () => {
    expect(
      resolveModalReturnFocusAction({
        tabType: 'terminal',
        worktreeId: 'wt-1',
        terminalTabId: 'terminal-1',
        terminalLeafId: 'leaf-1'
      })
    ).toEqual({ kind: 'terminal', tabId: 'terminal-1', leafId: 'leaf-1' })
  })

  it('restores the editor surface before falling back to terminal focus', () => {
    expect(
      resolveModalReturnFocusAction({
        tabType: 'editor',
        worktreeId: 'wt-1',
        terminalTabId: null,
        terminalLeafId: null
      })
    ).toEqual({ kind: 'editor' })
  })

  it('returns none when there is no worktree to restore into', () => {
    expect(
      resolveModalReturnFocusAction({
        tabType: 'terminal',
        worktreeId: null,
        terminalTabId: null,
        terminalLeafId: null
      })
    ).toEqual({ kind: 'none' })
  })
})
