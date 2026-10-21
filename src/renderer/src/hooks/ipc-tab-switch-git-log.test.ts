import { describe, expect, it, vi } from 'vitest'
import { activateCyclableTab } from './ipc-tab-switch'

function fakeStore() {
  const store = {
    setActiveTab: vi.fn(),
    setActiveTabType: vi.fn(),
    setActiveFile: vi.fn(),
    setActiveBrowserTab: vi.fn(),
    activateTab: vi.fn()
  }
  return store as typeof store & Parameters<typeof activateCyclableTab>[0]
}

describe('activateCyclableTab git-log rows', () => {
  it('activates a git-log tab by unified id without touching the editor file stack', () => {
    // Why: git-log rows joined the visible nav order (C2); without a dedicated
    // branch the Ctrl+Tab chord fell through to the editor path and called
    // setActiveFile with a git-log id.
    const store = fakeStore()
    activateCyclableTab(store, { type: 'git-log', id: 'git-log/wt-1', tabId: 'git-log/wt-1' })

    expect(store.activateTab).toHaveBeenCalledWith('git-log/wt-1')
    expect(store.setActiveTabType).toHaveBeenCalledWith('git-log')
    expect(store.setActiveFile).not.toHaveBeenCalled()
    expect(store.setActiveTab).not.toHaveBeenCalled()
  })
})
