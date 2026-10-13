// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PluginHostListEntry } from '../../../../preload/api-types'

const { storeState } = vi.hoisted(() => ({
  storeState: { pluginCenterEntries: [] as PluginHostListEntry[] }
}))

vi.mock('@/store', () => ({
  useAppStore: (selector: (store: typeof storeState) => unknown) => selector(storeState)
}))

vi.mock('../right-sidebar/plugin-panel-watchdog', () => ({
  createPanelWatchdog: () => ({ start: vi.fn(), stop: vi.fn(), handlePong: vi.fn() })
}))

import { GlobalPluginHostView } from './GlobalPluginHostView'

;(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true

const globalEntry: PluginHostListEntry = {
  pluginKey: 'orca-samples.my-plugin',
  scope: 'global',
  consentFingerprint: 'fingerprint',
  name: 'My Plugin',
  version: '1.0.0',
  publisher: 'orca',
  status: 'idle',
  needsReconsent: false,
  isDev: false,
  official: false,
  bundled: false,
  capabilities: [],
  panels: [
    {
      id: 'dashboard',
      title: 'Dashboard',
      icon: 'gauge',
      tabKey: 'plugin:orca-samples.my-plugin/dashboard'
    }
  ],
  tabs: [],
  commands: [],
  hasWorker: false,
  restarts: 0
}

let container: HTMLDivElement
let root: Root
const readPanelEntryMock = vi.fn()
let pluginChangedListener: (() => void) | null

function waitForHappyDomTasks(): Promise<void> {
  return (
    window as unknown as { happyDOM: { waitUntilComplete: () => Promise<void> } }
  ).happyDOM.waitUntilComplete()
}

beforeEach(() => {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  readPanelEntryMock.mockReset()
  pluginChangedListener = null
  storeState.pluginCenterEntries = [globalEntry]
  globalThis.window.api = {
    plugins: {
      readPanelEntry: readPanelEntryMock,
      panelAction: vi.fn(),
      onChanged: (listener: () => void) => {
        pluginChangedListener = listener
        return vi.fn()
      }
    }
  } as unknown as Window['api']
})

afterEach(async () => {
  await act(async () => {
    root.unmount()
  })
  container.remove()
  vi.restoreAllMocks()
})

async function renderView(pluginKey = 'orca-samples.my-plugin'): Promise<void> {
  await act(async () => {
    root.render(<GlobalPluginHostView pluginKey={pluginKey} />)
  })
}

describe('GlobalPluginHostView', () => {
  it('reads the first panel contribution through the global scope and renders the sandboxed iframe', async () => {
    readPanelEntryMock.mockResolvedValue({
      html: '<h1>Global plugin</h1>',
      sessionToken: 's'.repeat(43)
    })

    await renderView()

    expect(readPanelEntryMock).toHaveBeenCalledWith({
      pluginKey: 'orca-samples.my-plugin',
      panelId: 'dashboard',
      scope: 'global'
    })
    const iframe = container.querySelector('iframe')
    expect(iframe).not.toBeNull()
    expect(iframe?.getAttribute('srcdoc')).toContain('<h1>Global plugin</h1>')
    expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts')
    expect(iframe?.getAttribute('name')).toBe('orca-plugin-panel:plugin:orca-samples.my-plugin')
    expect(iframe?.getAttribute('title')).toBe('Dashboard')
  })

  it('re-reads the entry when plugin data changes', async () => {
    readPanelEntryMock.mockResolvedValue({
      html: '<h1>Global plugin</h1>',
      sessionToken: 's'.repeat(43)
    })
    await renderView()
    expect(readPanelEntryMock).toHaveBeenCalledTimes(1)

    readPanelEntryMock.mockResolvedValue({
      html: '<h1>Reloaded plugin</h1>',
      sessionToken: 'r'.repeat(43)
    })
    await act(async () => {
      pluginChangedListener?.()
      await waitForHappyDomTasks()
    })

    expect(readPanelEntryMock).toHaveBeenCalledTimes(2)
    expect(container.querySelector('iframe')?.getAttribute('srcdoc')).toContain('Reloaded plugin')
  })

  it('shows the unavailable state when the plugin contributes no panel', async () => {
    storeState.pluginCenterEntries = [{ ...globalEntry, panels: [] }]

    await renderView()

    expect(readPanelEntryMock).not.toHaveBeenCalled()
    expect(container.querySelector('iframe')).toBeNull()
    expect(container.textContent).toContain('此全局插件没有可用的面板。')
  })

  it('shows the unavailable state for an unknown or project-scoped plugin key', async () => {
    storeState.pluginCenterEntries = [{ ...globalEntry, scope: 'project', repoId: 'repo-1' }]

    await renderView()

    expect(readPanelEntryMock).not.toHaveBeenCalled()
    expect(container.textContent).toContain('此全局插件没有可用的面板。')
  })

  it('shows an error state when the panel entry cannot be read', async () => {
    readPanelEntryMock.mockResolvedValue(null)

    await renderView()

    expect(container.querySelector('iframe')).toBeNull()
    expect(container.textContent).toContain('全局插件面板加载失败。')
  })
})
