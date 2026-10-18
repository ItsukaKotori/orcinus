// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { HostBridge } from '../bridge'
import type { PluginPanelPushMessage } from '../../../../../src/shared/plugins/plugin-panel-bridge'
import { TabApp } from './App'

function fakeBridge() {
  const pushListeners: Array<(message: PluginPanelPushMessage) => void> = []
  const invokeCommand = vi.fn(async () => ({ ok: true as const, value: null }))
  const bridge = {
    ready: Promise.resolve(),
    invokeCommand,
    onPush: (listener: (message: PluginPanelPushMessage) => void) => {
      pushListeners.push(listener)
    },
    onInit: () => {}
  } as unknown as HostBridge
  return { bridge, invokeCommand, pushListeners }
}

let root: Root | null = null
let container: HTMLDivElement | null = null

afterEach(() => {
  act(() => root?.unmount())
  container?.remove()
  root = null
  container = null
})

describe('TabApp', () => {
  it('checks on mount and renders pushed rows with the live output pane', async () => {
    const { bridge, invokeCommand, pushListeners } = fakeBridge()
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    await act(async () => {
      root!.render(<TabApp bridge={bridge} />)
      await Promise.resolve()
    })
    expect(invokeCommand).toHaveBeenCalledWith('check')

    await act(async () => {
      for (const listener of pushListeners) {
        listener({
          type: 'orca-panel-push',
          topic: 'state',
          payload: {
            phase: 'done',
            checkedAt: Date.now(),
            managers: [
              { id: 'npm', label: 'npm', detected: true, enabled: true, count: 2 }
            ],
            rows: [
              { manager: 'npm', kind: 'global', name: 'typescript', current: '5.8.3', latest: '5.9.2', state: 'pending' },
              { manager: 'npm', kind: 'global', name: 'eslint', current: '9.18.0', latest: '9.21.0', state: 'pending' }
            ]
          }
        })
        listener({
          type: 'orca-panel-push',
          topic: 'log',
          payload: { lines: ['$ npm install -g typescript@latest'] }
        })
      }
    })

    const text = container.textContent ?? ''
    expect(text).toContain('typescript')
    expect(text).toContain('5.9.2')
    expect(text).toContain('Update all (2)')
    expect(text).toContain('npm install -g typescript@latest')
  })
})
