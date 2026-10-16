import { afterEach, describe, expect, it, vi } from 'vitest'
import { getTerminalUrlOpenHint } from './terminal-link-open-hints'

function stubPlatform(isMac: boolean): void {
  vi.stubGlobal('navigator', { userAgent: isMac ? 'Mac OS X' : 'Windows NT 10.0' })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('getTerminalUrlOpenHint', () => {
  it('keeps the system-browser wording by default', () => {
    stubPlatform(true)
    expect(getTerminalUrlOpenHint()).toBe(
      'Click for actions, ⌘+click to open, or ⇧⌘+click for system browser'
    )
  })

  it('uses the Ctrl chord off macOS', () => {
    stubPlatform(false)
    expect(getTerminalUrlOpenHint()).toBe(
      'Click for actions, Ctrl+click to open, or Shift+Ctrl+click for system browser'
    )
  })

  it('omits the action-menu gesture when terminal link actions are disabled', () => {
    stubPlatform(false)
    expect(getTerminalUrlOpenHint({ showActions: false })).toBe(
      'Ctrl+click to open, or Shift+Ctrl+click for system browser'
    )
  })
})
