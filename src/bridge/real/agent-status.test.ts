import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
let eventHandler: ((message: { payload: unknown }) => void) | null = null

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args)
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_event: string, handler: (message: { payload: unknown }) => void) => {
    eventHandler = handler
    return () => {
      eventHandler = null
    }
  })
}))

import { createAgentStatusRealApi } from './agent-status'

const PANE = 't1:123e4567-e89b-42d3-a456-426614174000'

function emitRaw(payload: Record<string, unknown>, receivedAt = 1000) {
  eventHandler?.({
    payload: {
      source: 'claude',
      payload,
      paneKey: PANE,
      tabId: 't1',
      worktreeId: 'r1::/wt',
      launchToken: 'tok-1',
      receivedAt,
      restored: false
    }
  })
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

describe('agentStatus real bridge', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    eventHandler = null
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('normalizes claude working/waiting/done and tracks stateStartedAt epochs', async () => {
    const api = createAgentStatusRealApi()
    const seen: { state: string; stateStartedAt: number }[] = []
    const unsubscribe = api.onSet((payload) => {
      seen.push({ state: payload.state, stateStartedAt: payload.stateStartedAt })
    })
    await flush()
    emitRaw({ hook_event_name: 'UserPromptSubmit', prompt: 'Fix login bug', session_id: 's1' }, 1000)
    emitRaw({ hook_event_name: 'PostToolUse', tool_name: 'Bash', session_id: 's1' }, 1100)
    emitRaw({ hook_event_name: 'PermissionRequest', tool_name: 'Bash', session_id: 's1' }, 1200)
    emitRaw({ hook_event_name: 'Stop', last_assistant_message: 'Done.', session_id: 's1' }, 1300)
    expect(seen.map((entry) => entry.state)).toEqual(['working', 'working', 'waiting', 'done'])
    // 同 state 的后续事件不重置 epoch；state 变化才换 epoch。
    expect(seen.map((entry) => entry.stateStartedAt)).toEqual([1000, 1000, 1200, 1300])
    unsubscribe()
  })

  it('assembles the IPC envelope and drops unnormalizable payloads', async () => {
    const api = createAgentStatusRealApi()
    const seen: unknown[] = []
    api.onSet((payload) => seen.push(payload))
    await flush()
    emitRaw({ hook_event_name: 'Stop', last_assistant_message: 'Done.', session_id: 's1' })
    emitRaw({ not_a_hook: true })
    expect(seen).toHaveLength(1)
    expect(seen[0]).toMatchObject({
      paneKey: PANE,
      tabId: 't1',
      worktreeId: 'r1::/wt',
      launchToken: 'tok-1',
      connectionId: null,
      receivedAt: 1000,
      stateStartedAt: 1000,
      state: 'done'
    })
  })

  it('onClear is an immediate noop subscription', () => {
    const api = createAgentStatusRealApi()
    const unsubscribe = api.onClear(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })

  it('getSnapshot replays cached entries and marks restored non-done rows', async () => {
    const api = createAgentStatusRealApi()
    invokeMock.mockResolvedValueOnce([
      {
        source: 'claude',
        payload: { hook_event_name: 'PermissionRequest', tool_name: 'Bash' },
        paneKey: PANE,
        tabId: 't1',
        worktreeId: 'r1::/wt',
        receivedAt: 500,
        restored: true
      },
      {
        source: 'claude',
        payload: { hook_event_name: 'Stop', last_assistant_message: 'Done.' },
        paneKey: 't2:123e4567-e89b-42d3-a456-426614174001',
        receivedAt: 600,
        restored: true
      }
    ])
    const snapshot = await api.getSnapshot()
    expect(invokeMock).toHaveBeenCalledWith('agent_status_get_snapshot')
    expect(snapshot[0]).toMatchObject({ state: 'waiting', restoredUnconfirmed: true })
    expect(snapshot[1]).toMatchObject({ state: 'done' })
    expect('restoredUnconfirmed' in snapshot[1]).toBe(false)
  })
})
