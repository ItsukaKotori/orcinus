// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { HostBridge } from '../bridge'
import { PanelApp } from './App'

let root: Root | null = null
let container: HTMLDivElement | null = null

afterEach(() => {
  act(() => root?.unmount())
  container?.remove()
  root = null
  container = null
})

describe('PanelApp', () => {
  it('loads a snapshot and persists manager toggles through set-managers', async () => {
    const invokeCommand = vi.fn(async (commandId: string) => {
      if (commandId === 'snapshot') {
        return {
          ok: true as const,
          value: {
            managers: [
              { id: 'npm', label: 'npm', detected: true, enabled: true, count: 4 },
              { id: 'brew', label: 'Homebrew', detected: false, enabled: false, count: null }
            ],
            state: { phase: 'done', checkedAt: null, managers: [], rows: [] }
          }
        }
      }
      return { ok: true as const, value: null }
    })
    const bridge = {
      ready: Promise.resolve(),
      invokeCommand,
      onPush: () => {},
      onInit: () => {}
    } as unknown as HostBridge
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    await act(async () => {
      root!.render(<PanelApp bridge={bridge} />)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(invokeCommand).toHaveBeenCalledWith('snapshot')
    expect(container.textContent).toContain('Homebrew')
    expect(container.textContent).toContain('not installed')

    // Why the explicit label: `set-managers` replaces the enabled set, so the plan's
    // bare `querySelector('[role="switch"]')` grabbed npm (enabled) and asserted a
    // set that no spec-correct implementation can emit. Target the not-installed
    // Homebrew row (spec §3.7: not-installed managers can still be toggled) and
    // assert the full post-toggle enabled set.
    const toggle = container.querySelector('[role="switch"][aria-label="Homebrew"]')
    expect(toggle).not.toBeNull()
    await act(async () => {
      toggle!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await Promise.resolve()
    })
    expect(invokeCommand).toHaveBeenCalledWith('set-managers', { managers: ['npm', 'brew'] })
  })
})
