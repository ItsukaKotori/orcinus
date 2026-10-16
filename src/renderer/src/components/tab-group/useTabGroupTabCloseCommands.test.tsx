// @vitest-environment happy-dom
import { renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Tab } from '../../../../shared/tab-types'

const mocks = vi.hoisted(() => ({
  closeWebRuntimeSessionTab: vi.fn(async (_args: { environmentId: string | null }) => true),
  isWebRuntimeSessionActive: vi.fn(() => true),
  closeTerminalTab: vi.fn(),
  requestEditorFileClose: vi.fn(),
  getRuntimeEnvironmentIdForWorktree: vi.fn(() => null as string | null)
}))

vi.mock('../../runtime/web-runtime-session', () => ({
  closeWebRuntimeSessionTab: mocks.closeWebRuntimeSessionTab,
  isWebRuntimeSessionActive: mocks.isWebRuntimeSessionActive
}))
vi.mock('../terminal/terminal-tab-actions', () => ({ closeTerminalTab: mocks.closeTerminalTab }))
vi.mock('../editor/editor-autosave', () => ({
  requestEditorFileClose: mocks.requestEditorFileClose
}))
vi.mock('@/lib/worktree-runtime-owner', () => ({
  getRuntimeEnvironmentIdForWorktree: mocks.getRuntimeEnvironmentIdForWorktree
}))

import { useAppStore } from '../../store'
import { useTabGroupTabCloseCommands } from './useTabGroupTabCloseCommands'

const CLOSE_BASE = {
  groupId: 'group-1'
} as const

let closeUnifiedTab: ReturnType<typeof vi.fn>

beforeEach(() => {
  // clearAllMocks leaves implementations in place, so a per-test mockReturnValue would leak.
  mocks.isWebRuntimeSessionActive.mockReturnValue(true)
  mocks.getRuntimeEnvironmentIdForWorktree.mockReturnValue(null)
  closeUnifiedTab = vi.fn()
  useAppStore.setState({
    closeUnifiedTab,
    closeTab: vi.fn(),
    closeFile: vi.fn(),
    setActiveWorktree: vi.fn(),
    reconcileWorktreeTabModel: vi.fn(() => ({ renderableTabCount: 1 })),
    unifiedTabsByWorktree: { 'worktree-a': [] }
  } as never)
})

afterEach(() => vi.clearAllMocks())

describe('shared tab close policies', () => {
  it('queues the dirty editor close without removing its tab or file', () => {
    const editor = {
      ...CLOSE_BASE,
      id: 'editor-tab',
      worktreeId: 'worktree-a',
      entityId: 'dirty-file',
      contentType: 'editor',
      label: 'file',
      customLabel: null,
      color: null,
      sortOrder: 0,
      createdAt: 0
    } as Tab
    useAppStore.setState({
      unifiedTabsByWorktree: { 'worktree-a': [editor] },
      openFiles: [{ id: 'dirty-file', isDirty: true, worktreeId: 'worktree-a' }]
    } as never)
    renderHook(() =>
      useTabGroupTabCloseCommands({ worktreeId: 'worktree-a', groupTabs: [editor] })
    ).result.current.closeItem(editor.id)
    expect(mocks.requestEditorFileClose).toHaveBeenCalledWith('dirty-file')
    expect(closeUnifiedTab).not.toHaveBeenCalled()
    expect(useAppStore.getState().closeFile).not.toHaveBeenCalled()
  })

  it('shares terminal teardown while skipping running-process prompts only for bulk closes', () => {
    const terminal = {
      ...CLOSE_BASE,
      id: 'terminal-tab',
      worktreeId: 'worktree-a',
      entityId: 'terminal-entity',
      contentType: 'terminal',
      label: 'terminal',
      customLabel: null,
      color: null,
      sortOrder: 0,
      createdAt: 0
    } as Tab
    useAppStore.setState({ unifiedTabsByWorktree: { 'worktree-a': [terminal] } })
    const { result } = renderHook(() =>
      useTabGroupTabCloseCommands({ worktreeId: 'worktree-a', groupTabs: [terminal] })
    )
    result.current.closeItem(terminal.id)
    expect(mocks.closeTerminalTab).toHaveBeenLastCalledWith('terminal-entity', {
      onClosed: expect.any(Function)
    })
    result.current.closeMany([terminal.id])
    expect(mocks.closeTerminalTab).toHaveBeenLastCalledWith('terminal-entity', {
      skipRunningProcessConfirm: true
    })
  })
})
