// @vitest-environment happy-dom

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Tab } from '../../../../shared/tab-types'
import type { UsePluginPanelEntryResult } from '../right-sidebar/use-plugin-panel-entry'

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => fallback
}))

const { usePluginPanelEntryMock, hookResult } = vi.hoisted(() => ({
  usePluginPanelEntryMock: vi.fn(),
  hookResult: { current: null as unknown as UsePluginPanelEntryResult }
}))

vi.mock('../right-sidebar/use-plugin-panel-entry', () => ({
  usePluginPanelEntry: (options: unknown) => {
    usePluginPanelEntryMock(options)
    return hookResult.current
  }
}))

import PluginTabContent from './PluginTabContent'

;(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true

const iframeRef = { current: null }
const handleFrameLoad = vi.fn()

function readyResult(): UsePluginPanelEntryResult {
  return {
    entryState: { status: 'ready', shellHtml: '<h1>Tab</h1>', documentRevision: 1 },
    iframeRef,
    panelDocument: '<h1>Tab</h1>',
    panelFrameName: 'orca-plugin-panel:plugin-tab-1',
    panelFrameKey: 'plugin-tab-1:1:0',
    loadedFrameKey: 'plugin-tab-1:1:0',
    handleFrameLoad
  }
}

const pluginTab: Tab = {
  id: 'plugin-tab-1',
  entityId: 'plugin.one/dashboard/inst-1',
  groupId: 'group-1',
  worktreeId: 'wt-1',
  contentType: 'plugin',
  label: 'Board',
  customLabel: null,
  color: null,
  sortOrder: 0,
  createdAt: 0,
  pluginTab: {
    pluginKey: 'plugin.one',
    tabId: 'dashboard',
    scope: 'global',
    instanceId: 'inst-1',
    titleHint: 'Board',
    params: { view: 'open' }
  }
}

let container: HTMLDivElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  usePluginPanelEntryMock.mockReset()
  handleFrameLoad.mockReset()
  hookResult.current = readyResult()
})

afterEach(async () => {
  await act(async () => {
    root.unmount()
  })
  container.remove()
})

function renderContent(tab: Tab, onClose = vi.fn()): ReturnType<typeof vi.fn> {
  root.render(<PluginTabContent tab={tab} onClose={onClose} />)
  return onClose
}

describe('PluginTabContent', () => {
  it('reuses the shared panel session hook with the tab binding', async () => {
    await act(async () => {
      renderContent(pluginTab)
    })

    expect(usePluginPanelEntryMock).toHaveBeenCalledWith({
      pluginKey: 'plugin.one',
      panelId: null,
      tabId: 'dashboard',
      instanceId: 'inst-1',
      worktreeId: 'wt-1',
      scope: 'global',
      instanceKey: 'plugin-tab-1',
      initParams: { view: 'open' }
    })
  })

  it('mounts the shell document in a scripts-only sandboxed iframe', async () => {
    await act(async () => {
      renderContent(pluginTab)
    })

    const iframe = container.querySelector('iframe')
    expect(iframe).not.toBeNull()
    expect(iframe?.getAttribute('srcdoc')).toBe('<h1>Tab</h1>')
    expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts')
    expect(iframe?.getAttribute('name')).toBe('orca-plugin-panel:plugin-tab-1')
    expect(iframe?.getAttribute('title')).toBe('Board')
  })

  it('omits initParams when the tab binding carries none', async () => {
    await act(async () => {
      renderContent({
        ...pluginTab,
        pluginTab: { ...pluginTab.pluginTab!, params: undefined }
      })
    })

    expect(usePluginPanelEntryMock).toHaveBeenCalledWith(
      expect.objectContaining({ tabId: 'dashboard' })
    )
    const options = usePluginPanelEntryMock.mock.calls[0]![0] as Record<string, unknown>
    expect(Object.hasOwn(options, 'initParams')).toBe(false)
  })

  it('renders a closeable loading state', async () => {
    hookResult.current = { ...readyResult(), entryState: { status: 'loading' } }
    const onClose = vi.fn()

    await act(async () => {
      renderContent(pluginTab, onClose)
    })

    expect(container.textContent).toContain('正在加载插件 Tab…')
    ;(container.querySelector('button') as HTMLButtonElement).click()
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('renders a closeable unavailable state when the entry cannot be read', async () => {
    hookResult.current = { ...readyResult(), entryState: { status: 'error' } }
    const onClose = vi.fn()

    await act(async () => {
      renderContent(pluginTab, onClose)
    })

    expect(container.querySelector('iframe')).toBeNull()
    expect(container.textContent).toContain('该插件 Tab 不可用（插件可能已禁用或卸载）。')
    ;(container.querySelector('button') as HTMLButtonElement).click()
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('renders a closeable unresponsive state', async () => {
    hookResult.current = { ...readyResult(), entryState: { status: 'unresponsive' } }

    await act(async () => {
      renderContent(pluginTab)
    })

    expect(container.textContent).toContain('该插件 Tab 已停止响应。')
    expect(container.querySelector('button')).not.toBeNull()
  })

  it('renders a closeable unavailable state for a tab without a binding', async () => {
    const onClose = vi.fn()

    await act(async () => {
      renderContent({ ...pluginTab, pluginTab: undefined }, onClose)
    })

    expect(container.querySelector('iframe')).toBeNull()
    expect(container.textContent).toContain('该插件 Tab 已不可用。')
    ;(container.querySelector('button') as HTMLButtonElement).click()
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('delegates bridge/session plumbing to the shared hook module only', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'src/renderer/src/components/tabs/PluginTabContent.tsx'),
      'utf8'
    )
    expect(source).toContain("from '../right-sidebar/use-plugin-panel-entry'")
    expect(source).not.toContain('createPanelBridgeMessageHandler')
    expect(source).not.toContain("addEventListener('message'")
  })
})
