// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest'
import { registerHostShortcutFallback } from './host-shortcut-fallback'
import { useAppStore } from '@/store'

const CODES: Record<string, string> = { p: 'KeyP', j: 'KeyJ', ',': 'Comma' }

function dispatchShortcut(key: string): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key,
    code: CODES[key] ?? '',
    metaKey: true,
    ctrlKey: true,
    bubbles: true,
    cancelable: true
  })
  window.dispatchEvent(event)
  return event
}

describe('host shortcut fallback', () => {
  let unregister: (() => void) | null = null

  afterEach(() => {
    unregister?.()
    unregister = null
    useAppStore.setState({ activeModal: 'none', activeView: 'terminal' })
  })

  it('opens quick open from the terminal view', () => {
    unregister = registerHostShortcutFallback()
    useAppStore.setState({ activeView: 'terminal', activeWorktreeId: 'repo::/x', activeModal: 'none' })
    const event = dispatchShortcut('p')
    expect(event.defaultPrevented).toBe(true)
    expect(useAppStore.getState().activeModal).toBe('quick-open')
  })

  it('ignores quick open outside the terminal view', () => {
    unregister = registerHostShortcutFallback()
    useAppStore.setState({ activeView: 'settings', activeWorktreeId: 'repo::/x', activeModal: 'none' })
    const event = dispatchShortcut('p')
    expect(event.defaultPrevented).toBe(false)
    expect(useAppStore.getState().activeModal).toBe('none')
  })

  it('toggles the worktree palette', () => {
    unregister = registerHostShortcutFallback()
    useAppStore.setState({ activeModal: 'none' })
    dispatchShortcut('j')
    expect(useAppStore.getState().activeModal).toBe('worktree-palette')
    dispatchShortcut('j')
    expect(useAppStore.getState().activeModal).toBe('none')
  })

  it('opens settings', () => {
    unregister = registerHostShortcutFallback()
    useAppStore.setState({ activeView: 'terminal' })
    dispatchShortcut(',')
    expect(useAppStore.getState().activeView).toBe('settings')
  })

  it('stops listening after unregister', () => {
    unregister = registerHostShortcutFallback()
    unregister()
    unregister = null
    useAppStore.setState({ activeView: 'terminal', activeWorktreeId: 'repo::/x', activeModal: 'none' })
    dispatchShortcut('p')
    expect(useAppStore.getState().activeModal).toBe('none')
  })
})
