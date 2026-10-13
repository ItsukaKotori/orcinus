// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PluginOpenTabRequest } from '../../../preload/api-types'

const { openPluginTabMock } = vi.hoisted(() => ({
  openPluginTabMock: vi.fn()
}))

vi.mock('@/store', () => ({
  useAppStore: {
    getState: () => ({ openPluginTab: openPluginTabMock })
  }
}))

import { PluginOpenTabGate } from './PluginOpenTabGate'

;(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true

let container: HTMLDivElement
let root: Root
let mounted: boolean
let listener: ((request: PluginOpenTabRequest) => void) | null
const unsubscribeMock = vi.fn()

function mountGate(): void {
  act(() => {
    root.render(<PluginOpenTabGate />)
  })
}

function unmountGate(): void {
  if (!mounted) {
    return
  }
  act(() => {
    root.unmount()
  })
  mounted = false
}

function emit(request: PluginOpenTabRequest): void {
  act(() => {
    listener?.(request)
  })
}

beforeEach(() => {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  mounted = true
  listener = null
  openPluginTabMock.mockReset()
  unsubscribeMock.mockReset()
  globalThis.window.api = {
    plugins: {
      onOpenTab: (callback: (request: PluginOpenTabRequest) => void) => {
        listener = callback
        return unsubscribeMock
      }
    }
  } as unknown as Window['api']
})

afterEach(() => {
  unmountGate()
  container.remove()
})

const request: PluginOpenTabRequest = {
  pluginKey: 'acme.plugin',
  tabId: 'dashboard',
  instanceId: 'inst-1',
  title: 'Dashboard',
  params: { query: 'q' },
  worktreeId: 'wt-1',
  scope: 'global'
}

describe('PluginOpenTabGate', () => {
  it('dispatches openPluginTab with the full event payload', () => {
    mountGate()

    emit(request)

    expect(openPluginTabMock).toHaveBeenCalledTimes(1)
    expect(openPluginTabMock).toHaveBeenCalledWith({
      pluginKey: 'acme.plugin',
      tabId: 'dashboard',
      scope: 'global',
      instanceId: 'inst-1',
      title: 'Dashboard',
      params: { query: 'q' },
      worktreeId: 'wt-1'
    })
  })

  it('dispatches every event, letting the store dedupe by instanceId', () => {
    mountGate()

    emit(request)
    emit(request)

    expect(openPluginTabMock).toHaveBeenCalledTimes(2)
    expect(openPluginTabMock).toHaveBeenNthCalledWith(1, expect.objectContaining({ instanceId: 'inst-1' }))
    expect(openPluginTabMock).toHaveBeenNthCalledWith(2, expect.objectContaining({ instanceId: 'inst-1' }))
  })

  it('omits absent optional fields instead of sending undefined', () => {
    mountGate()
    const { title: _title, params: _params, ...bare } = request

    emit(bare)

    expect(openPluginTabMock).toHaveBeenCalledWith({
      pluginKey: 'acme.plugin',
      tabId: 'dashboard',
      scope: 'global',
      instanceId: 'inst-1',
      worktreeId: 'wt-1'
    })
  })

  it('unsubscribes on unmount', () => {
    mountGate()

    expect(unsubscribeMock).not.toHaveBeenCalled()
    unmountGate()

    expect(unsubscribeMock).toHaveBeenCalledTimes(1)
  })
})
