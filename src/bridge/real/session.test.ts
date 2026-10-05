import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { EventCallback } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getDefaultWorkspaceSession } from '../../shared/constants'
import type { WorkspaceSessionState } from '../../shared/workspace-session-state-types'
import { createSessionRealApi } from './session'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

// Why EventCallback: `subscribeToEvent` forwards Tauri `Event` envelopes, so the
// captured listener is the envelope consumer, not a raw-payload receiver.
type FlushListener = EventCallback<unknown>
let flushListener: FlushListener | null = null

const flushRequestEvent = { event: 'session:flush-requested', id: 0, payload: {} }

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
  flushListener = null
  listenMock.mockImplementation((_event: string, handler: FlushListener) => {
    flushListener = handler
    return Promise.resolve(() => {})
  })
})

describe('session real domain', () => {
  it('get parses the JSON text payload', async () => {
    invokeMock.mockResolvedValue(JSON.stringify({ activeTabId: 't1' }))
    const api = createSessionRealApi()
    const state = await api.session.get()
    expect(invokeMock).toHaveBeenCalledWith('session_get')
    expect(state.activeTabId).toBe('t1')
  })

  // Why read-side normalization: the store keeps sparse rows, but the contract is a
  // full WorkspaceSessionState — unguarded consumers in the hydration chain read
  // top-level fields directly, so a raw `{}` from a first-run empty store would
  // throw (`Object.keys(session.tabsByWorktree)` on undefined).
  it('get merges canonical defaults under stored rows', async () => {
    invokeMock.mockResolvedValue('{"activeTabId":"t1"}')
    const api = createSessionRealApi()
    const state = await api.session.get()
    expect(state.tabsByWorktree).toEqual({})
    expect(state.activeTabId).toBe('t1')
    expect(state).toEqual({ ...getDefaultWorkspaceSession(), activeTabId: 't1' })
  })

  it('get on an empty store returns the full canonical default', async () => {
    invokeMock.mockResolvedValue('{}')
    const api = createSessionRealApi()
    await expect(api.session.get()).resolves.toEqual(getDefaultWorkspaceSession())
  })

  it('patch stringifies the payload into the args envelope', async () => {
    invokeMock.mockResolvedValue(undefined)
    const api = createSessionRealApi()
    await api.session.patch({ activeTabId: 't2' })
    expect(invokeMock).toHaveBeenCalledWith('session_patch', {
      args: JSON.stringify({ activeTabId: 't2' })
    })
  })

  it('set stringifies the full state into the args envelope', async () => {
    invokeMock.mockResolvedValue(undefined)
    const api = createSessionRealApi()
    await api.session.set({ activeTabId: null } as WorkspaceSessionState)
    expect(invokeMock).toHaveBeenCalledWith('session_set', {
      args: JSON.stringify({ activeTabId: null })
    })
  })

  it('flush invokes session_flush', async () => {
    invokeMock.mockResolvedValue(undefined)
    const api = createSessionRealApi()
    await api.session.flush()
    expect(invokeMock).toHaveBeenCalledWith('session_flush')
  })

  it('setSync fires without awaiting and swallows errors', async () => {
    invokeMock.mockRejectedValue(new Error('disk full'))
    const api = createSessionRealApi()
    expect(() => api.session.setSync({ activeTabId: null } as WorkspaceSessionState)).not.toThrow()
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalled())
  })

  it('readTerminalScrollback stays null (deleted domain G refs)', () => {
    expect(createSessionRealApi().session.readTerminalScrollback({ ref: 'v1-x' })).toBeNull()
  })
})

// Why: `registerSessionFlushHandler` keeps module-level singleton state
// (`flushHandler` / `flushSubscriptionStarted`), so the handshake tests load a
// fresh module instance per test — otherwise only the first registration would
// ever subscribe and later tests would fire a never-wired listener.
describe('session flush handshake', () => {
  const loadFreshModule = async (): Promise<typeof import('./session')> => {
    vi.resetModules()
    return import('./session')
  }

  it('runs the registered handler then acks', async () => {
    const { registerSessionFlushHandler } = await loadFreshModule()
    const handler = vi.fn().mockResolvedValue(undefined)
    const unregister = registerSessionFlushHandler(handler)
    await vi.waitFor(() => expect(typeof flushListener).toBe('function'))
    flushListener!(flushRequestEvent)
    await vi.waitFor(() => {
      expect(handler).toHaveBeenCalledTimes(1)
      expect(invokeMock).toHaveBeenCalledWith('session_flush_ack')
    })
    unregister()
  })

  it('acks even when the handler rejects', async () => {
    const { registerSessionFlushHandler } = await loadFreshModule()
    const handler = vi.fn().mockRejectedValue(new Error('capture failed'))
    registerSessionFlushHandler(handler)
    await vi.waitFor(() => expect(typeof flushListener).toBe('function'))
    flushListener!(flushRequestEvent)
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('session_flush_ack'))
  })

  it('unregister drops the handler but acks still flow', async () => {
    const { registerSessionFlushHandler } = await loadFreshModule()
    const handler = vi.fn().mockResolvedValue(undefined)
    const unregister = registerSessionFlushHandler(handler)
    unregister()
    await vi.waitFor(() => expect(typeof flushListener).toBe('function'))
    flushListener!(flushRequestEvent)
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('session_flush_ack'))
    expect(handler).not.toHaveBeenCalled()
  })
})
