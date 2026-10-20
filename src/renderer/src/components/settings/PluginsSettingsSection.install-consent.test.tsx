// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PluginHostInstallSource, PluginHostListEntry } from '../../../../preload/api-types'
import { getDefaultSettings } from '../../../../shared/constants'
import { PluginsSettingsSection } from './PluginsSettingsSection'

vi.mock('./SettingsSection', () => ({
  SettingsSection: ({ children, headerAction }: { children: React.ReactNode; headerAction: React.ReactNode }) => (
    <section>
      {headerAction}
      {children}
    </section>
  )
}))

vi.mock('./PluginMarketplaceBrowser', () => ({
  PluginMarketplaceBrowser: ({ renderInstalledContent }: { renderInstalledContent?: (search: string) => React.ReactNode }) => (
    <>{renderInstalledContent?.('')}</>
  )
}))

// Drive the real install() path without the install dialog's own UI: expose a
// trigger that calls onInstall directly.
vi.mock('./PluginInstallDialog', () => ({
  PluginInstallDialog: ({ onInstall }: { onInstall: (source: PluginHostInstallSource) => Promise<void> }) => (
    <button
      type="button"
      data-testid="test-install"
      onClick={() => void onInstall({ scope: 'global', kind: 'local-path', path: '/tmp/new' })}
    >
      install
    </button>
  )
}))

const installedPending: PluginHostListEntry = {
  pluginKey: 'acme.new',
  scope: 'global',
  consentFingerprint: 'fingerprint-new',
  name: 'New Plugin',
  version: '1.0.0',
  publisher: 'acme',
  status: 'pending',
  needsReconsent: false,
  isDev: false,
  official: false,
  bundled: false,
  capabilities: [{ kind: 'git:read', description: 'Read git history' }],
  panels: [],
  tabs: [],
  commands: [],
  hasWorker: false,
  restarts: 0
}

beforeEach(() => {
  Object.defineProperty(window, 'api', {
    configurable: true,
    value: {
      plugins: {
        // Not present until the post-install refresh resolves.
        list: vi.fn().mockResolvedValue([]),
        onChanged: vi.fn().mockReturnValue(vi.fn()),
        refresh: vi.fn().mockResolvedValue([installedPending]),
        setEnabled: vi.fn().mockResolvedValue([installedPending]),
        remove: vi.fn().mockResolvedValue([]),
        getLogs: vi.fn().mockResolvedValue([]),
        consent: vi.fn().mockResolvedValue([installedPending]),
        install: vi.fn().mockResolvedValue({
          ok: true,
          pluginKey: 'acme.new',
          version: '1.0.0',
          contentHash: 'hash',
          consentFingerprint: 'fingerprint-new'
        })
      }
    }
  })
})

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  Reflect.deleteProperty(window, 'api')
})

describe('PluginsSettingsSection install→consent timing', () => {
  it('opens the consent dialog once the installed plugin arrives in the list', async () => {
    // refresh() is deferred so we can commit the intermediate render where
    // consentTarget is set but the plugin is NOT yet in the list — the exact
    // window that froze the dialog's `useRef(currentPlugin)` to null pre-fix.
    let resolveRefresh!: (entries: PluginHostListEntry[]) => void
    const plugins = (window as unknown as { api: { plugins: Record<string, unknown> } }).api.plugins
    plugins.refresh = vi.fn().mockImplementation(
      () => new Promise<PluginHostListEntry[]>((resolve) => { resolveRefresh = resolve }),
    )

    const settings = { ...getDefaultSettings('/tmp'), pluginSystemEnabled: true }
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    await act(async () => {
      root.render(
        <PluginsSettingsSection mounted settings={settings} updateSettings={vi.fn().mockResolvedValue(undefined)} />
      )
    })

    const trigger = container.querySelector('[data-testid="test-install"]')
    if (!trigger) {
      throw new Error('missing install trigger')
    }
    // install() sets consentTarget, then awaits the (still-pending) refresh.
    await act(async () => {
      trigger!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    // Now the plugin arrives; the dialog must mount and open for it.
    await act(async () => {
      resolveRefresh([installedPending])
      await Promise.resolve()
    })

    const dialog = document.querySelector('[role="dialog"]')
    expect(dialog?.textContent).toContain('New Plugin')
    expect(dialog?.textContent).toContain('Global scope')
    act(() => root.unmount())
  })
})
