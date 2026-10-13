// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Tab } from '../../../../shared/tab-types'
import { parsePanelInitMessage } from '../../../../shared/plugins/plugin-panel-bridge'

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => fallback
}))

vi.mock('../right-sidebar/plugin-panel-watchdog', () => ({
  createPanelWatchdog: () => ({ start: vi.fn(), stop: vi.fn(), handlePong: vi.fn() })
}))

import PluginTabContent from './PluginTabContent'

;(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true

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
    scope: 'project',
    repoId: 'repo-1',
    instanceId: 'inst-1',
    titleHint: 'Board',
    params: { view: 'open' }
  }
}

let container: HTMLDivElement
let root: Root
const readTabEntryMock = vi.fn()
const panelActionMock = vi.fn()
let pluginChangedListener: (() => void) | null
const SESSION_TOKEN = 's'.repeat(43)

function waitForHappyDomTasks(): Promise<void> {
  return (
    window as unknown as { happyDOM: { waitUntilComplete: () => Promise<void> } }
  ).happyDOM.waitUntilComplete()
}

beforeEach(() => {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  readTabEntryMock.mockReset()
  panelActionMock.mockReset()
  panelActionMock.mockResolvedValue({ ok: true, value: null })
  pluginChangedListener = null
  globalThis.window.api = {
    plugins: {
      readTabEntry: readTabEntryMock,
      panelAction: panelActionMock,
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

async function renderContent(tab: Tab = pluginTab): Promise<void> {
  await act(async () => {
    root.render(<PluginTabContent tab={tab} onClose={vi.fn()} />)
    await waitForHappyDomTasks()
  })
}

async function dispatchPong(iframe: HTMLIFrameElement): Promise<void> {
  await act(async () => {
    const event = new MessageEvent('message', {
      data: { type: 'orca-panel-pong', pingId: 0 }
    })
    Object.defineProperty(event, 'source', { value: iframe.contentWindow })
    window.dispatchEvent(event)
    await waitForHappyDomTasks()
  })
}

describe('PluginTabContent session', () => {
  it('loads the tab entry and posts params as orca-panel-init after the shell pong', async () => {
    readTabEntryMock.mockResolvedValue({
      html: '<h1>Tab plugin</h1>',
      sessionToken: SESSION_TOKEN
    })

    await renderContent()

    expect(readTabEntryMock).toHaveBeenCalledWith({
      pluginKey: 'plugin.one',
      tabId: 'dashboard',
      instanceId: 'inst-1',
      worktreeId: 'wt-1',
      ownerWindow: 'main',
      scope: 'project',
      repoId: 'repo-1'
    })
    const iframe = container.querySelector('iframe')
    expect(iframe?.getAttribute('srcdoc')).toContain('<h1>Tab plugin</h1>')
    expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts')

    const received: unknown[] = []
    iframe!.contentWindow?.addEventListener('message', (event) => {
      received.push((event as MessageEvent).data)
    })

    await dispatchPong(iframe!)

    const initFrame = received.find(
      (data) => parsePanelInitMessage(data).ok && (data as { type: string }).type === 'orca-panel-init'
    )
    expect(initFrame).toBeDefined()
    expect(parsePanelInitMessage(initFrame)).toEqual({
      ok: true,
      params: { view: 'open' }
    })
  })

  it('posts no init frame when the binding carries no params', async () => {
    readTabEntryMock.mockResolvedValue({
      html: '<h1>Tab plugin</h1>',
      sessionToken: SESSION_TOKEN
    })

    await renderContent({
      ...pluginTab,
      pluginTab: { ...pluginTab.pluginTab!, params: undefined }
    })

    const iframe = container.querySelector('iframe')
    const received: unknown[] = []
    iframe!.contentWindow?.addEventListener('message', (event) => {
      received.push((event as MessageEvent).data)
    })

    await dispatchPong(iframe!)

    expect(received).toEqual([])
  })

  it('re-reads the entry when plugins.onChanged fires', async () => {
    readTabEntryMock.mockResolvedValueOnce({
      html: '<h1>Tab plugin</h1>',
      sessionToken: SESSION_TOKEN
    })
    await renderContent()

    readTabEntryMock.mockResolvedValue({
      html: '<h1>Reloaded tab plugin</h1>',
      sessionToken: SESSION_TOKEN
    })
    await act(async () => {
      pluginChangedListener?.()
      await waitForHappyDomTasks()
    })

    expect(container.querySelector('iframe')?.getAttribute('srcdoc')).toContain(
      'Reloaded tab plugin'
    )
  })

  it('renders a closeable unavailable state when the entry read returns null', async () => {
    readTabEntryMock.mockResolvedValue(null)
    const onClose = vi.fn()

    await act(async () => {
      root.render(<PluginTabContent tab={pluginTab} onClose={onClose} />)
      await waitForHappyDomTasks()
    })

    expect(container.querySelector('iframe')).toBeNull()
    expect(container.textContent).toContain('该插件 Tab 不可用（插件可能已禁用或卸载）。')
    ;(container.querySelector('button') as HTMLButtonElement).click()
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
