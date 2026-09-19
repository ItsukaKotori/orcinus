import { beforeEach, describe, expect, it, vi } from 'vitest'
import { recordKeyboardCreatedTerminalPaneSplit } from './keyboard-handlers'
import { recordContextMenuCreatedTerminalPaneSplit } from './use-terminal-pane-split-actions'
import { recordRuntimeCreatedTerminalPaneSplit } from './use-terminal-pane-lifecycle'

const mocks = vi.hoisted(() => ({
  recordFeatureInteraction: vi.fn()
}))

vi.mock('@/store', () => ({
  useAppStore: {
    getState: () => ({
      recordFeatureInteraction: mocks.recordFeatureInteraction
    })
  }
}))

describe('terminal split writer paths', () => {
  beforeEach(() => {
    mocks.recordFeatureInteraction.mockReset()
  })

  it('does not record keyboard split completion when the local split fails', () => {
    expect(recordKeyboardCreatedTerminalPaneSplit(null)).toBe(false)

    expect(mocks.recordFeatureInteraction).not.toHaveBeenCalled()
  })

  it('records context-menu split completion after the local split succeeds', () => {
    expect(recordContextMenuCreatedTerminalPaneSplit({ id: 2 })).toBe(true)

    expect(mocks.recordFeatureInteraction).toHaveBeenCalledWith('terminal-pane-split')
  })

  it('records runtime split completion after SPLIT_TERMINAL_PANE_EVENT creates a pane', () => {
    expect(recordRuntimeCreatedTerminalPaneSplit({ id: 2 })).toBe(true)

    expect(mocks.recordFeatureInteraction).toHaveBeenCalledWith('terminal-pane-split')
  })
})
