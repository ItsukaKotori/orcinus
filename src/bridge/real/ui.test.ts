import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createUiRealApi, UI_NOOP_SUBSCRIPTION_METHODS } from './ui'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

type UiApi = PreloadApi['ui']

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

describe('ui real adapter commands', () => {
  it('maps get to ui_get without a payload', async () => {
    invokeMock.mockResolvedValueOnce({ activeView: 'settings' })
    await expect(createUiRealApi().get()).resolves.toEqual({ activeView: 'settings' })
    expect(invokeMock).toHaveBeenCalledWith('ui_get')
  })

  it('maps set to ui_set with the { args } envelope', async () => {
    invokeMock.mockResolvedValueOnce(null)
    await expect(createUiRealApi().set({ activeView: 'settings' })).resolves.toBeNull()
    expect(invokeMock).toHaveBeenCalledWith('ui_set', { args: { activeView: 'settings' } })
  })

  it('maps setWithAck to ui_set_with_ack with the { args } envelope', async () => {
    invokeMock.mockResolvedValueOnce(null)
    await createUiRealApi().setWithAck?.({ activeView: 'settings' })
    expect(invokeMock).toHaveBeenCalledWith('ui_set_with_ack', {
      args: { activeView: 'settings' }
    })
  })

  it('maps a setWithAck {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'ui state persist failed' })
    const rejection = createUiRealApi().setWithAck!({ activeView: 'settings' })
    await expect(rejection).rejects.toBeInstanceOf(Error)
    await expect(rejection).rejects.toThrow('ui state persist failed')
  })

  it('maps recordFeatureInteraction to ui_record_feature_interaction with { id }', async () => {
    invokeMock.mockResolvedValueOnce({ featureInteractions: {} })
    await createUiRealApi().recordFeatureInteraction('cmd-j')
    expect(invokeMock).toHaveBeenCalledWith('ui_record_feature_interaction', {
      args: { id: 'cmd-j' }
    })
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'ui state write failed' })
    await expect(createUiRealApi().get()).rejects.toBeInstanceOf(Error)
    invokeMock.mockRejectedValueOnce({ message: 'ui state write failed' })
    await expect(createUiRealApi().get()).rejects.toThrow('ui state write failed')
  })
})

describe('ui real adapter events', () => {
  it('subscribes to ui:stateChanged with the payload and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()
    const state = { activeView: 'settings' }

    const unsubscribe = createUiRealApi().onStateChanged(callback)

    expect(listenMock).toHaveBeenCalledWith('ui:stateChanged', expect.any(Function))
    handlers[0]?.({ payload: state })
    expect(callback).toHaveBeenCalledWith(state)

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it.each(UI_NOOP_SUBSCRIPTION_METHODS)(
    '%s hands back a no-op unsubscriber without listening',
    (method) => {
      const subscribe = createUiRealApi() as unknown as Record<
        string,
        (callback: () => void) => () => void
      >
      const unsubscribe = subscribe[method](() => {})
      expect(typeof unsubscribe).toBe('function')
      expect(listenMock).not.toHaveBeenCalled()
      expect(() => unsubscribe()).not.toThrow()
    }
  )
})

describe('ui real adapter unimplemented surface', () => {
  it.each([
    'onOpenCrashReport',
    'onExportPdfRequested',
    'readClipboardText',
    'popupMenu'
  ] satisfies Array<keyof UiApi>)('rejects %s with UnimplementedBridgeError', async (method) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const ui = createUiRealApi() as unknown as Record<
      string,
      (callArgs?: unknown) => Promise<unknown>
    >
    await expect(ui[method](() => {})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
