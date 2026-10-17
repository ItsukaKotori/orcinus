import { afterEach, describe, expect, it, vi } from 'vitest'
import { dispatchAppCommand, registerAppCommandDispatcher } from './app-command-dispatch'

let cleanup: (() => void) | null = null

afterEach(() => cleanup?.())

describe('app command dispatch', () => {
  it('routes known action ids through the current handler registry', () => {
    const dispatcher = vi.fn(() => true)
    cleanup = registerAppCommandDispatcher(dispatcher)

    expect(dispatchAppCommand('app.settings', 'plugin-palette')).toBe(true)
    expect(dispatcher).toHaveBeenCalledWith('app.settings', 'plugin-palette')
  })

  it('rejects unknown actions and unregisters only the current dispatcher', () => {
    const staleCleanup = registerAppCommandDispatcher(() => true)
    const current = vi.fn(() => true)
    cleanup = registerAppCommandDispatcher(current)
    staleCleanup()

    expect(dispatchAppCommand('missing.action', 'plugin-keybinding')).toBe(false)
    expect(dispatchAppCommand('app.settings', 'plugin-keybinding')).toBe(true)
    cleanup()
    cleanup = null
    expect(dispatchAppCommand('app.settings', 'plugin-keybinding')).toBe(false)
  })
})
