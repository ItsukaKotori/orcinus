import { describe, expect, it } from 'vitest'
import { FLOATING_TERMINAL_WORKTREE_ID } from './constants'
import type { WorkspaceSessionState } from './workspace-session-state-types'
import { TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT } from './terminal-scrollback-limits'
import { getUtf8ByteLength } from './utf8-byte-limits'
import {
  pruneLocalTerminalScrollbackBuffers,
  shouldPreserveTerminalScrollbackBuffers
} from './workspace-session-terminal-buffers'

function makeSession(overrides: Partial<WorkspaceSessionState> = {}): WorkspaceSessionState {
  return {
    activeRepoId: null,
    activeWorktreeId: null,
    activeTabId: null,
    tabsByWorktree: {
      'local-repo::/local/worktree': [
        {
          id: 'local-tab',
          title: 'local',
          customTitle: null,
          color: null,
          sortOrder: 0,
          createdAt: 1,
          ptyId: 'local-pty',
          worktreeId: 'local-repo::/local/worktree'
        }
      ],
      'remote-repo::/remote/worktree': [
        {
          id: 'remote-tab',
          title: 'remote',
          customTitle: null,
          color: null,
          sortOrder: 0,
          createdAt: 1,
          ptyId: 'remote-pty',
          worktreeId: 'remote-repo::/remote/worktree'
        }
      ]
    },
    terminalLayoutsByTabId: {
      'local-tab': {
        root: null,
        activeLeafId: null,
        expandedLeafId: null,
        buffersByLeafId: { 'pane:1': 'local-scrollback' },
        scrollbackRefsByLeafId: { 'pane:1': 'v1-local' },
        ptyIdsByLeafId: { 'pane:1': 'local-pty' }
      },
      'remote-tab': {
        root: null,
        activeLeafId: null,
        expandedLeafId: null,
        buffersByLeafId: { 'pane:1': 'remote-scrollback' },
        scrollbackRefsByLeafId: { 'pane:1': 'v1-remote' },
        ptyIdsByLeafId: { 'pane:1': 'remote-pty' }
      }
    },
    ...overrides
  }
}

function makeRuntimeSession(): WorkspaceSessionState {
  return makeSession({
    tabsByWorktree: {
      'runtime-repo::/runtime/worktree': [
        {
          id: 'runtime-tab',
          title: 'runtime',
          customTitle: null,
          color: null,
          sortOrder: 0,
          createdAt: 1,
          ptyId: 'runtime-pty',
          worktreeId: 'runtime-repo::/runtime/worktree'
        }
      ]
    },
    terminalLayoutsByTabId: {
      'runtime-tab': {
        root: null,
        activeLeafId: null,
        expandedLeafId: null,
        buffersByLeafId: { 'pane:1': 'runtime-scrollback' },
        scrollbackRefsByLeafId: { 'pane:1': 'v1-runtime' },
        ptyIdsByLeafId: { 'pane:1': 'runtime-pty' }
      }
    }
  })
}

describe('pruneLocalTerminalScrollbackBuffers', () => {
  it('tolerates legacy sessions without terminal maps', () => {
    const legacySession = {
      activeRepoId: null,
      activeWorktreeId: null,
      activeTabId: null
    } as WorkspaceSessionState

    expect(() => pruneLocalTerminalScrollbackBuffers(legacySession, [])).not.toThrow()
    expect(pruneLocalTerminalScrollbackBuffers(legacySession, [])).toEqual(legacySession)
  })

  it('preserves scrollback for local repos too (ade has no daemon — spec R2)', () => {
    // Why: renderer-captured scrollback is the only durable copy for every repo
    // kind now that no out-of-process daemon holds a local history, so local
    // worktrees preserve exactly like remote/runtime ones.
    const repos = [
      { id: 'local-repo', connectionId: null },
      { id: 'remote-repo', connectionId: 'ssh-target-1' }
    ]

    expect(shouldPreserveTerminalScrollbackBuffers('local-repo::/local/worktree', repos)).toBe(
      true
    )
    expect(shouldPreserveTerminalScrollbackBuffers('remote-repo::/remote/worktree', repos)).toBe(
      true
    )
    expect(shouldPreserveTerminalScrollbackBuffers(FLOATING_TERMINAL_WORKTREE_ID, repos)).toBe(
      false
    )
    expect(
      shouldPreserveTerminalScrollbackBuffers('unknown-repo::/maybe-remote/worktree', repos)
    ).toBe(true)
  })

  it('preserves runtime-host scrollback without requiring an SSH connection ID', () => {
    expect(
      shouldPreserveTerminalScrollbackBuffers('runtime-repo::/runtime/worktree', [
        {
          id: 'runtime-repo',
          connectionId: null,
          executionHostId: 'runtime:env-1'
        }
      ])
    ).toBe(true)

    const result = pruneLocalTerminalScrollbackBuffers(makeRuntimeSession(), [
      {
        id: 'runtime-repo',
        connectionId: null,
        executionHostId: 'runtime:env-1'
      }
    ])

    expect(result.terminalLayoutsByTabId['runtime-tab'].buffersByLeafId).toEqual({
      'pane:1': 'runtime-scrollback'
    })
    expect(result.terminalLayoutsByTabId['runtime-tab'].scrollbackRefsByLeafId).toEqual({
      'pane:1': 'v1-runtime'
    })
  })

  it('preserves scrollback for explicitly local execution hosts (spec R2)', () => {
    expect(
      shouldPreserveTerminalScrollbackBuffers('runtime-repo::/runtime/worktree', [
        {
          id: 'runtime-repo',
          connectionId: null,
          executionHostId: 'local'
        }
      ])
    ).toBe(true)

    const result = pruneLocalTerminalScrollbackBuffers(makeRuntimeSession(), [
      {
        id: 'runtime-repo',
        connectionId: null,
        executionHostId: 'local'
      }
    ])

    expect(result.terminalLayoutsByTabId['runtime-tab'].buffersByLeafId).toEqual({
      'pane:1': 'runtime-scrollback'
    })
    expect(result.terminalLayoutsByTabId['runtime-tab'].scrollbackRefsByLeafId).toEqual({
      'pane:1': 'v1-runtime'
    })
  })

  it('preserves local scrollback alongside SSH scrollback and PTY bindings (spec R2)', () => {
    const result = pruneLocalTerminalScrollbackBuffers(makeSession(), [
      { id: 'local-repo', connectionId: null },
      { id: 'remote-repo', connectionId: 'ssh-target-1' }
    ])

    expect(result.terminalLayoutsByTabId['local-tab'].buffersByLeafId).toEqual({
      'pane:1': 'local-scrollback'
    })
    expect(result.terminalLayoutsByTabId['local-tab'].scrollbackRefsByLeafId).toEqual({
      'pane:1': 'v1-local'
    })
    expect(result.terminalLayoutsByTabId['remote-tab'].buffersByLeafId).toEqual({
      'pane:1': 'remote-scrollback'
    })
    expect(result.terminalLayoutsByTabId['remote-tab'].scrollbackRefsByLeafId).toEqual({
      'pane:1': 'v1-remote'
    })
  })

  it('caps preserved SSH buffers so session JSON cannot scale with raw scrollback', () => {
    const hugeScrollback = `start-${'x'.repeat(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT + 10)}`
    const result = pruneLocalTerminalScrollbackBuffers(
      makeSession({
        terminalLayoutsByTabId: {
          'remote-tab': {
            root: null,
            activeLeafId: null,
            expandedLeafId: null,
            buffersByLeafId: { 'pane:1': hugeScrollback }
          }
        }
      }),
      [{ id: 'remote-repo', connectionId: 'ssh-target-1' }]
    )

    const buffer = result.terminalLayoutsByTabId['remote-tab'].buffersByLeafId?.['pane:1']
    expect(buffer).toHaveLength(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT)
    expect(buffer?.startsWith('start-')).toBe(false)
  })

  it('caps preserved SSH buffers by UTF-8 bytes for multibyte scrollback', () => {
    const multibyteRow = 'é'.repeat(1024)
    const hugeScrollback = multibyteRow.repeat(512)
    const result = pruneLocalTerminalScrollbackBuffers(
      makeSession({
        terminalLayoutsByTabId: {
          'remote-tab': {
            root: null,
            activeLeafId: null,
            expandedLeafId: null,
            buffersByLeafId: { 'pane:1': hugeScrollback }
          }
        }
      }),
      [{ id: 'remote-repo', connectionId: 'ssh-target-1' }]
    )

    const buffer = result.terminalLayoutsByTabId['remote-tab'].buffersByLeafId?.['pane:1'] ?? ''
    expect(buffer.length).toBeGreaterThan(0)
    expect(getUtf8ByteLength(buffer)).toBeLessThanOrEqual(
      TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT
    )
    expect(buffer).toHaveLength(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT / 2)
  })

  it('drops floating terminal buffers even though the synthetic worktree has no repo', () => {
    const result = pruneLocalTerminalScrollbackBuffers(
      makeSession({
        tabsByWorktree: {
          [FLOATING_TERMINAL_WORKTREE_ID]: [
            {
              id: 'floating-tab',
              title: 'floating',
              customTitle: null,
              color: null,
              sortOrder: 0,
              createdAt: 1,
              ptyId: 'floating-pty',
              worktreeId: FLOATING_TERMINAL_WORKTREE_ID
            }
          ]
        },
        terminalLayoutsByTabId: {
          'floating-tab': {
            root: null,
            activeLeafId: null,
            expandedLeafId: null,
            buffersByLeafId: { 'pane:1': 'floating-scrollback' },
            ptyIdsByLeafId: { 'pane:1': 'floating-pty' }
          }
        }
      }),
      []
    )

    expect(result.terminalLayoutsByTabId['floating-tab']).toEqual({
      root: null,
      activeLeafId: null,
      expandedLeafId: null,
      ptyIdsByLeafId: { 'pane:1': 'floating-pty' }
    })
  })

  it('treats orphaned layouts as local and prunes their buffers', () => {
    const result = pruneLocalTerminalScrollbackBuffers(
      makeSession({
        terminalLayoutsByTabId: {
          'orphan-tab': {
            root: null,
            activeLeafId: null,
            expandedLeafId: null,
            buffersByLeafId: { 'pane:1': 'orphan-scrollback' }
          }
        }
      }),
      [{ id: 'remote-repo', connectionId: 'ssh-target-1' }]
    )

    expect(result.terminalLayoutsByTabId['orphan-tab'].buffersByLeafId).toBeUndefined()
  })

  it('preserves buffers for unresolved repo catalogs until worktrees can be classified', () => {
    const result = pruneLocalTerminalScrollbackBuffers(
      makeSession({
        tabsByWorktree: {
          'remote-repo::/remote/worktree': [
            {
              id: 'remote-tab',
              title: 'remote',
              customTitle: null,
              color: null,
              sortOrder: 0,
              createdAt: 1,
              ptyId: 'remote-pty',
              worktreeId: 'remote-repo::/remote/worktree'
            }
          ]
        },
        terminalLayoutsByTabId: {
          'remote-tab': {
            root: null,
            activeLeafId: null,
            expandedLeafId: null,
            buffersByLeafId: { 'pane:1': 'maybe-remote-scrollback' }
          }
        }
      }),
      []
    )

    expect(result.terminalLayoutsByTabId['remote-tab'].buffersByLeafId).toEqual({
      'pane:1': 'maybe-remote-scrollback'
    })
  })

  it('preserves local scrollback buffers, capped like remote ones so session JSON cannot scale with raw scrollback (spec R2)', () => {
    const hugeScrollback = `start-${'x'.repeat(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT + 10)}`
    const session = makeSession({
      terminalLayoutsByTabId: {
        'local-tab': {
          root: null,
          activeLeafId: null,
          expandedLeafId: null,
          buffersByLeafId: { 'pane:1': hugeScrollback },
          scrollbackRefsByLeafId: { 'pane:1': 'v1-local' },
          ptyIdsByLeafId: { 'pane:1': 'local-pty' }
        }
      }
    })

    const result = pruneLocalTerminalScrollbackBuffers(session, [
      { id: 'local-repo', connectionId: null }
    ])

    expect(result.terminalLayoutsByTabId['local-tab'].buffersByLeafId).toBeDefined()
    const buffer = result.terminalLayoutsByTabId['local-tab'].buffersByLeafId?.['pane:1']
    expect(buffer).toHaveLength(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT)
    expect(buffer?.startsWith('start-')).toBe(false)
    expect(result.terminalLayoutsByTabId['local-tab'].scrollbackRefsByLeafId).toEqual({
      'pane:1': 'v1-local'
    })
  })
})
