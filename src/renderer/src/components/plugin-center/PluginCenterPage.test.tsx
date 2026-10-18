// @vitest-environment happy-dom

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  PluginHostListEntry,
  PluginMarketplaceHostInstallPreview,
  PluginMarketplaceHostListing,
  PluginMarketplaceHostSourceState
} from '../../../../preload/api-types'
import { useAppStore } from '@/store'
import { PluginCenterPage } from './PluginCenterPage'

const SOURCE_ID = 'a'.repeat(32)
const MARKETPLACE_COMMIT = 'b'.repeat(40)
const PLUGIN_COMMIT = 'c'.repeat(40)

const source: PluginMarketplaceHostSourceState = {
  id: SOURCE_ID,
  source: { kind: 'git', url: 'https://example.com/marketplace.git', ref: 'main' },
  addedAt: 1,
  marketplace: {
    name: 'Community',
    owner: 'example',
    resolvedCommit: MARKETPLACE_COMMIT,
    fetchedAt: 2
  },
  stale: false,
  official: false
}

const listing: PluginMarketplaceHostListing = {
  marketplaceSourceId: SOURCE_ID,
  marketplaceName: 'Community',
  marketplaceOwner: 'example',
  marketplaceCommit: MARKETPLACE_COMMIT,
  pluginKey: 'example.notes',
  source: { kind: 'git', url: 'https://example.com/notes.git', ref: 'v1' },
  description: 'Notes for active worktrees.',
  categories: ['productivity'],
  official: false,
  bundled: false
}

const preview: PluginMarketplaceHostInstallPreview = {
  ...listing,
  resolvedCommit: PLUGIN_COMMIT,
  contentHash: 'sha256-content',
  consentFingerprint: 'sha256-consent',
  manifest: {
    manifestVersion: 1,
    id: 'notes',
    publisher: 'example',
    name: 'Worktree Notes',
    version: '1.0.0',
    description: 'Notes for active worktrees.',
    engines: { orca: '>=1.0.0' },
    pluginApi: 1,
    main: 'dist/worker.js',
    contributes: {
      panels: [],
      tabs: [],
      commands: [],
      events: [],
      languagePacks: [],
      keybindings: [],
      vmRecipes: [],
      agents: []
    },
    capabilities: []
  }
}

function pluginEntry(overrides: Partial<PluginHostListEntry> = {}): PluginHostListEntry {
  return {
    pluginKey: 'example.notes',
    scope: 'global',
    consentFingerprint: null,
    name: 'Worktree Notes',
    version: '1.0.0',
    publisher: 'example',
    status: 'idle',
    needsReconsent: false,
    isDev: false,
    official: false,
    bundled: false,
    capabilities: [],
    panels: [],
    tabs: [],
    commands: [],
    hasWorker: true,
    restarts: 0,
    source: {
      kind: 'marketplace',
      reference: listing.source.url,
      resolvedCommit: PLUGIN_COMMIT,
      contentHash: 'other-content',
      marketplace: {
        reference: source.source.url,
        resolvedCommit: MARKETPLACE_COMMIT
      }
    },
    ...overrides
  }
}

const devEntry = pluginEntry({
  pluginKey: 'example.dev-notes',
  name: 'Dev Notes',
  isDev: true,
  status: 'running',
  source: { kind: 'dev', reference: '/Users/you/plugins/dev-notes' }
})

const stableEntry = pluginEntry({ pluginKey: 'example.stable', name: 'Stable Notes' })

const listMock = vi.fn()
const onChangedMock = vi.fn()
const listMarketplacesMock = vi.fn()
const listMarketplacePluginsMock = vi.fn()
const refreshMarketplacesMock = vi.fn()
const previewMarketplacePluginMock = vi.fn()
const installMarketplacePluginMock = vi.fn()
const rollbackMarketplacePluginMock = vi.fn()
const refreshMock = vi.fn()

function installApi(): void {
  Object.defineProperty(window, 'api', {
    configurable: true,
    value: {
      plugins: {
        list: listMock,
        onChanged: onChangedMock,
        listMarketplaces: listMarketplacesMock,
        listMarketplacePlugins: listMarketplacePluginsMock,
        refreshMarketplaces: refreshMarketplacesMock,
        previewMarketplacePlugin: previewMarketplacePluginMock,
        previewMarketplaceUpdate: vi.fn().mockResolvedValue(preview),
        installMarketplacePlugin: installMarketplacePluginMock,
        rollbackMarketplacePlugin: rollbackMarketplacePluginMock,
        addMarketplace: vi.fn(),
        removeMarketplace: vi.fn(),
        refresh: refreshMock,
        setEnabled: vi.fn(),
        remove: vi.fn(),
        getLogs: vi.fn(),
        getProjectLockState: vi.fn().mockResolvedValue({
          repoId: 'repo-1',
          lockfilePath: '/repo/.ade/plugins.lock.json',
          entries: []
        })
      }
    }
  })
}

function openTab(name: string): void {
  fireEvent.mouseDown(screen.getByRole('tab', { name }))
}

beforeEach(() => {
  vi.clearAllMocks()
  useAppStore.setState({
    activeRepoId: null,
    settingsNavigationTarget: null,
    activeView: 'plugin-center'
  })
  listMock.mockResolvedValue([stableEntry])
  onChangedMock.mockReturnValue(() => {})
  listMarketplacesMock.mockResolvedValue([source])
  listMarketplacePluginsMock.mockResolvedValue([listing])
  refreshMarketplacesMock.mockResolvedValue([source])
  previewMarketplacePluginMock.mockResolvedValue(preview)
  installMarketplacePluginMock.mockResolvedValue({
    ok: true,
    pluginKey: listing.pluginKey,
    version: preview.manifest.version,
    contentHash: preview.contentHash,
    consentFingerprint: preview.consentFingerprint,
    resolvedCommit: PLUGIN_COMMIT
  })
  rollbackMarketplacePluginMock.mockResolvedValue({
    ok: true,
    pluginKey: listing.pluginKey,
    version: '0.9.0',
    contentHash: preview.contentHash,
    consentFingerprint: preview.consentFingerprint,
    resolvedCommit: PLUGIN_COMMIT
  })
  refreshMock.mockResolvedValue([stableEntry])
  installApi()
})

afterEach(() => {
  useAppStore.setState({
    activeRepoId: null,
    settingsNavigationTarget: null,
    pluginCenterEntries: [],
    pluginCenterStatus: 'idle'
  })
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

describe('PluginCenterPage market tab', () => {
  it('renders marketplace listings from the real API', async () => {
    render(<PluginCenterPage />)
    openTab('市场')

    expect(await screen.findByText('Notes for active worktrees.')).toBeTruthy()
    expect(listMarketplacesMock).toHaveBeenCalled()
    expect(listMarketplacePluginsMock).toHaveBeenCalled()
  })

  it('reviews a listing and installs with the selected project scope', async () => {
    useAppStore.setState({ activeRepoId: 'repo-1' })
    render(<PluginCenterPage />)
    openTab('市场')

    fireEvent.click(await screen.findByRole('button', { name: 'Install' }))
    expect(previewMarketplacePluginMock).toHaveBeenCalledWith({
      marketplaceSourceId: SOURCE_ID,
      pluginKey: listing.pluginKey
    })
    fireEvent.click(await screen.findByRole('radio', { name: 'Current project' }))
    fireEvent.click(screen.getByRole('button', { name: 'Install plugin' }))

    await waitFor(() =>
      expect(installMarketplacePluginMock).toHaveBeenCalledWith({
        marketplaceSourceId: SOURCE_ID,
        marketplaceCommit: MARKETPLACE_COMMIT,
        pluginKey: listing.pluginKey,
        resolvedCommit: PLUGIN_COMMIT,
        scope: 'project',
        repoId: 'repo-1'
      })
    )
  })

  it('disables the project scope without an active project', async () => {
    render(<PluginCenterPage />)
    openTab('市场')

    fireEvent.click(await screen.findByRole('button', { name: 'Install' }))

    expect(
      (await screen.findByRole('radio', { name: 'Current project' })).getAttribute('aria-disabled')
    ).toBe('true')
  })

  it('refreshes a marketplace source from the source dialog', async () => {
    render(<PluginCenterPage />)
    openTab('市场')

    fireEvent.click(await screen.findByRole('button', { name: /Manage sources/ }))
    fireEvent.click(await screen.findByRole('button', { name: 'Refresh Community' }))

    await waitFor(() =>
      expect(refreshMarketplacesMock).toHaveBeenCalledWith({ sourceId: SOURCE_ID })
    )
  })

  it('rolls back an installed marketplace plugin', async () => {
    listMock.mockResolvedValue([pluginEntry()])
    render(<PluginCenterPage />)
    openTab('市场')

    fireEvent.click(await screen.findByRole('button', { name: 'Roll back' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Roll back plugin' }))

    await waitFor(() =>
      expect(rollbackMarketplacePluginMock).toHaveBeenCalledWith({
        pluginKey: listing.pluginKey
      })
    )
  })

  it('shows the load error with a retry action', async () => {
    listMarketplacesMock.mockRejectedValue(new Error('offline'))
    render(<PluginCenterPage />)
    openTab('市场')

    expect(await screen.findByText('Could not load marketplace plugins.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Try again' })).toBeTruthy()
  })

  it('shows the empty state when no sources are configured', async () => {
    listMarketplacesMock.mockResolvedValue([])
    render(<PluginCenterPage />)
    openTab('市场')

    expect(await screen.findByText('No marketplaces configured')).toBeTruthy()
  })
})

describe('PluginCenterPage dev tab', () => {
  it('lists dev plugins with their status and hides non-dev entries', async () => {
    listMock.mockResolvedValue([stableEntry, devEntry])
    render(<PluginCenterPage />)
    openTab('开发中')

    expect(await screen.findByText('Dev Notes')).toBeTruthy()
    expect(screen.queryByText('Stable Notes')).toBeNull()
    expect(screen.getByText('运行中')).toBeTruthy()
    expect(screen.getByText('devPluginPaths', { exact: false })).toBeTruthy()
  })

  it('reloads dev plugins through plugins.refresh', async () => {
    listMock.mockResolvedValue([devEntry])
    render(<PluginCenterPage />)
    openTab('开发中')

    fireEvent.click(await screen.findByRole('button', { name: /重新加载/ }))

    await waitFor(() => expect(refreshMock).toHaveBeenCalled())
    expect(listMock.mock.calls.length).toBeGreaterThan(1)
  })

  it('points at the settings dev section when no dev plugins exist', async () => {
    render(<PluginCenterPage />)
    openTab('开发中')

    expect(await screen.findByText(/在「设置 → 插件 → 开发」中配置 devPluginPaths/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /打开插件设置/ }))

    expect(useAppStore.getState().settingsNavigationTarget).toEqual({
      pane: 'plugins',
      repoId: null
    })
    expect(useAppStore.getState().activeView).toBe('settings')
  })

  it('reloads entries when plugins.onChanged fires', async () => {
    let changed: (() => void) | undefined
    onChangedMock.mockImplementation((callback: () => void) => {
      changed = callback
      return () => {}
    })
    listMock.mockResolvedValue([devEntry])
    render(<PluginCenterPage />)
    openTab('开发中')
    await screen.findByText('Dev Notes')
    const before = listMock.mock.calls.length

    await waitFor(() => expect(changed).toBeDefined())
    changed?.()

    await waitFor(() => expect(listMock.mock.calls.length).toBeGreaterThan(before))
  })
})
