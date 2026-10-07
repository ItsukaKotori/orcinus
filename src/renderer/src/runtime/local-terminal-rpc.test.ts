import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppState } from '@/store/types'
import { callLocalTerminalRpc, isLocalTerminalRpcMethod } from './local-terminal-rpc'

const LEAF_ID = '11111111-1111-4111-8111-111111111111'

const testState = vi.hoisted(() => ({
  appState: null as unknown as Partial<AppState>
}))

vi.mock('@/store', () => ({
  useAppStore: Object.assign(
    (selector: (state: Partial<AppState>) => unknown) => selector(testState.appState),
    { getState: () => testState.appState }
  )
}))

const listSessions = vi.fn()
const writeAccepted = vi.fn()

function makeState(overrides: Partial<AppState> = {}): Partial<AppState> {
  return {
    tabsByWorktree: { 'wt-1': [{ id: 'tab-1' }] },
    ptyIdsByTabId: { 'tab-1': ['pty-1'] },
    terminalLayoutsByTabId: {
      'tab-1': {
        root: null,
        activeLeafId: LEAF_ID,
        expandedLeafId: null,
        ptyIdsByLeafId: { [LEAF_ID]: 'pty-1' }
      }
    } as AppState['terminalLayoutsByTabId'],
    runtimePaneTitlesByTabId: {},
    worktreesByRepo: {},
    ...overrides
  } as Partial<AppState>
}

beforeEach(() => {
  testState.appState = makeState()
  listSessions.mockReset()
  writeAccepted.mockReset()
  listSessions.mockResolvedValue([
    { id: 'pty-1', cwd: '/tmp/wt', title: '', worktreeId: 'wt-1', agentOwnership: 'present' }
  ])
  writeAccepted.mockResolvedValue(true)
  vi.stubGlobal('window', { api: { pty: { listSessions, writeAccepted } } })
})

describe('local terminal RPC adapter', () => {
  it('recognizes exactly the five local terminal methods', () => {
    for (const method of [
      'terminal.list',
      'terminal.agentStatus',
      'terminal.isRunningAgent',
      'terminal.wait',
      'terminal.send'
    ]) {
      expect(isLocalTerminalRpcMethod(method)).toBe(true)
    }
    expect(isLocalTerminalRpcMethod('terminal.create')).toBe(false)
    expect(isLocalTerminalRpcMethod('repo.list')).toBe(false)
  })

  it('lists live local terminals mapped to tab/leaf/worktree with ptyId as handle', async () => {
    const result = await callLocalTerminalRpc('terminal.list', {
      worktree: 'id:wt-1',
      limit: 200,
      includeVisualLayouts: false
    })
    expect(result).toEqual({
      terminals: [
        {
          handle: 'pty-1',
          ptyId: 'pty-1',
          worktreeId: 'wt-1',
          worktreePath: '',
          branch: '',
          tabId: 'tab-1',
          leafId: LEAF_ID,
          title: null,
          connected: true,
          writable: true,
          lastOutputAt: null,
          preview: ''
        }
      ],
      totalCount: 1,
      truncated: false
    })
    expect(listSessions).toHaveBeenCalledWith({ connectionId: null })
  })

  it('excludes terminals whose pty is no longer live', async () => {
    listSessions.mockResolvedValue([])
    const result = await callLocalTerminalRpc<{ terminals: unknown[] }>('terminal.list', {})
    expect(result.terminals).toEqual([])
  })

  it('filters by the runtime worktree selector', async () => {
    const result = await callLocalTerminalRpc<{ terminals: unknown[] }>('terminal.list', {
      worktree: 'id:wt-other'
    })
    expect(result.terminals).toEqual([])
  })

  it('rejects unknown methods with method_not_found', async () => {
    await expect(callLocalTerminalRpc('terminal.unknown', {})).rejects.toMatchObject({
      name: 'RuntimeRpcCallError',
      code: 'method_not_found'
    })
  })
})
