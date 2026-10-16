import { describe, expect, it, vi } from 'vitest'
import {
  buildNewWorkspaceShortcutModalData,
  openNewWorkspaceFromShortcut
} from './ipc-events/new-workspace-command'

describe('buildNewWorkspaceShortcutModalData', () => {
  it('opens the plain composer without provider context', () => {
    const data = buildNewWorkspaceShortcutModalData({
      activeView: 'tasks',
      taskPageData: {}
    })

    expect(data).toEqual({ telemetrySource: 'shortcut' })
  })

  it('does not reuse stale task context outside the Tasks view', () => {
    const data = buildNewWorkspaceShortcutModalData({
      activeView: 'terminal',
      taskPageData: {}
    })

    expect(data).toEqual({ telemetrySource: 'shortcut' })
  })
})

describe('openNewWorkspaceFromShortcut', () => {
  it('opens the composer even when no project has been added yet', () => {
    const openModal = vi.fn()

    openNewWorkspaceFromShortcut({
      activeModal: 'none',
      activeView: 'terminal',
      taskPageData: {},
      openModal
    })

    expect(openModal).toHaveBeenCalledWith('new-workspace-composer', {
      telemetrySource: 'shortcut'
    })
  })

  it('does not reopen the composer when it is already active', () => {
    const openModal = vi.fn()

    openNewWorkspaceFromShortcut({
      activeModal: 'new-workspace-composer',
      activeView: 'terminal',
      taskPageData: {},
      openModal
    })

    expect(openModal).not.toHaveBeenCalled()
  })
})
