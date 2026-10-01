import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { EMPTY_PTY_MAIN_DELIVERY_DIAGNOSTICS } from '../../shared/pty-delivery-diagnostics'

const { FakeChannel } = vi.hoisted(() => {
  /** Programmable Channel stub: tests deliver ArrayBuffer / number[] frames by hand. */
  class FakeChannel {
    static instances: FakeChannel[] = []
    onmessage: ((chunk: ArrayBuffer | number[]) => void) | null = null
    constructor() {
      FakeChannel.instances.push(this)
    }
    deliver(chunk: ArrayBuffer | number[]): void {
      this.onmessage?.(chunk)
    }
  }
  return { FakeChannel }
})

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: FakeChannel }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
  listenMock.mockResolvedValue(() => {})
  vi.resetModules()
  FakeChannel.instances = []
})

function mockPtyCommands(handlers: Record<string, (args?: unknown) => unknown>): void {
  invokeMock.mockImplementation(async (command: string, request?: unknown) => {
    const handler = handlers[command]
    if (!handler) throw new Error(`unexpected command: ${command}`)
    return handler((request as { args?: unknown } | undefined)?.args)
  })
}

function mockSpawn(reply: unknown, handlers?: Record<string, (args?: unknown) => unknown>): void {
  mockPtyCommands({ pty_spawn: () => reply, ...handlers })
}

async function importFresh() {
  return await import('./pty')
}

const spawnOpts = { cols: 80, rows: 24 }

/** API with one spawned session whose channel has landed; `channel` is that channel. */
async function spawnedApi(reply: unknown = { id: 'pty-1' }) {
  mockSpawn(reply, { pty_attach: () => null })
  const { createPtyRealApi } = await importFresh()
  const api = createPtyRealApi()
  await api.spawn(spawnOpts)
  await vi.waitFor(() => expect(FakeChannel.instances).toHaveLength(1))
  return { api, channel: FakeChannel.instances[0] as InstanceType<typeof FakeChannel> }
}

function tauriHandler(event: string): (payload: unknown) => void {
  const call = listenMock.mock.calls.find(([name]) => name === event)
  if (!call) throw new Error(`no listener registered for ${event}`)
  return call[1] as (payload: unknown) => void
}

describe('spawn', () => {
  it('spawn_invokes_pty_spawn_with_args_wrapper', async () => {
    mockSpawn({ id: 'pty-1' }, { pty_attach: () => null })
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
  it('on_data_fans_out_from_channel_number_array_frames', async () => {
    const { api, channel } = await spawnedApi()
    const received: Array<Record<string, unknown>> = []
    const stopFirst = api.onData((data) => received.push(data))
    api.onData((data) => received.push({ ...data, second: true }))
    expect(api.getPtyDataListenerCount()).toBe(2)
    channel.deliver(Array.from(new TextEncoder().encode('héllo'))) // <1KiB JSON 数字数组帧
    expect(received).toEqual([
      { id: 'pty-1', data: 'héllo', rawLength: 6 },
      { id: 'pty-1', data: 'héllo', rawLength: 6, second: true }
    ])
    stopFirst()
    stopFirst() // 重复退订安全
    expect(api.getPtyDataListenerCount()).toBe(1)
  })

  it('on_data_decodes_raw_arraybuffer_frames_verbatim', async () => {
    const { api, channel } = await spawnedApi()
    const received: Array<Record<string, unknown>> = []
    api.onData((data) => received.push(data))
    const text = 'a'.repeat(2048) // ≥1KiB Raw 二进制帧形态
    channel.deliver(new TextEncoder().encode(text).slice().buffer as ArrayBuffer)
    expect(received).toEqual([{ id: 'pty-1', data: text, rawLength: 2048 }])
  })
})

describe('exit semantics', () => {
  it('attach_reject_emits_local_exit', async () => {
    // 唯一异常路径：pty_attach 被拒（spawn 后会话即刻消失的竞态等）→ 本地 code:-1。
    mockSpawn({ id: 'pty-1' }, { pty_attach: () => { throw new Error('unknown session') } })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    const exits: unknown[] = []
    api.onExit((data) => exits.push(data))
    await api.spawn(spawnOpts)
    await vi.waitFor(() => expect(exits).toEqual([{ id: 'pty-1', code: -1 }]))
  })

  it('channel_silence_never_emits_a_local_exit (pty:exit is the sole authority)', async () => {
    // 宿主转发结束 = 通道静默：不广播任何本地死亡，退出码只由 pty:exit 事件携带。
    const { api, channel } = await spawnedApi()
    const exits: unknown[] = []
    api.onExit((data) => exits.push(data))
    channel.deliver(Array.from(new TextEncoder().encode('out')))
    expect(api.getPtyDataListenerCount()).toBe(0) // 未订阅 onData
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(exits).toEqual([])
  })

  it('tauri_exit_event_closes_channel_and_forwards', async () => {
    const { api, channel } = await spawnedApi()
    const exits: unknown[] = []
    const received: Array<Record<string, unknown>> = []
    const unlisten = vi.fn()
    listenMock.mockResolvedValue(unlisten)
    const stop = api.onExit((data) => exits.push(data))
    api.onData((data) => received.push(data))
    tauriHandler('pty:exit')({ payload: { id: 'pty-1', code: 0 } })
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
    channel.deliver(Array.from(new TextEncoder().encode('late'))) // 摘除后晚到的帧保持惰性
    expect(received).toEqual([])
    // 发出即回 true（规格 §3.2 修订二）：exit 后的 writeAccepted 不再回 false。
    await expect(api.writeAccepted('pty-1', 'x')).resolves.toBe(true)
    stop()
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1))
    stop() // 重复退订安全
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('exit landing during the attach await drops the late channel (no zombie onData)', async () => {
    // Task 13 墓碑用例的 attach 替换语义等价断言：pty_attach 在飞期间 pty:exit 到达
    // （规格 §3.2 窗口），墓碑让晚到的通道被丢弃，而不是让死会话的 backlog 字节以
    // onData 形态污染渲染层；通道静默不判死，故不再有 code:-1 的重复死亡。
    let releaseAttach: () => void = () => {}
    let attachCalls = 0
    mockSpawn({ id: 'pty-1' }, {
      pty_attach: () => {
        attachCalls += 1
        if (attachCalls === 1) {
          return new Promise((resolve) => {
            releaseAttach = () => resolve(null)
          })
        }
        return null
      }
    })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    const exits: unknown[] = []
    const received: Array<Record<string, unknown>> = []
    api.onExit((data) => exits.push(data))
    api.onData((data) => received.push(data))
    await api.spawn(spawnOpts)
    // Channel 对象在 attach invoke 挂起期间即已存在（构造先于 await），但尚未落地收数。
    expect(FakeChannel.instances).toHaveLength(1)
    tauriHandler('pty:exit')({ payload: { id: 'pty-1', code: 0 } })
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
    releaseAttach()
    // 宏任务 flush：让 attach 的 resolve 续体（墓碑检查 → 通道哑化）走完。
    // 通道对象在 attach 在飞期间即已存在，waitFor 计数不会变化，故用确定性 flush。
    await new Promise((resolve) => setTimeout(resolve, 0))
    FakeChannel.instances[0]?.deliver(Array.from(new TextEncoder().encode('zombie')))
    expect(received).toEqual([]) // 晚到的通道被墓碑丢弃
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }]) // 无 code:-1 的重复死亡
    // 同 id 重生：墓碑已被消费，新 attach 正常落地收数。
    await api.spawn(spawnOpts)
    await vi.waitFor(() => expect(FakeChannel.instances).toHaveLength(2))
    FakeChannel.instances[1]?.deliver(Array.from(new TextEncoder().encode('fresh')))
    expect(received).toEqual([{ id: 'pty-1', data: 'fresh', rawLength: 5 }])
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
  })

  it('duplicate exit deliveries after the channel landed leave no tombstone (respawn stays live)', async () => {
    // 审查 fix round 1 等价断言：真实渲染层 ≥2 个 onExit 订阅者，Tauri 逐订阅者
    // 投递——第 1 个摘表，第 2+ 个全命中 close-miss。此时无 attach 在飞，不得立碑；
    // 否则 Phase 2 的 id 复用/reattach 会被残留墓碑静默吞掉活通道。
    const { api, channel } = await spawnedApi()
    api.onExit(() => {})
    api.onExit(() => {})
    const exits: unknown[] = []
    const received: Array<Record<string, unknown>> = []
    api.onExit((data) => exits.push(data))
    api.onData((data) => received.push(data))
    const exitHandlers = listenMock.mock.calls
      .filter(([name]) => name === 'pty:exit')
      .map(([, handler]) => handler as (message: { payload: unknown }) => void)
    expect(exitHandlers.length).toBeGreaterThanOrEqual(2)
    for (const handler of exitHandlers) handler({ payload: { id: 'pty-1', code: 0 } })
    expect(exits).toEqual([{ id: 'pty-1', code: 0 }])
    channel.deliver(Array.from(new TextEncoder().encode('late')))
    expect(received).toEqual([]) // 第 1 个订阅者已摘除通道
    // 同 id 重生：无墓碑残留，新通道正常落地（旧实现此处 open 被静默吞掉）。
    await api.spawn(spawnOpts)
    await vi.waitFor(() => expect(FakeChannel.instances).toHaveLength(2))
    FakeChannel.instances[1]?.deliver(Array.from(new TextEncoder().encode('live')))
    expect(received).toEqual([{ id: 'pty-1', data: 'live', rawLength: 4 }])
  })
})

describe('write', () => {
  it('write_and_write_accepted_invoke_pty_write_commands', async () => {
    const { api } = await spawnedApi()
    api.write('pty-1', 'abc')
    expect(invokeMock).toHaveBeenCalledWith('pty_write', {
      args: { id: 'pty-1', data: 'abc' }
    })
    await expect(api.writeAccepted('pty-1', 'déjà')).resolves.toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('pty_write_accepted', {
      args: { id: 'pty-1', data: 'déjà' }
    })
  })

  it('uplink survives unknown sessions and rejected commands (fire-and-forget)', async () => {
    let writeAcceptedCalls = 0
    mockPtyCommands({
      pty_spawn: () => ({ id: 'pty-1' }),
      pty_attach: () => null,
      pty_write: () => {
        throw new Error('session gone') // 宿主拒绝也必须静默（void 契约无 error lane）
      },
      pty_write_accepted: () => {
        writeAcceptedCalls += 1
        throw new Error('session gone')
      }
    })
    const { createPtyRealApi } = await importFresh()
    const api = createPtyRealApi()
    await api.spawn(spawnOpts)
    await vi.waitFor(() => expect(FakeChannel.instances).toHaveLength(1))
    expect(() => api.write('missing', 'x')).not.toThrow() // 未知会话仍上行，宿主静默丢
    await expect(api.writeAccepted('missing', 'x')).resolves.toBe(true) // 发出即 true
    expect(() => api.write('pty-1', 'x')).not.toThrow()
    await expect(api.writeAccepted('pty-1', 'x')).resolves.toBe(true)
    await vi.waitFor(() => expect(writeAcceptedCalls).toBe(2))
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
