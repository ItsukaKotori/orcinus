import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  sendNotesToActiveAgentSession
} from '@/lib/active-agent-note-send'
import {
  createNoteSendAppState,
  LEAF_ID,
  PASTE_BEGIN,
  PASTE_END,
  type NoteSendAppState
} from '@/lib/active-agent-note-send-test-harness'
import { makeAgentStatusEntry } from './sync-runtime-graph-test-harness'

const testState = vi.hoisted(() => ({
  appState: null as unknown as NoteSendAppState
}))

vi.mock('@/store', () => ({
  useAppStore: Object.assign(
    (selector: (state: NoteSendAppState) => unknown) => selector(testState.appState),
    { getState: () => testState.appState }
  )
}))

const listSessions = vi.fn()
const writeAccepted = vi.fn()
const runtimeCall = vi.fn()

beforeEach(() => {
  testState.appState = createNoteSendAppState()
  listSessions.mockReset()
  writeAccepted.mockReset()
  runtimeCall.mockReset()
  listSessions.mockResolvedValue([
    { id: 'pty-1', cwd: '/tmp/wt', title: '', worktreeId: 'wt-1', agentOwnership: 'present' }
  ])
  writeAccepted.mockResolvedValue(true)
  runtimeCall.mockRejectedValue(new Error('local runtime.call must not be used for terminal methods'))
  vi.stubGlobal('window', {
    api: {
      pty: {
        listSessions,
        writeAccepted,
        onData: vi.fn(() => () => {}),
        onExit: vi.fn(() => () => {}),
        onReplay: vi.fn(() => () => {}),
        onSpawned: vi.fn(() => () => {})
      },
      runtime: { call: runtimeCall }
    }
  })
})

describe('local notes send end-to-end through the local terminal adapter', () => {
  it('pastes unsent notes into an idle running agent and submits Enter', async () => {
    testState.appState.agentStatusByPaneKey[`tab-1:${LEAF_ID}`] = makeAgentStatusEntry({
      state: 'waiting',
      updatedAt: Date.now()
    })

    const result = await sendNotesToActiveAgentSession({
      worktreeId: 'wt-1',
      prompt: 'fix the bug'
    })

    expect(result).toEqual({ status: 'sent' })
    const writeTargets = writeAccepted.mock.calls.map((call) => call[0] as string)
    const writes = writeAccepted.mock.calls.map((call) => call[1] as string)
    expect(writeTargets).toEqual(['pty-1', 'pty-1'])
    expect(writes[0]).toBe(`${PASTE_BEGIN}fix the bug${PASTE_END}`)
    expect(writes[1]).toBe('\r')
    expect(runtimeCall).not.toHaveBeenCalled()
  })

  it('reports permission instead of pasting while the agent is blocked', async () => {
    testState.appState.agentStatusByPaneKey[`tab-1:${LEAF_ID}`] = makeAgentStatusEntry({
      state: 'blocked',
      updatedAt: Date.now()
    })

    const result = await sendNotesToActiveAgentSession({
      worktreeId: 'wt-1',
      prompt: 'fix the bug'
    })

    expect(result).toMatchObject({ status: 'permission' })
    expect(writeAccepted).not.toHaveBeenCalled()
  })
})
