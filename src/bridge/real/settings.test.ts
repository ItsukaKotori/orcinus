import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SettingsApi } from '../../shared/preload-api/api/settings-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createSettingsRealApi } from './settings'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type SettingsMethod = keyof SettingsApi

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

describe('settings real adapter commands', () => {
  it('maps get to settings_get without a payload', async () => {
    invokeMock.mockResolvedValueOnce({ theme: 'dark' })
    await expect(createSettingsRealApi().get()).resolves.toEqual({ theme: 'dark' })
    expect(invokeMock).toHaveBeenCalledWith('settings_get')
  })

  it('maps set to settings_set with the { args } envelope', async () => {
    invokeMock.mockResolvedValueOnce({ theme: 'light' })
    await expect(createSettingsRealApi().set({ theme: 'light' })).resolves.toEqual({
      theme: 'light'
    })
    expect(invokeMock).toHaveBeenCalledWith('settings_set', { args: { theme: 'light' } })
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'settings write failed' })
    const rejection = createSettingsRealApi().get()
    await expect(rejection).rejects.toBeInstanceOf(Error)
    await expect(rejection).rejects.toThrow('settings write failed')
  })
})

describe('settings real adapter events', () => {
  it('subscribes to settings:changed with the partial payload and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()

    const unsubscribe = createSettingsRealApi().onChanged(callback)

    expect(listenMock).toHaveBeenCalledWith('settings:changed', expect.any(Function))
    handlers[0]?.({ payload: { theme: 'light' } })
    expect(callback).toHaveBeenCalledWith({ theme: 'light' })

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})

describe('settings real adapter unimplemented surface', () => {
  it.each([
    'setActiveRuntimeEnvironmentPreference',
    'updatePRBotAuthorOverride',
    'listFonts',
    'previewGhosttyImport',
    'previewWarpThemeImport'
  ] satisfies SettingsMethod[])('rejects %s with UnimplementedBridgeError', async (method) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const settings = createSettingsRealApi() as unknown as Record<
      string,
      (callArgs?: unknown) => Promise<unknown>
    >
    await expect(settings[method]({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
