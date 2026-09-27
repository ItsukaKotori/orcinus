import { describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createWorkspacePortsApi } from './workspace-ports-api'

describe('Phase 0 workspacePorts mock', () => {
  it('answers scan with an explicit empty unavailable result instead of rejecting', async () => {
    const scan = await createWorkspacePortsApi().scan({})

    expect(scan).toMatchObject({
      platform: 'unknown',
      ports: [],
      unavailableReason: 'not-implemented'
    })
    expect(typeof scan.scannedAt).toBe('number')
  })

  it('keeps kill on the unimplemented fallback', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await expect(createWorkspacePortsApi().kill({ pid: 1, port: 3000 })).rejects.toBeInstanceOf(
      UnimplementedBridgeError
    )
    warn.mockRestore()
  })
})
