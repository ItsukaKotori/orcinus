import { describe, expect, it } from 'vitest'
import { createPluginsApi } from './plugins-api'

describe('Phase 0 plugins mock', () => {
  it('does not leak closure state through the entries list and consent return', async () => {
    const plugins = createPluginsApi()

    const entries = await plugins.list()
    entries[0].status = 'disabled'
    entries[0].capabilities.push({ kind: 'workspace:read', description: 'polluted' })
    entries.push({ ...entries[0], pluginKey: 'polluted' })

    const reread = await plugins.list()
    expect(reread).toHaveLength(3)
    expect(reread[0]).toMatchObject({ pluginKey: 'database-manager', status: 'idle' })
    expect(reread[0].capabilities).toHaveLength(1)

    const consented = await plugins.consent({
      pluginKey: 'database-manager',
      scope: 'global',
      reviewedFingerprint: 'mock-fingerprint-db',
      decision: 'approve'
    })
    consented.pop()
    await expect(plugins.refresh()).resolves.toHaveLength(3)
  })

  it('serves the project lockfile empty state and a no-op open-tab subscription', async () => {
    const plugins = createPluginsApi()

    await expect(plugins.getProjectLockState({ repoId: 'mock-repo-1' })).resolves.toEqual({
      repoId: 'mock-repo-1',
      lockfilePath: '',
      entries: []
    })
    await expect(plugins.installMissingFromLockfile({ repoId: 'mock-repo-1' })).resolves.toEqual([])

    const unsubscribe = plugins.onOpenTab(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()

    const entries = await plugins.list()
    expect(entries.every((entry) => entry.tabs.length >= 0)).toBe(true)
    expect(entries.find((entry) => entry.scope === 'project')?.tabs).toHaveLength(1)
  })

  it('scopes marketplace installs and echoes the reviewed identity', async () => {
    const plugins = createPluginsApi()
    const preview = {
      marketplaceSourceId: 'mock-marketplace-1',
      marketplaceCommit: 'b'.repeat(40),
      pluginKey: 'example.notes',
      resolvedCommit: 'c'.repeat(40)
    }

    await expect(
      plugins.installMarketplacePlugin({ ...preview, scope: 'project', repoId: 'repo-1' })
    ).resolves.toEqual({
      ok: true,
      pluginKey: 'example.notes',
      version: '0.0.0-mock',
      contentHash: 'mock-content-hash',
      consentFingerprint: 'mock-consent-fingerprint',
      resolvedCommit: 'c'.repeat(40)
    })

    await expect(
      plugins.installMarketplacePlugin({ ...preview, scope: 'project' })
    ).rejects.toThrow(/repoId/)
    await expect(
      plugins.installMarketplacePlugin({
        ...preview,
        scope: undefined as unknown as 'global'
      })
    ).rejects.toThrow(/scope/)
  })
})
