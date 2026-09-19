import { beforeEach, describe, expect, it, vi } from 'vitest'
import { recordCreatedTerminalPaneSplit } from './terminal-pane-split-completion'

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

describe('recordCreatedTerminalPaneSplit', () => {
  beforeEach(() => {
    mocks.recordFeatureInteraction.mockReset()
  })

  it('does not record durable split completion when no pane was created', () => {
    expect(recordCreatedTerminalPaneSplit(null)).toBe(false)

    expect(mocks.recordFeatureInteraction).not.toHaveBeenCalled()
  })

  it('records durable split completion after a pane is created', () => {
    expect(recordCreatedTerminalPaneSplit({ id: 2 })).toBe(true)

    expect(mocks.recordFeatureInteraction).toHaveBeenCalledWith('terminal-pane-split')
  })
})
