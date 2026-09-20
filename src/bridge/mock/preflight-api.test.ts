import { describe, expect, it } from 'vitest'
import { createPreflightApi } from './preflight-api'

describe('Phase 0 preflight mock', () => {
  it('does not leak module state through the status and agent report it returns', async () => {
    const preflight = createPreflightApi()

    const status = await preflight.check()
    status.git.installed = true
    status.gh.authenticated = true
    await expect(preflight.check()).resolves.toEqual({
      git: { installed: false },
      gh: { installed: false, authenticated: false }
    })

    const report = await preflight.refreshAgents()
    report.shellHydrationOk = true
    report.addedPathSegments.push('/polluted')
    await expect(preflight.refreshAgents()).resolves.toMatchObject({
      shellHydrationOk: false,
      addedPathSegments: []
    })
  })
})
