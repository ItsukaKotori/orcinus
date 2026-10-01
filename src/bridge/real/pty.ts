import type { PreloadApi } from '../../shared/preload-api/api-types'
import { EMPTY_PTY_MAIN_DELIVERY_DIAGNOSTICS } from '../../shared/pty-delivery-diagnostics'
import { noopUnsubscribe } from '../mock/noop-unsubscribe'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'
import { closePtySocket, openPtySocket, sendPtySocketData } from './pty-socket'

/** 契约 `onData` 载荷（`Parameters` 两层剥壳：订阅方法 → 回调 → 载荷）。 */
type PtyDataPayload = Parameters<Parameters<PreloadApi['pty']['onData']>[0]>[0]
/** 契约 `onExit` 载荷（`pty:exit` 事件同形）。 */
type PtyExitPayload = Parameters<Parameters<PreloadApi['pty']['onExit']>[0]>[0]
/** 契约 `onSpawned` 载荷（`pty:spawned` 事件同形）。 */
type PtySpawnedPayload = Parameters<Parameters<PreloadApi['pty']['onSpawned']>[0]>[0]
/** 契约 `spawn` 回复（最小合法响应 `{id}`，reattach 命中带 `isReattach:true`）。 */
type PtySpawnReply = Awaited<ReturnType<PreloadApi['pty']['spawn']>>

/**
 * Minimal fan-out emitter: subscribers fire in subscription order and an
 * unsubscribe is idempotent, so a cleanup list that runs twice stays benign.
 */
function createListenerSet<Payload>(): {
  add: (callback: (payload: Payload) => void) => () => void
  emit: (payload: Payload) => void
  size: () => number
} {
  const listeners = new Set<(payload: Payload) => void>()
  return {
    add(callback) {
      listeners.add(callback)
      return () => {
        listeners.delete(callback)
      }
    },
    emit(payload) {
      for (const listener of [...listeners]) listener(payload)
    },
    size: () => listeners.size
  }
}

/**
 * Real `pty` adapter (spec §2.1–§2.3 disposition table). The control plane
 * rides the `pty_*` commands with the `{args}` envelope; the data plane is a
 * per-session loopback WebSocket (`openPtySocket`) fanned out through the
 * local `onData` emitter — pty bytes never transit a Tauri event. A Tauri
 * `pty:exit` forwards to `onExit` and closes that session's socket; an
 * abnormal socket close (not caused by `pty:exit`) declares the session dead
 * with a local `code: -1` exit broadcast (spec §3.2 known semantics). The
 * renderer machinery that has no Phase 1C host (delivery gate, pane
 * serializers, snapshots, liveness) keeps the web stub's verbatim shapes.
 */
export function createPtyRealApi(): PreloadApi['pty'] {
  const dataListeners = createListenerSet<PtyDataPayload>()
  const exitListeners = createListenerSet<PtyExitPayload>()
  // Why: the local death broadcast must reach every onExit subscriber, while
  // `pty:exit` forwards per subscription — the two sources never overlap
  // because closePtySocket removes the socket before its onclose can fire.
  const declareSessionDeath = (id: string): void => {
    exitListeners.emit({ id, code: -1 })
  }

  return withMethodFallback<PreloadApi['pty']>('pty', {
    // ===== §2.1 real —— 控制面（`{args}` 包裹，契约-only 字段整包透传）=====
    spawn: async (opts) => {
      const reply = await invokeCommand<PtySpawnReply>('pty_spawn', { args: opts })
      // Why fire-and-forget: the endpoint fetch is one cached IPC hop; bytes
      // emitted before the socket lands drain from the host's pre-attach
      // buffer on first connect (spec §3.2).
      void openPtySocket(reply.id, {
        onData: (payload) => dataListeners.emit(payload),
        onAbnormalClose: declareSessionDeath
      }).catch(() => {})
      return reply
    },
    resize: (id, cols, rows) => {
      // Why swallowed: the void contract has no error lane, and resizing a
      // session that just exited must not surface as an unhandled rejection.
      void invokeCommand('pty_resize', { args: { id, cols, rows } }).catch(() => {})
    },
    signal: (id, signal) => {
      void invokeCommand('pty_signal', { args: { id, signal } }).catch(() => {})
    },
    clearBuffer: (id) => {
      void invokeCommand('pty_clear_buffer', { args: { id } }).catch(() => {})
    },
    kill: (id, opts) =>
      invokeCommand<void>('pty_kill', { args: { id, keepHistory: opts?.keepHistory } }),
    getCwd: (id) => invokeCommand<string>('pty_get_cwd', { args: { id } }),
    getSize: (id) =>
      invokeCommand<{ cols: number; rows: number } | null>('pty_get_size', { args: { id } }),
    hasPty: (id) => invokeCommand<boolean | null>('pty_has_pty', { args: { id } }),
    listSessions: (scope) => invokeCommand('pty_list_sessions', { args: { scope } }),

    // ===== §2.1 real —— 数据面（WS 直写；不经 Tauri event）=====
    write: (id, data) => {
      sendPtySocketData(id, data) // WS 未就绪即静默丢
    },
    writeAccepted: async (id, data) => sendPtySocketData(id, data),
    onData: (callback) => dataListeners.add(callback),
    /** 真实返回 pty:data emitter 的监听数（比 stub 的 0 诚实，规格 §2.2）。 */
    getPtyDataListenerCount: () => dataListeners.size(),

    // ===== §2.1 real —— 事件映射 =====
    onExit: (callback) => {
      const stopLocal = exitListeners.add(callback)
      // Server order (spec §3.2): the socket closes before the event arrives;
      // closing here also suppresses that socket's abnormal-close path.
      const stopEvent = subscribeToEvent<PtyExitPayload>('pty:exit', (payload) => {
        closePtySocket(payload.id)
        callback(payload)
      })
      return () => {
        stopLocal()
        stopEvent()
      }
    },
    onSpawned: (callback) => subscribeToEvent<PtySpawnedPayload>('pty:spawned', callback),

    // ===== §2.2 web-stub 同形缺省（值逐字照抄 web-terminal-api.ts）=====
    // Why local constants instead of the `pty_*` stub commands: Phase 1C has
    // no liveness/snapshot/delivery machinery on the host, so these are
    // renderer-side defaults; answering without an IPC hop keeps watchdogs and
    // capability probes idle at zero cost.
    getForegroundProcess: () => Promise.resolve(null),
    confirmForegroundProcess: () => Promise.resolve(null),
    hasChildProcesses: () => Promise.resolve(false),
    inspectProcess: () => Promise.reject(new Error('terminal_liveness_unavailable')),
    getMainBufferSnapshot: () => Promise.resolve(null),
    getAuthoritativeBufferSnapshotCapabilities: (ids) =>
      Promise.resolve(ids.map((id) => ({ id, authoritative: false }))),
    // Why: no delivery machinery on this host; a zero-in-flight reply keeps the watchdog idle.
    reportRendererDeliveryState: () =>
      Promise.resolve({ inFlightTotalChars: 0, inFlightPtyCount: 0, msSinceLastAck: null }),
    getRendererDeliveryDebugSnapshot: () =>
      Promise.resolve({
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
      }),

    // ===== §2.3 noop 订阅 / no-op 方法（web stub 现状逐字）=====
    ackData: () => {},
    ackColdRestore: () => {},
    claimViewport: () => {},
    reportGeometry: () => {},
    onDeliveryResyncRequest: () => noopUnsubscribe,
    respondDeliveryResync: () => {},
    rendererDispatcherReady: () => {},
    setActiveRendererPty: () => {},
    setRendererPtyVisible: () => {},
    setHiddenRendererPty: () => {},
    setPtyDeliveryInterest: () => {},
    publishTerminalViewAttributes: () => {},
    onWriteUnavailable: () => noopUnsubscribe,
    onReplay: () => noopUnsubscribe,
    onModelRestoreNeeded: () => noopUnsubscribe,
    onSideEffect: () => noopUnsubscribe,
    getSideEffectSnapshot: () => Promise.resolve(null),
    onSerializeBufferRequest: () => noopUnsubscribe,
    onClearBufferRequest: () => noopUnsubscribe,
    sendSerializedBuffer: () => {},
    declarePendingPaneSerializer: () => Promise.resolve(0),
    settlePaneSerializer: () => Promise.resolve(),
    clearPendingPaneSerializer: () => Promise.resolve(),
    reportRendererSerializerReady: () => Promise.resolve(),
    resetRendererDeliveryDebug: () => Promise.resolve(),

    // ===== management（§2.1：前三者注册表真实操作；restart/tcc 走宿主常量）=====
    management: {
      listSessions: () => invokeCommand('pty_management_list_sessions'),
      killAll: () => invokeCommand('pty_management_kill_all'),
      killOne: (args) => invokeCommand('pty_management_kill_one', { args }),
      restart: () => invokeCommand('pty_management_restart'),
      macTccAttribution: () => invokeCommand('pty_management_mac_tcc_attribution')
    }
  })
}
