import type { AgentStatusIpcPayload } from '../../shared/agent-status-ipc-payload'
import { normalizeHookPayload } from '../../shared/agent-hook-listener'
import type { HookListenerState } from '../../shared/agent-hook-listener/listener-state'
import { createHookListenerState } from '../../shared/agent-hook-listener/listener-state'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/** `agent-hook:raw` 事件载荷（Rust `events::AgentHookRawPayload` 同形）。 */
type AgentHookRawEvent = {
  source: string
  payload: Record<string, unknown>
  paneKey: string
  tabId?: string
  worktreeId?: string
  launchToken?: string
  receivedAt: number
  restored?: boolean
  env?: string
}

/** `agent_status_get_snapshot` 元素（Rust `AgentHookSnapshotEntry` 同形）。 */
type AgentHookSnapshotEntry = AgentHookRawEvent

type PaneStateEpoch = { state: string; startedAt: number }

export function createAgentStatusRealApi(): PreloadApi['agentStatus'] {
  // Why: 归一化在 renderer（规格 §3.2）；每个域实例持有 fork 监听器的
  // per-pane 缓存（prompt/tool/lead 状态）与 stateStartedAt epoch，跨事件演化。
  const listenerState = createHookListenerState()
  const stateStartedAtByPaneKey = new Map<string, PaneStateEpoch>()

  const buildIpcPayloadWith = (
    raw: AgentHookRawEvent,
    normalizerState: HookListenerState,
    epochs: Map<string, PaneStateEpoch>
  ): AgentStatusIpcPayload | null => {
    const normalized = normalizeHookPayload(
      normalizerState,
      'claude',
      {
        paneKey: raw.paneKey,
        tabId: raw.tabId,
        worktreeId: raw.worktreeId,
        launchToken: raw.launchToken,
        payload: raw.payload
      },
      raw.env ?? ''
    )
    if (!normalized) {
      return null
    }
    const state = normalized.payload.state
    const prior = epochs.get(normalized.paneKey)
    const stateStartedAt = prior && prior.state === state ? prior.startedAt : raw.receivedAt
    epochs.set(normalized.paneKey, { state, startedAt: stateStartedAt })
    const restoredUnconfirmed = raw.restored === true && state !== 'done'
    return {
      ...normalized.payload,
      paneKey: normalized.paneKey,
      ...(normalized.launchToken ? { launchToken: normalized.launchToken } : {}),
      ...(normalized.tabId ? { tabId: normalized.tabId } : {}),
      ...(normalized.worktreeId ? { worktreeId: normalized.worktreeId } : {}),
      connectionId: null,
      receivedAt: raw.receivedAt,
      stateStartedAt,
      ...(restoredUnconfirmed ? { restoredUnconfirmed: true } : {}),
      ...(normalized.promptInteractionKey
        ? { promptInteractionKey: normalized.promptInteractionKey }
        : {}),
      ...(normalized.providerSession ? { providerSession: normalized.providerSession } : {}),
      ...(normalized.providerSessionOnly ? { providerSessionOnly: true } : {})
    }
  }

  return withMethodFallback<PreloadApi['agentStatus']>('agentStatus', {
    onSet: (callback) =>
      subscribeToEvent<AgentHookRawEvent>('agent-hook:raw', (raw) => {
        const payload = buildIpcPayloadWith(raw, listenerState, stateStartedAtByPaneKey)
        if (payload) {
          callback(payload)
        }
      }),
    // 宿主不产 clear 事件；通道保留为立即退订的 noop（规格 §3.7）。
    onClear: () => () => {},
    getSnapshot: async () => {
      const entries = await invokeCommand<AgentHookSnapshotEntry[]>('agent_status_get_snapshot')
      // Why: 回放绝不共用实时归一化状态/epoch——与快照 in-flight 竞争的实时事件会被过期
      // 缓存覆盖。每次调用新建回放状态也让重复 getSnapshot 幂等（一次性守卫如 compact
      // 消费标记不再跨调用吃掉条目）。
      const replayState = createHookListenerState()
      const replayEpochs = new Map<string, PaneStateEpoch>()
      const payloads: AgentStatusIpcPayload[] = []
      for (const entry of entries) {
        const payload = buildIpcPayloadWith(entry, replayState, replayEpochs)
        if (payload) {
          payloads.push(payload)
        }
      }
      // Why: 仅当该 pane 还没有实时条目时才并入回放 epoch，保证抢跑的实时事件不被过期回放覆盖。
      for (const [paneKey, epoch] of replayEpochs) {
        if (!stateStartedAtByPaneKey.has(paneKey)) {
          stateStartedAtByPaneKey.set(paneKey, epoch)
        }
      }
      return payloads
    }
  })
}
