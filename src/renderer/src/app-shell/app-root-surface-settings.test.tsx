// @vitest-environment happy-dom

import { act, renderHook } from '@testing-library/react'
import { useStore } from 'zustand'
import { createStore } from 'zustand/vanilla'
import { describe, expect, it } from 'vitest'
import { getDefaultSettings } from '../../../shared/constants'
import { selectAppRootSurfaceTelemetryOptedIn } from './app-root-surface-settings'

type SurfaceState = Parameters<typeof selectAppRootSurfaceTelemetryOptedIn>[0]

describe('app root surface settings selectors', () => {
  it('does not rerender for an unrelated settings replacement', () => {
    const store = createStore<SurfaceState>(() => ({ settings: getDefaultSettings('/tmp') }))
    let renderCount = 0
    const view = renderHook(() => {
      renderCount += 1
      return useStore(store, selectAppRootSurfaceTelemetryOptedIn)
    })

    expect(renderCount).toBe(1)
    act(() => {
      const settings = store.getState().settings!
      store.setState({ settings: { ...settings, editorAutoSave: !settings.editorAutoSave } })
    })

    expect(renderCount).toBe(1)
    expect(view.result.current).toBe('unknown')
    view.unmount()
  })

  it('still rerenders when a setting used by a surface changes', () => {
    const store = createStore<SurfaceState>(() => ({ settings: getDefaultSettings('/tmp') }))
    let renderCount = 0
    const view = renderHook(() => {
      renderCount += 1
      return useStore(store, selectAppRootSurfaceTelemetryOptedIn)
    })

    act(() => {
      const settings = store.getState().settings!
      store.setState({
        settings: {
          ...settings,
          telemetry: { optedIn: true, installId: 'test-install', existedBeforeTelemetryRelease: false }
        }
      })
    })

    expect(renderCount).toBe(2)
    expect(view.result.current).toBe(true)
    view.unmount()
  })
})
