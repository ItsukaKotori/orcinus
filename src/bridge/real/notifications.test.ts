// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
const sendNotificationMock = vi.fn()
let permissionGranted = true
let requestResult: 'granted' | 'denied' | 'default' = 'granted'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args)
}))
vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: vi.fn(async () => permissionGranted),
  requestPermission: vi.fn(async () => requestResult),
  sendNotification: (...args: unknown[]) => sendNotificationMock(...args),
  removeActive: vi.fn(async () => undefined)
}))

import {
  buildNotificationCopy,
  createNotificationsRealApi,
  reserveNotificationCooldown
} from './notifications'

function installSettings(notifications: Record<string, unknown>): void {
  ;(globalThis as Record<string, unknown>).__ADE_BOOTSTRAP__ = {
    settings: {
      notifications: {
        enabled: true,
        agentTaskComplete: true,
        terminalBell: true,
        suppressWhenFocused: false,
        customSoundId: 'system',
        customSoundPath: null,
        customSoundVolume: 0.5,
        ...notifications
      }
    },
    platform: { platform: 'darwin' },
    schemaVersion: 1
  }
}

describe('notifications real bridge', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    sendNotificationMock.mockReset()
    permissionGranted = true
    requestResult = 'granted'
    installSettings({})
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterEach(() => {
    delete (globalThis as Record<string, unknown>).__ADE_BOOTSTRAP__
    vi.restoreAllMocks()
  })

  it('dispatches an OS notification when settings and permission allow', async () => {
    const api = createNotificationsRealApi()
    const result = await api.dispatch({
      source: 'agent-task-complete',
      worktreeId: 'r1::/wt',
      paneKey: 't1:leaf',
      worktreeLabel: 'feature/login',
      terminalTitle: 'claude',
      agentState: 'waiting',
      agentPrompt: 'Fix login bug'
    })
    expect(result).toEqual({ delivered: true })
    expect(sendNotificationMock).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'claude', body: expect.stringContaining('Fix login bug') })
    )
  })

  it('gates on disabled/source-disabled/suppressed-focus/cooldown', async () => {
    installSettings({ enabled: false })
    expect(await createNotificationsRealApi().dispatch({ source: 'agent-task-complete' })).toEqual({
      delivered: false,
      reason: 'disabled'
    })
    installSettings({ agentTaskComplete: false })
    expect(await createNotificationsRealApi().dispatch({ source: 'agent-task-complete' })).toEqual({
      delivered: false,
      reason: 'source-disabled'
    })
    installSettings({ suppressWhenFocused: true })
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    expect(
      await createNotificationsRealApi().dispatch({
        source: 'agent-task-complete',
        isActiveWorktree: true,
        worktreeId: 'r-cooldown'
      })
    ).toEqual({ delivered: false, reason: 'suppressed-focus' })
    installSettings({})
    const api = createNotificationsRealApi()
    await api.dispatch({ source: 'agent-task-complete', worktreeId: 'r-cooldown' })
    expect(await api.dispatch({ source: 'agent-task-complete', worktreeId: 'r-cooldown' })).toEqual({
      delivered: false,
      reason: 'cooldown'
    })
  })

  it('reports blocked-by-system when permission stays denied', async () => {
    permissionGranted = false
    requestResult = 'denied'
    const api = createNotificationsRealApi()
    expect(await api.dispatch({ source: 'agent-task-complete' })).toEqual({
      delivered: false,
      reason: 'blocked-by-system'
    })
    expect(await api.probeDelivery()).toEqual({ state: 'blocked', authoritative: false })
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: false
    })
  })

  it('uses the native authoritative readout for permission status and probe', async () => {
    let statusCall = 0
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        statusCall += 1
        return { status: 'authorized', available: true }
      }
      if (command === 'ui_get') {
        return { notificationPermissionRequested: false }
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: false
    })
    expect(await api.probeDelivery()).toEqual({ state: 'delivered', authoritative: true })
    // ensureNativeAvailability + readNativeStatus 各读一次。
    expect(statusCall).toBe(2)
  })

  it('triggers the authorization dialog once per session on not-determined and stamps requested', async () => {
    let requested = false
    let requestCalls = 0
    const uiWrites: unknown[] = []
    invokeMock.mockImplementation(async (command: string, payload?: { args?: unknown }) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: requested ? 'authorized' : 'not-determined', available: true }
      }
      if (command === 'notifications_request_authorization') {
        requestCalls += 1
        requested = true
        return { status: 'authorized', available: true }
      }
      if (command === 'ui_get') {
        return { notificationPermissionRequested: false }
      }
      if (command === 'ui_set') {
        uiWrites.push(payload?.args)
        return payload?.args
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.probeDelivery()).toEqual({ state: 'delivered', authoritative: true })
    expect(await api.probeDelivery()).toEqual({ state: 'delivered', authoritative: true })
    expect(requestCalls).toBe(1)
    expect(uiWrites).toContainEqual({ notificationPermissionRequested: true })
  })

  it('reports denied as authoritative blocked and reads requested from persisted ui state', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'denied', available: true }
      }
      if (command === 'ui_get') {
        return { notificationPermissionRequested: true }
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.probeDelivery()).toEqual({ state: 'blocked', authoritative: true })
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: true
    })
  })

  it('falls back to the plugin probe when the native channel is unavailable', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        throw new Error('unavailable')
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    permissionGranted = false
    requestResult = 'denied'
    const api = createNotificationsRealApi()
    expect(await api.probeDelivery()).toEqual({ state: 'blocked', authoritative: false })
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: false
    })
  })

  it('delivers through the native channel with a stable identifier and system-sound rule', async () => {
    const deliverCalls: unknown[] = []
    invokeMock.mockImplementation(async (command: string, payload?: { args?: unknown }) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'authorized', available: true }
      }
      if (command === 'notifications_deliver_native') {
        deliverCalls.push(payload?.args)
        return { ok: true }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    installSettings({ customSoundId: 'two-tone' })
    const api = createNotificationsRealApi()
    const result = await api.dispatch({
      source: 'agent-task-complete',
      worktreeId: 'r-native',
      notificationId: 'agent:r-native:t1:leaf:100',
      terminalTitle: 'claude',
      agentState: 'waiting',
      agentPrompt: 'Fix login bug'
    })
    expect(result).toEqual({ delivered: true })
    expect(sendNotificationMock).not.toHaveBeenCalled()
    expect(deliverCalls[0]).toEqual({
      id: 'agent:r-native:t1:leaf:100',
      title: 'claude',
      body: 'Fix login bug',
      silent: true
    })
    installSettings({ customSoundId: 'system' })
    await createNotificationsRealApi().dispatch({
      source: 'agent-task-complete',
      worktreeId: 'r-native-2',
      notificationId: 'agent:r-native-2:t1:leaf:200'
    })
    expect(deliverCalls[1]).toMatchObject({ silent: false })
  })

  it('returns blocked-by-system for denied and not-displayed for native delivery failure', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'denied', available: true }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    expect(
      await createNotificationsRealApi().dispatch({
        source: 'agent-task-complete',
        worktreeId: 'r-denied'
      })
    ).toEqual({ delivered: false, reason: 'blocked-by-system' })

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'authorized', available: true }
      }
      if (command === 'notifications_deliver_native') {
        return { ok: false, error: 'boom' }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    expect(
      await createNotificationsRealApi().dispatch({
        source: 'agent-task-complete',
        worktreeId: 'r-failed'
      })
    ).toEqual({ delivered: false, reason: 'not-displayed' })
  })

  it('fires the authorization request on not-determined and returns blocked, delivering on the next call', async () => {
    let requested = false
    const commands: string[] = []
    invokeMock.mockImplementation(async (command: string) => {
      commands.push(command)
      if (command === 'notifications_get_authorization_status') {
        return { status: requested ? 'authorized' : 'not-determined', available: true }
      }
      if (command === 'notifications_request_authorization') {
        requested = true
        return { status: 'not-determined', available: true }
      }
      if (command === 'notifications_deliver_native') {
        return { ok: true }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(
      await api.dispatch({ source: 'agent-task-complete', worktreeId: 'r-ask' })
    ).toEqual({ delivered: false, reason: 'blocked-by-system' })
    expect(commands).toContain('notifications_request_authorization')
    expect(commands).not.toContain('notifications_deliver_native')
    expect(
      await api.dispatch({ source: 'agent-task-complete', worktreeId: 'r-ask-2' })
    ).toEqual({ delivered: true })
    expect(commands).toContain('notifications_deliver_native')
  })

  it('dismisses through the native channel with raw ids and counts the intersection', async () => {
    const dismissCalls: unknown[] = []
    invokeMock.mockImplementation(async (command: string, payload?: { args?: unknown }) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'authorized', available: true }
      }
      if (command === 'notifications_dismiss_native') {
        dismissCalls.push(payload?.args)
        return { dismissed: 1 }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.dismiss(['agent:r1:t1:leaf:100'])).toEqual({ dismissed: 1 })
    expect(dismissCalls[0]).toEqual({ ids: ['agent:r1:t1:leaf:100'] })
  })

  it('builds deterministic copy and reserves cooldown per worktree', () => {
    expect(
      buildNotificationCopy({
        source: 'agent-task-complete',
        terminalTitle: 'claude',
        worktreeLabel: 'feature/login',
        agentState: 'waiting',
        agentPrompt: 'Fix login bug'
      })
    ).toEqual({ title: 'claude', body: 'Fix login bug' })
    const map = new Map<string, number>()
    expect(reserveNotificationCooldown(map, 'w1', 1000)).toBe(true)
    expect(reserveNotificationCooldown(map, 'w1', 2000)).toBe(false)
    expect(reserveNotificationCooldown(map, 'w1', 7000)).toBe(true)
  })

  it('plays custom sounds through the host reader and dedupes while playing', async () => {
    installSettings({ customSoundId: 'custom', customSoundPath: '/tmp/ding.wav' })
    invokeMock.mockResolvedValueOnce({
      ok: true,
      dataBase64: 'UklGRg==',
      mimeType: 'audio/wav',
      path: '/tmp/ding.wav'
    })
    const played: string[] = []
    const constructedAudios: Array<{ volume: number }> = []
    class FakeAudio {
      volume = 1
      source: string
      onended: (() => void) | null = null
      onerror: (() => void) | null = null
      constructor(src: string) {
        this.source = src
        constructedAudios.push(this)
      }
      play(): Promise<void> {
        played.push('play')
        this.onended?.()
        return Promise.resolve()
      }
    }
    vi.stubGlobal('Audio', FakeAudio)
    vi.stubGlobal('URL', {
      createObjectURL: () => 'blob:fake',
      revokeObjectURL: () => {}
    })
    const api = createNotificationsRealApi()
    expect(await api.playSound({ volume: 30 })).toEqual({ played: true })
    expect(played).toEqual(['play'])
    expect(constructedAudios[0]?.volume).toBe(0.3)
    vi.unstubAllGlobals()
  })

  it('plays built-in sounds from bundled assets, dedupes while playing and honors force', async () => {
    installSettings({ customSoundId: 'two-tone' })
    const sources: string[] = []
    const volumes: number[] = []
    const pending: Array<() => void> = []
    class FakeAudio {
      volume = 1
      onended: (() => void) | null = null
      onerror: (() => void) | null = null
      constructor(src: string) {
        sources.push(src)
      }
      play(): Promise<void> {
        volumes.push(this.volume)
        pending.push(() => this.onended?.())
        return Promise.resolve()
      }
    }
    vi.stubGlobal('Audio', FakeAudio)
    const api = createNotificationsRealApi()
    // 第一次调用同步注册在播集合（playAudio 之前的 add 是同步的）。
    const first = api.playSound({ volume: 60 })
    expect(await api.playSound({ volume: 60 })).toEqual({ played: false, reason: 'deduped' })
    const forced = api.playSound({ volume: 60, force: true })
    pending.forEach((finish) => finish())
    expect(await first).toEqual({ played: true })
    expect(await forced).toEqual({ played: true })
    expect(sources).toEqual([expect.stringMatching(/two-tone/), expect.stringMatching(/two-tone/)])
    expect(volumes[0]).toBe(0.6)
    vi.unstubAllGlobals()
  })

  it('returns missing-path for system and unknown ids', async () => {
    installSettings({ customSoundId: 'system' })
    expect(await createNotificationsRealApi().playSound({})).toEqual({
      played: false,
      reason: 'missing-path'
    })
    installSettings({ customSoundId: 'not-a-sound' as never })
    expect(await createNotificationsRealApi().playSound({})).toEqual({
      played: false,
      reason: 'missing-path'
    })
  })
})
