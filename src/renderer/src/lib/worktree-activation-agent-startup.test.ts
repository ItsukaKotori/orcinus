import { describe, expect, it, vi } from 'vitest'
import { ensureWorktreeHasInitialTerminal } from './worktree-initial-terminal-seeding'
import {
  createMockStore,
  registerWorktreeActivationReset
} from './worktree-activation-test-harness'

registerWorktreeActivationReset()

describe('ensureWorktreeHasInitialTerminal', () => {
  it('queues a startup command when agent launch is provided', () => {
    const store = createMockStore()

    ensureWorktreeHasInitialTerminal(
      store,
      'wt-1',
      { command: 'claude "Fix this bug"' },
      undefined,
      undefined
    )

    expect(store.createTab).toHaveBeenCalledWith('wt-1', undefined, undefined, {
      pendingActivationSpawn: true
    })
    expect(store.setActiveTab).toHaveBeenCalledWith('tab-1')
    expect(store.queueTabStartupCommand).toHaveBeenCalledWith('tab-1', {
      command: 'claude "Fix this bug"'
    })
    expect(store.queueTabSetupSplit).not.toHaveBeenCalled()
    expect(store.queueTabIssueCommandSplit).not.toHaveBeenCalled()
  })

  it('opens new agent workspace terminals in terminal mode', () => {
    const store = createMockStore()

    ensureWorktreeHasInitialTerminal(
      store,
      'wt-1',
      {
        command: 'claude',
        launchAgent: 'claude'
      },
      undefined,
      undefined
    )

    expect(store.createTab).toHaveBeenCalledWith('wt-1', undefined, undefined, {
      pendingActivationSpawn: true,
      launchAgent: 'claude'
    })
    expect(store.queueTabStartupCommand).toHaveBeenCalledWith('tab-1', {
      command: 'claude',
      launchAgent: 'claude'
    })
  })

  it('opens the startup default tab in terminal mode', () => {
    let createdIndex = 0
    const createTab = vi.fn(() => ({ id: `tab-${++createdIndex}` }))
    const store = createMockStore({ createTab })

    ensureWorktreeHasInitialTerminal(
      store,
      'wt-1',
      { command: 'claude', launchAgent: 'claude' },
      undefined,
      undefined,
      { runCommands: true, tabs: [{ title: 'Claude', command: 'claude' }] }
    )

    expect(createTab).toHaveBeenNthCalledWith(1, 'wt-1', undefined, undefined, {
      pendingActivationSpawn: true,
      recordInteraction: false,
      launchAgent: 'claude'
    })
  })

  it('forwards telemetry on the queued startup so main can fire agent_started', () => {
    const store = createMockStore()

    ensureWorktreeHasInitialTerminal(
      store,
      'wt-1',
      {
        command: 'claude',
        telemetry: {
          agent_kind: 'claude-code',
          launch_source: 'new_workspace_composer',
          request_kind: 'new'
        }
      },
      undefined,
      undefined
    )

    expect(store.createTab).toHaveBeenCalledWith('wt-1', undefined, undefined, {
      pendingActivationSpawn: true,
      launchAgent: 'claude'
    })
    expect(store.queueTabStartupCommand).toHaveBeenCalledWith('tab-1', {
      command: 'claude',
      telemetry: {
        agent_kind: 'claude-code',
        launch_source: 'new_workspace_composer',
        request_kind: 'new'
      }
    })
  })

  it('stamps the tab agent from startup launchAgent without telemetry', () => {
    const store = createMockStore()

    ensureWorktreeHasInitialTerminal(
      store,
      'wt-1',
      {
        command: 'codex',
        launchAgent: 'codex'
      },
      undefined,
      undefined
    )

    expect(store.createTab).toHaveBeenCalledWith('wt-1', undefined, undefined, {
      pendingActivationSpawn: true,
      launchAgent: 'codex'
    })
    expect(store.queueTabStartupCommand).toHaveBeenCalledWith('tab-1', {
      command: 'codex',
      launchAgent: 'codex'
    })
  })
})
