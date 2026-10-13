// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PluginHostListEntry } from '../../../../preload/api-types'
import { getDefaultSettings } from '../../../../shared/constants'
import { PluginsSettingsSection } from './PluginsSettingsSection'

vi.mock('./SettingsSection', () => ({
  SettingsSection: ({
    children,
    headerAction
  }: {
    children: React.ReactNode
    headerAction: React.ReactNode
  }) => (
    <section>
      {headerAction}
      {children}
    </section>
  )
}))

vi.mock('./PluginMarketplaceBrowser', () => ({
  PluginMarketplaceBrowser: ({
    renderInstalledContent
  }: {
    renderInstalledContent?: (search: string) => React.ReactNode
  }) => <>{renderInstalledContent?.('')}</>
}))

const globalTwin: PluginHostListEntry = {
  pluginKey: 'acme.notes',
  scope: 'global',
  consentFingerprint: 'fingerprint-global',
  name: 'Notes Global',
  version: '1.0.0',
  publisher: 'acme',
  status: 'idle',
  needsReconsent: false,
  isDev: false,
  official: false,
  bundled: false,
  capabilities: [{ kind: 'panels', description: 'Add a Notes panel' }],
  panels: [],
  tabs: [],
  commands: [],
  hasWorker: false,
  restarts: 0
}

const projectTwin: PluginHostListEntry = {
  ...globalTwin,
  scope: 'project',
  repoId: 'repo-1',
  consentFingerprint: 'fingerprint-project',
  name: 'Notes Project',
  status: 'pending'
}

function installApi(): void {
  Object.defineProperty(window, 'api', {
    configurable: true,
    value: {
      plugins: {
        list: vi.fn().mockResolvedValue([globalTwin, projectTwin]),
        onChanged: vi.fn().mockReturnValue(vi.fn()),
        refresh: vi.fn().mockResolvedValue([globalTwin, projectTwin]),
        setEnabled: vi.fn().mockResolvedValue([globalTwin, projectTwin]),
        remove: vi.fn().mockResolvedValue([]),
        getLogs: vi.fn().mockResolvedValue([]),
        consent: vi.fn().mockResolvedValue([globalTwin, projectTwin]),
        install: vi.fn()
      }
    }
  })
}

beforeEach(() => {
  installApi()
})

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  Reflect.deleteProperty(window, 'api')
})

function click(element: Element): void {
  element.dispatchEvent(new MouseEvent('click', { bubbles: true }))
}

describe('PluginsSettingsSection consent targeting', () => {
  it('consents the reviewed scope instance when two plugins share a key', async () => {
    const settings = { ...getDefaultSettings('/tmp'), pluginSystemEnabled: true }
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    await act(async () => {
      root.render(
        <PluginsSettingsSection
          mounted
          settings={settings}
          updateSettings={vi.fn().mockResolvedValue(undefined)}
        />
      )
    })

    const projectRow = Array.from(container.querySelectorAll('article')).find((article) =>
      article.textContent?.includes('Notes Project')
    )
    if (!projectRow) {
      throw new Error('missing project twin row')
    }
    const review = Array.from(projectRow.querySelectorAll('button')).find(
      (candidate) => candidate.textContent?.trim() === 'Review & enable'
    )
    await act(async () => {
      click(review!)
    })

    const dialog = document.querySelector('[role="dialog"]')
    expect(dialog?.textContent).toContain('Notes Project')
    expect(dialog?.textContent).toContain('Project scope')
    expect(dialog?.textContent).toContain('must be treated as untrusted')

    const enable = Array.from(document.body.querySelectorAll('button')).find(
      (candidate) => candidate.textContent?.trim() === 'Enable plugin'
    )
    await act(async () => {
      click(enable!)
    })

    expect(window.api.plugins.consent).toHaveBeenCalledWith({
      pluginKey: 'acme.notes',
      scope: 'project',
      repoId: 'repo-1',
      reviewedFingerprint: 'fingerprint-project',
      decision: 'approve'
    })
    act(() => root.unmount())
  })
})
