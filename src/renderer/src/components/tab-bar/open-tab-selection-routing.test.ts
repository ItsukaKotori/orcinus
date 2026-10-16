// @vitest-environment happy-dom

import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { OpenTabSearchResult } from './open-tab-search'

const mocks = vi.hoisted(() => ({
  activateWorkspaceTab: vi.fn(),
  focusTerminalTabSurface: vi.fn()
}))

vi.mock('@/lib/workspace-tab-palette-activation', () => ({
  activateWorkspaceTabPaletteResult: mocks.activateWorkspaceTab
}))
vi.mock('@/lib/focus-terminal-tab-surface', () => ({
  focusTerminalTabSurface: mocks.focusTerminalTabSurface
}))

import { activateOpenTabSearchResult } from './open-tab-selection-routing'

const terminalResult: Extract<OpenTabSearchResult, { source: 'workspace' }> = {
  executionHostId: 'runtime:host-1',
  source: 'workspace',
  id: 'open-tab:workspace:tab-1',
  title: 'Claude Code',
  matchedText: null,
  worktreeId: 'wt-1',
  contentType: 'terminal',
  tabId: 'tab-1',
  entityId: 'term-1',
  groupId: 'group-2',
  relativePath: null,
  occupantAgent: null
}

const editorResult: OpenTabSearchResult = {
  ...terminalResult,
  id: 'open-tab:workspace:tab-2',
  contentType: 'editor',
  tabId: 'tab-2',
  entityId: 'file-2',
  relativePath: 'src/zebra.ts'
}


describe('activateOpenTabSearchResult', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.activateWorkspaceTab.mockReturnValue({ status: 'activated' })
  })

  it('activates a workspace tab by its identifiers and focuses the terminal surface', () => {
    const outcome = activateOpenTabSearchResult(terminalResult)

    expect(mocks.activateWorkspaceTab).toHaveBeenCalledWith({
      contentType: 'terminal',
      entityId: 'term-1',
      executionHostId: 'runtime:host-1',
      groupId: 'group-2',
      tabId: 'tab-1',
      worktreeId: 'wt-1'
    })
    expect(outcome).toMatchObject({ status: 'activated' })

    if (outcome.status !== 'activated') {
      throw new Error('expected activation')
    }
    outcome.focus?.()
    expect(mocks.focusTerminalTabSurface).toHaveBeenCalledWith('term-1')
  })

  it('queues no focus handoff for an editor tab, which focuses itself', () => {
    expect(activateOpenTabSearchResult(editorResult)).toEqual({ status: 'activated', focus: null })
  })

  it('reports a stale tab target', () => {
    mocks.activateWorkspaceTab.mockReturnValue({ status: 'failed', reason: 'missing-tab' })
    expect(activateOpenTabSearchResult(terminalResult)).toEqual({
      status: 'failed',
      message: 'Tab no longer exists'
    })
  })

  it('reports a missing worktree distinguishably from a stale tab', () => {
    mocks.activateWorkspaceTab.mockReturnValue({ status: 'failed', reason: 'missing-worktree' })

    expect(activateOpenTabSearchResult(terminalResult)).toEqual({
      status: 'failed',
      message: 'Workspace no longer exists'
    })
    expect(mocks.focusTerminalTabSurface).not.toHaveBeenCalled()
  })
})
