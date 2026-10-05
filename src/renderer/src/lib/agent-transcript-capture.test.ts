import { describe, expect, it, vi } from 'vitest'
import type { AgentProviderSessionMetadata } from '../../../shared/agent-session-resume'
import type { AppState } from '../store'
import {
  captureTranscriptAgentSessions,
  type TranscriptCaptureDeps
} from './agent-transcript-capture'

const LEAF_A = '11111111-1111-4111-8111-111111111111'

// Why single-pane: every live pane is now a scan target (no title gate), so a
// second pane in the default fixture would double every record/call assertion.
function makeState(overrides?: Partial<Record<string, unknown>>): AppState {
  return {
    tabsByWorktree: {
      'w1::/repo': [{ id: 'tab1', title: 'claude · working' }]
    },
    terminalLayoutsByTabId: {
      tab1: {
        root: { type: 'leaf', leafId: LEAF_A },
        activeLeafId: LEAF_A,
        expandedLeafId: null,
        ptyIdsByLeafId: { [LEAF_A]: 'pty-1' },
        titlesByLeafId: { [LEAF_A]: 'claude · working' }
      }
    },
    sleepingAgentSessionsByPaneKey: {},
    ...overrides
  } as unknown as AppState
}

function makeDeps(overrides?: Partial<TranscriptCaptureDeps>): TranscriptCaptureDeps {
  const merged: TranscriptCaptureDeps = {
    state: makeState(),
    origin: 'quit',
    resolveCwd: async () => '/repo/work',
    resolveProviderSession: async () => ({ key: 'session_id', id: 'session-uuid' }),
    now: () => 5_000,
    mergeRecords: vi.fn(),
    ...overrides
  }
  return merged
}

describe('captureTranscriptAgentSessions', () => {
  it('captures records for agent-titled panes via the host scan', async () => {
    const mergeRecords = vi.fn()
    await captureTranscriptAgentSessions(makeDeps({ mergeRecords }))
    expect(mergeRecords).toHaveBeenCalledTimes(1)
    const records = mergeRecords.mock.calls[0][0]
    expect(records).toHaveLength(1)
    expect(records[0]).toMatchObject({
      paneKey: `tab1:${LEAF_A}`,
      tabId: 'tab1',
      worktreeId: 'w1::/repo',
      agent: 'claude',
      providerSession: { key: 'session_id', id: 'session-uuid' },
      prompt: '',
      state: 'waiting',
      origin: 'quit'
    })
  })

  it('passes cwd, agentKind and the boot-anchored window to the scan', async () => {
    const resolveProviderSession = vi.fn().mockResolvedValue(null)
    await captureTranscriptAgentSessions(makeDeps({ resolveProviderSession }))
    expect(resolveProviderSession).toHaveBeenCalledWith({
      cwd: '/repo/work',
      agentKind: 'claude',
      windowFromMs: expect.any(Number),
      windowToMs: 5_000
    })
  })

  it('creates a record for a shell-titled pane when the scan hits (title no longer gates)', async () => {
    const mergeRecords = vi.fn()
    const state = makeState({
      tabsByWorktree: {
        'w1::/repo': [{ id: 'tab1', title: 'Terminal 1' }]
      }
    })
    ;(state.terminalLayoutsByTabId.tab1 as { titlesByLeafId?: Record<string, string> }).titlesByLeafId = {
      [LEAF_A]: 'zsh'
    }
    await captureTranscriptAgentSessions(makeDeps({ state, mergeRecords }))
    expect(mergeRecords).toHaveBeenCalledTimes(1)
    const records = mergeRecords.mock.calls[0][0]
    expect(records[0]).toMatchObject({
      paneKey: `tab1:${LEAF_A}`,
      worktreeId: 'w1::/repo',
      agent: 'claude',
      origin: 'quit'
    })
  })

  it('falls back to the codex scan when the claude scan misses', async () => {
    const mergeRecords = vi.fn()
    const resolveProviderSession = vi.fn(
      async (args: { agentKind: string }): Promise<AgentProviderSessionMetadata | null> =>
        args.agentKind === 'codex' ? { key: 'session_id', id: 'codex-uuid' } : null
    )
    await captureTranscriptAgentSessions(makeDeps({ resolveProviderSession, mergeRecords }))
    expect(resolveProviderSession.mock.calls.map((call) => call[0].agentKind)).toEqual([
      'claude',
      'codex'
    ])
    const records = mergeRecords.mock.calls[0][0]
    expect(records[0]).toMatchObject({
      paneKey: `tab1:${LEAF_A}`,
      agent: 'codex',
      providerSession: { key: 'session_id', id: 'codex-uuid' }
    })
  })

  it('a miss on every scan yields no record and no merge', async () => {
    const mergeRecords = vi.fn()
    await captureTranscriptAgentSessions(
      makeDeps({ resolveProviderSession: async () => null, mergeRecords })
    )
    expect(mergeRecords).not.toHaveBeenCalled()
  })

  it('skips panes whose record already has a providerSession', async () => {
    const resolveProviderSession = vi.fn()
    const state = makeState({
      sleepingAgentSessionsByPaneKey: {
        [`tab1:${LEAF_A}`]: {
          paneKey: `tab1:${LEAF_A}`,
          worktreeId: 'w1::/repo',
          agent: 'claude',
          providerSession: { key: 'session_id', id: 'existing' },
          prompt: '',
          state: 'waiting',
          capturedAt: 1,
          updatedAt: 1
        }
      }
    })
    await captureTranscriptAgentSessions(makeDeps({ state, resolveProviderSession }))
    expect(resolveProviderSession).not.toHaveBeenCalled()
  })

  it('skips panes without a live ptyId or a failed cwd probe', async () => {
    const resolveProviderSession = vi.fn()
    const noPty = makeState()
    ;(noPty.terminalLayoutsByTabId.tab1 as { ptyIdsByLeafId?: Record<string, string> }).ptyIdsByLeafId = undefined
    await captureTranscriptAgentSessions(makeDeps({ state: noPty, resolveProviderSession }))
    expect(resolveProviderSession).not.toHaveBeenCalled()
    await captureTranscriptAgentSessions(
      makeDeps({ resolveCwd: async () => null, resolveProviderSession })
    )
    expect(resolveProviderSession).not.toHaveBeenCalled()
  })
})
