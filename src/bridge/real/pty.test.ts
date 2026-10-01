import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EMPTY_PTY_MAIN_DELIVERY_DIAGNOSTICS } from '../../shared/pty-delivery-diagnostics'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

const DATA_ENDPOINT = { port: 51234, token: 'tok' }

/** Controllable WebSocket stub: tests drive open/message/close by hand. */
class FakeWebSocket {
  static instances: FakeWebSocket[] = []
  binaryType: string = 'blob'
  readyState: number = 0 // CONNECTING
  onopen: (() => void) | null = null
  onmessage: ((event: { data: ArrayBuffer }) => void) | null = null
  onclose: ((event: { code: number }) => void) | null = null
  onerror: (() => void) | null = null
  sent: unknown[] = []
  closedViaCloseMethod = false
  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this)
  }
  send(data: unknown): void {
    this.sent.push(data)
  }
  close(): void {
    this.closedViaCloseMethod = true
    this.readyState = 3
  }
  open(): void {
    this.readyState = 1
    this.onopen?.()
  }
  message(text: string): void {
    this.onmessage?.({ data: new TextEncoder().encode(text).buffer as ArrayBuffer })
  }
  drop(code = 1006): void {
    this.readyState = 3
    this.onclose?.({ code })
  }
}

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
  listenMock.mockResolvedValue(() => {})
  vi.resetModules()
  FakeWebSocket.instances = []
  vi.stubGlobal('WebSocket', FakeWebSocket)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function mockPtyCommands(handlers: Record<string, (args?: unknown) => unknown>): void {
  invokeMock.mockImplementation(async (command: string, request?: unknown) => {
    const handler = handlers[command]
    if (!handler) throw new Error(`unexpected command: ${command}`)
    return handler((request as { args?: unknown } | undefined)?.args)
  })
}

function mockSpawn(reply: unknown): void {
  mockPtyCommands({ pty_data_endpoint: () => DATA_ENDPOINT, pty_spawn: () => reply })
}

async function importFresh() {
  return await import('./pty')
}

const spawnOpts = { cols: 80, rows: 24 }

/** API with one spawned session whose socket has landed; `ws` is that socket. */
async function spawnedApi(reply: unknown = { id: 'pty-1' }) {
  mockSpawn(reply)
  const { createPtyRealApi } = await importFresh()
  const api = createPtyRealApi()
  await api.spawn(spawnOpts)
  await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1))
  return { api, ws: FakeWebSocket.instances[0] as FakeWebSocket }
}

function tauriHandler(event: string): (payload: unknown) => void {
  const call = listenMock.mock.calls.find(([name]) => name === event)
  if (!call) throw new Error(`no listener registered for ${event}`)
  return call[1] as (payload: unknown) => void
}

describe('spawn', () => {
  it('spawn_invokes_pty_spawn_with_args_wrapper', async () => {
    mockSpawn({ id: 'pty-1' })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    // 契约-only 字段（launchToken 等）整包透传，Rust serde 忽略未知（规格 §2.4）。
    const opts = { cols: 120, rows: 40, cwd: '/repo', command: 'echo hi', launchToken: 'tok' }
    await expect(api.spawn(opts)).resolves.toEqual({ id: 'pty-1' })
    expect(invokeMock).toHaveBeenCalledWith('pty_spawn', { args: opts })
  })

  it('returns the reattach reply verbatim when sessionId hits a live session', async () => {
    const { api } = await spawnedApi({ id: 's1', isReattach: true })
    await expect(api.spawn({ ...spawnOpts, sessionId: 's1' })).resolves.toEqual({
      id: 's1',
      isReattach: true
    })
  })
})

describe('onData', () => {
  it('on_data_fans_out_from_ws_frames', async () => {
    const { api, ws } = await spawnedApi()
    const received: Array<Record<string, unknown>> = []
    const stopFirst = api.onData((data) => received.push(data))
    api.onData((data) => received.push({ ...data, second: true }))
    expect(api.getPtyDataListenerCount()).toBe(2)
    ws.message('héllo') // 多字节 utf-8：rawLength 记原始字节数
    expect(received).toEqual([
      { id: 'pty-1', data: 'héllo', rawLength: 6 },
      { id: 'pty-1', data: 'héllo', rawLength: 6, second: true }
    ])
    stopFirst()
    stopFirst() // 重复退订安全
    expect(api.getPtyDataListenerCount()).toBe(1)
  })
})

describe('exit semantics', () => {
  it('ws_abnormal_close_emits_local_exit', async () => {
    const { api, ws } = await spawnedApi()
    const exits: unknown[] = []
    api.onExit((data) => exits.push(data))
    ws.drop(1006)
    expect(exits).toEqual([{ id: 'pty-1', code: -1 }])
  })

  it('a clean close(1000) leaves the exit code to the pty:exit event', async () => {
    const { api, ws } = await spawnedApi()
    const exits: unknown[] = []
    api.onExit((data) => exits.push(data))
    ws.drop(1000)
    expect(exits).toEqual([])
  })

  it('tauri_exit_event_closes_ws_and_forwards', async () => {
    const { api, ws } = await spawnedApi()
    const exits: unknown[] = []
    const unlisten = vi.fn()
    listenMock.mockResolvedValue(unlisten)
    const stop = api.onExit((data) => exits.push(data))
    tauriHandler('pty:exit')({ payload: { id: 'pty-1', code: 0 } })
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
    expect(ws.closedViaCloseMethod).toBe(true)
    await expect(api.writeAccepted('pty-1', 'x')).resolves.toBe(false)
    stop()
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1))
    stop() // 重复退订安全
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('exit landing during the endpoint await drops the late socket (no duplicate death)', async () => {
    // Task 12 Minor-1 竞态：openPtySocket await 端点期间 pty:exit 到达（规格 §3.2
    // 窗口）。墓碑让晚到的 open 丢弃连接，而不是连上已死会话再广播假的 code:-1。
    let releaseEndpoint: (endpoint: unknown) => void = () => {}
    const endpointGate = new Promise((resolve) => {
      releaseEndpoint = resolve
    })
    mockPtyCommands({
      pty_data_endpoint: () => endpointGate,
      pty_spawn: () => ({ id: 'pty-1' })
    })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    const exits: unknown[] = []
    api.onExit((data) => exits.push(data))
    await api.spawn(spawnOpts)
    expect(FakeWebSocket.instances).toHaveLength(0) // 端点仍 gated：socket 未落地
    tauriHandler('pty:exit')({ payload: { id: 'pty-1', code: 0 } })
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
    releaseEndpoint(DATA_ENDPOINT)
    await new Promise((resolve) => setTimeout(resolve, 0)) // 让 open 走完墓碑检查
    expect(FakeWebSocket.instances).toHaveLength(0) // 晚到的连接被墓碑丢弃
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }]) // 无 code:-1 的重复死亡
    // 同 id 重生：墓碑已被消费，新 spawn 正常落地连接。
    await api.spawn(spawnOpts)
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1))
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
  })

  it('duplicate exit deliveries after the socket landed leave no tombstone (respawn stays live)', async () => {
    // 审查 fix round 1：真实渲染层 ≥2 个 onExit 订阅者（pty-dispatcher、
    // use-resource-session-inventory，subscribeToEvent 不去重），Tauri 逐订阅者
    // 投递——第 1 个摘表，第 2+ 个全命中 close-miss。此时没有 open 在飞，不得
    // 立碑；否则 Phase 2 的 id 复用/reattach 会被残留墓碑静默吞掉活连接。
    const { api, ws } = await spawnedApi()
    api.onExit(() => {})
    api.onExit(() => {})
    const exits: unknown[] = []
    api.onExit((data) => exits.push(data))
    const exitHandlers = listenMock.mock.calls
      .filter(([name]) => name === 'pty:exit')
      .map(([, handler]) => handler as (message: { payload: unknown }) => void)
    expect(exitHandlers.length).toBeGreaterThanOrEqual(2)
    for (const handler of exitHandlers) handler({ payload: { id: 'pty-1', code: 0 } })
    expect(ws.closedViaCloseMethod).toBe(true)
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
    // 同 id 重生：无墓碑残留，新连接正常落地（旧实现此处 open 被静默吞掉）。
    mockSpawn({ id: 'pty-1' })
    await api.spawn(spawnOpts)
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2))
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
  })
})

describe('write', () => {
  it('write_sends_binary_frame_and_write_accepted_true', async () => {
    const { api, ws } = await spawnedApi()
    ws.open()
    api.write('pty-1', 'abc')
    expect(ws.sent[0]).toEqual(new TextEncoder().encode('abc'))
    await expect(api.writeAccepted('pty-1', 'déjà')).resolves.toBe(true)
    expect(ws.sent[1]).toEqual(new TextEncoder().encode('déjà'))
  })

  it('drops writes and refuses writeAccepted while the socket is not ready', async () => {
    let spawnCall = 0
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'pty_data_endpoint') return DATA_ENDPOINT
      if (command === 'pty_spawn') return { id: spawnCall++ === 0 ? 'pty-1' : 'pty-2' }
      throw new Error(`unexpected command: ${command}`)
    })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    await api.spawn(spawnOpts)
    await api.spawn(spawnOpts)
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2))
    const connecting = FakeWebSocket.instances[1] as FakeWebSocket
    expect(connecting.readyState).toBe(0)
    expect(() => api.write('pty-2', 'x')).not.toThrow()
    expect(connecting.sent).toEqual([])
    await expect(api.writeAccepted('pty-2', 'x')).resolves.toBe(false)
    expect(connecting.sent).toEqual([])
    connecting.open()
    await expect(api.writeAccepted('pty-2', 'x')).resolves.toBe(true)
    // 未知会话（无 socket）：静默丢 / false
    expect(() => api.write('missing', 'x')).not.toThrow()
    await expect(api.writeAccepted('missing', 'x')).resolves.toBe(false)
  })
})

describe('web-stub shaped methods (web-terminal-api.ts verbatim)', () => {
  it('stub 五件套逐字缺省且零 IPC', async () => {
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    await expect(api.getForegroundProcess('p1')).resolves.toBeNull()
    await expect(api.confirmForegroundProcess('p1')).resolves.toBeNull()
    await expect(api.hasChildProcesses('p1')).resolves.toBe(false)
    await expect(api.getMainBufferSnapshot('p1')).resolves.toBeNull()
    await expect(api.getAuthoritativeBufferSnapshotCapabilities?.(['a', 'b'])).resolves.toEqual([
      { id: 'a', authoritative: false },
      { id: 'b', authoritative: false }
    ])
    await expect(api.inspectProcess('p1')).rejects.toThrow('terminal_liveness_unavailable')
    await expect(api.reportRendererDeliveryState({} as never)).resolves.toEqual({
      inFlightTotalChars: 0,
      inFlightPtyCount: 0,
      msSinceLastAck: null
    })
    await expect(api.getRendererDeliveryDebugSnapshot()).resolves.toEqual({
      pendingPtyCount: 0,
      pendingChars: 0,
      maxPendingCharsByPty: 0,
      rendererInFlightPtyCount: 0,
      rendererInFlightChars: 0,
      maxRendererInFlightCharsByPty: 0,
      activeRendererPtyCount: 0,
      flushScheduled: false,
      peakPendingChars: 0,
      peakMaxPendingCharsByPty: 0,
      peakRendererInFlightChars: 0,
      peakMaxRendererInFlightCharsByPty: 0,
      ackGatedFlushSkipCount: 0,
      hiddenDeliveryGatedPtyCount: 0,
      hiddenDeliveryGatedVisiblePtyCount: 0,
      hiddenDeliveryGatedActivePtyCount: 0,
      deliveryInterestPtyCount: 0,
      hiddenDeliveryDroppedChars: 0,
      hiddenDeliveryDroppedChunks: 0,
      pendingDroppedChars: 0,
      diagnostics: EMPTY_PTY_MAIN_DELIVERY_DIAGNOSTICS,
      rendererLifecycleResetCount: 0,
      lastLifecycleResetClearedChars: 0,
      rendererPtyDispatcherReady: false,
      rendererDispatcherReadyForcedCount: 0
    })
    expect(invokeMock).not.toHaveBeenCalled()
  })
})

describe('noop set', () => {
  it('subscriptions return repeat-safe unsubscribers and no-op methods resolve', async () => {
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    const unsubscribers = [
      api.onDeliveryResyncRequest(() => {}),
      api.onReplay(() => {}),
      api.onModelRestoreNeeded(() => {}),
      api.onSideEffect(() => {}),
      api.onSerializeBufferRequest(() => {}),
      api.onClearBufferRequest(() => {}),
      api.onWriteUnavailable?.(() => {})
    ]
    for (const stop of unsubscribers) {
      expect(typeof stop).toBe('function')
      stop?.()
      stop?.() // 重复退订安全
    }
    expect(() => {
      api.ackData('p1', 10)
      api.ackColdRestore('p1')
      api.claimViewport('p1', 80, 24)
      api.reportGeometry('p1', 80, 24)
      api.setActiveRendererPty('p1', true)
      api.setRendererPtyVisible('p1', true)
      api.setHiddenRendererPty('p1', true)
      api.setPtyDeliveryInterest('p1', true)
      api.publishTerminalViewAttributes({} as never)
      api.respondDeliveryResync({ requestId: 1, processedCharsByPty: {} })
      api.sendSerializedBuffer('req', null)
      api.rendererDispatcherReady()
    }).not.toThrow()
    await expect(api.declarePendingPaneSerializer('pane')).resolves.toBe(0)
    await expect(api.settlePaneSerializer('pane', 0)).resolves.toBeUndefined()
    await expect(api.clearPendingPaneSerializer('pane', 0)).resolves.toBeUndefined()
    await expect(api.reportRendererSerializerReady?.('p1')).resolves.toBeUndefined()
    await expect(api.getSideEffectSnapshot('p1')).resolves.toBeNull()
    await expect(api.resetRendererDeliveryDebug()).resolves.toBeUndefined()
    expect(invokeMock).not.toHaveBeenCalled()
  })
})

describe('invoke passthrough', () => {
  it('list_sessions 形状透传', async () => {
    const rows = [{ id: 'p1', cwd: '/repo', title: '', agentOwnership: 'unknown' }]
    mockPtyCommands({ pty_list_sessions: () => rows })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    await expect(api.listSessions()).resolves.toEqual(rows)
    expect(invokeMock).toHaveBeenCalledWith('pty_list_sessions', { args: { scope: undefined } })
    const scope = { connectionId: null }
    await expect(api.listSessions(scope)).resolves.toEqual(rows)
    expect(invokeMock).toHaveBeenLastCalledWith('pty_list_sessions', { args: { scope } })
  })

  it('routes the control plane and management through their commands', async () => {
    const seen: Array<[string, unknown]> = []
    mockPtyCommands({
      pty_resize: (args) => {
        seen.push(['pty_resize', args])
        return null
      },
      pty_signal: (args) => {
        seen.push(['pty_signal', args])
        return null
      },
      pty_clear_buffer: (args) => {
        seen.push(['pty_clear_buffer', args])
        return null
      },
      pty_kill: (args) => {
        seen.push(['pty_kill', args])
        return null
      },
      pty_get_cwd: () => '/repo',
      pty_get_size: () => ({ cols: 80, rows: 24 }),
      pty_has_pty: () => true,
      pty_management_list_sessions: () => ({ sessions: [], degraded: false }),
      pty_management_kill_all: () => ({ killedCount: 1, remainingCount: 0, killedSessionIds: ['p1'] }),
      pty_management_kill_one: () => ({ success: true }),
      pty_management_restart: () => ({ success: true }),
      pty_management_mac_tcc_attribution: () => ({ health: 'unknown' })
    })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    api.resize('p1', 100, 30)
    api.signal('p1', 'SIGTERM')
    api.clearBuffer('p1')
    expect(seen).toEqual([
      ['pty_resize', { id: 'p1', cols: 100, rows: 30 }],
      ['pty_signal', { id: 'p1', signal: 'SIGTERM' }],
      ['pty_clear_buffer', { id: 'p1' }]
    ])
    await api.kill('p1', { keepHistory: true })
    await api.kill('p2')
    expect(seen.slice(3)).toEqual([
      ['pty_kill', { id: 'p1', keepHistory: true }],
      ['pty_kill', { id: 'p2', keepHistory: undefined }]
    ])
    await expect(api.getCwd('p1')).resolves.toBe('/repo')
    await expect(api.getSize('p1')).resolves.toEqual({ cols: 80, rows: 24 })
    await expect(api.hasPty('p1')).resolves.toBe(true)
    await expect(api.management.listSessions()).resolves.toEqual({ sessions: [], degraded: false })
    await expect(api.management.killAll()).resolves.toEqual({
      killedCount: 1,
      remainingCount: 0,
      killedSessionIds: ['p1']
    })
    await expect(api.management.killOne({ sessionId: 'p1' })).resolves.toEqual({ success: true })
    await expect(api.management.restart()).resolves.toEqual({ success: true })
    await expect(api.management.macTccAttribution()).resolves.toEqual({ health: 'unknown' })
  })
})
