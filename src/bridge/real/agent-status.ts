import type { AgentStatusIpcPayload } from '../../shared/agent-status-ipc-payload'
import { normalizeHookPayload } from '../../shared/agent-hook-listener'
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

export function createAgentStatusRealApi(): PreloadApi['agentStatus'] {
  // Why: 归一化在 renderer（规格 §3.2）；每个域实例持有 fork 监听器的
  // per-pane 缓存（prompt/tool/lead 状态）与 stateStartedAt epoch，跨事件演化。
  const listenerState = createHookListenerState()
  const stateStartedAtByPaneKey = new Map<string, { state: string; startedAt: number }>()

  const buildIpcPayload = (raw: AgentHookRawEvent): AgentStatusIpcPayload | null => {
    const normalized = normalizeHookPayload(
      listenerState,
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
    const prior = stateStartedAtByPaneKey.get(normalized.paneKey)
    const stateStartedAt = prior && prior.state === state ? prior.startedAt : raw.receivedAt
    stateStartedAtByPaneKey.set(normalized.paneKey, { state, startedAt: stateStartedAt })
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
        const payload = buildIpcPayload(raw)
        if (payload) {
          callback(payload)
        }
      }),
    // 宿主不产 clear 事件；通道保留为立即退订的 noop（规格 §3.7）。
    onClear: () => () => {},
    getSnapshot: async () => {
      const entries = await invokeCommand<AgentHookSnapshotEntry[]>('agent_status_get_snapshot')
      const payloads: AgentStatusIpcPayload[] = []
      for (const entry of entries) {
        const payload = buildIpcPayload(entry)
        if (payload) {
          payloads.push(payload)
        }
      }
      return payloads
    }
  })
}
