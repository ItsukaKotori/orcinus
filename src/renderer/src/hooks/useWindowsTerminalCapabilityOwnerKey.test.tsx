import { describe, expect, it } from 'vitest'
import { resolveWindowsTerminalCapabilityOwnerKey } from './useWindowsTerminalCapabilityOwnerKey'

describe('Windows terminal capability owner key', () => {
  it('preserves desktop-local and desktop-remote owner keys', () => {
    expect(resolveWindowsTerminalCapabilityOwnerKey({ activeRuntimeEnvironmentId: null })).toBe(
      'local'
    )
    expect(
      resolveWindowsTerminalCapabilityOwnerKey({ activeRuntimeEnvironmentId: 'remote-a' })
    ).toBe('runtime:remote-a')
  })
})
