import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AutomationDispatchRequest, AutomationRun } from '../../shared/automations-types'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createAutomationsRealApi } from './automations'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

const runFixture = {
  id: 'run-1',
  automationId: 'auto-1',
  title: 'Run 1',
  scheduledFor: 1_000,
  status: 'dispatched',
  trigger: 'scheduled'
} as unknown as AutomationRun

beforeEach(() => {
  invokeMock.mockReset()
  listenMock.mockReset()
})

const explicitMethods = [
  'runPrecheck',
  'markDispatchResult',
  'snapshotWorkspaceName',
  'rendererReady',
  'onDispatchRequested',
  'onChanged'
] as const satisfies readonly (keyof PreloadApi['automations'])[]

const externalMethods = [
  'listExternalManagerForOwner',
  'listExternalRunsForOwner',
  'createExternalForOwner',
  'updateExternalForOwner',
  'runExternalActionForOwner',
  'retainExternalScopes'
] as const satisfies readonly (keyof PreloadApi['automations'])[]

describe('automations real adapter commands', () => {
  it('explicitly implements every host-backed method', () => {
    const automations = createAutomationsRealApi() as unknown as Record<string, unknown>
    for (const method of explicitMethods) {
      expect(
        Object.prototype.hasOwnProperty.call(automations, method),
        `automations.${method} must be explicit (not a fabricated fallback)`
      ).toBe(true)
      expect(typeof automations[method]).toBe('function')
    }
  })

  it('maps runPrecheck to automations_run_precheck and unwraps { result }', async () => {
    const precheckResult = {
      command: 'echo ok',
      exitCode: 0,
      timedOut: false,
      durationMs: 5,
      stdout: 'ok\n',
      stderr: '',
      stdoutTruncated: false,
      stderrTruncated: false,
      error: null,
      startedAt: 1,
      completedAt: 6
    }
    invokeMock.mockResolvedValueOnce({ result: precheckResult })
    await expect(
      createAutomationsRealApi().runPrecheck({ automationId: 'auto-1', runId: 'run-1' })
    ).resolves.toEqual(precheckResult)
    expect(invokeMock).toHaveBeenCalledWith('automations_run_precheck', {
      args: { automationId: 'auto-1', runId: 'run-1' }
    })
  })

  it('resolves runPrecheck to null when the host has no precheck result', async () => {
    invokeMock.mockResolvedValueOnce({ result: null })
    await expect(
      createAutomationsRealApi().runPrecheck({ automationId: 'auto-1', runId: 'run-1' })
    ).resolves.toBeNull()
  })

  it('maps markDispatchResult to automations_mark_dispatch_result and unwraps { run }', async () => {
    invokeMock.mockResolvedValueOnce({ run: runFixture })
    const result = {
      runId: 'run-1',
      status: 'completed',
      outputSnapshot: null,
      error: null
    } as const
    await expect(createAutomationsRealApi().markDispatchResult(result)).resolves.toEqual(runFixture)
    expect(invokeMock).toHaveBeenCalledWith('automations_mark_dispatch_result', {
      args: { runId: 'run-1', result }
    })
  })

  it('passes the host { run: null } through when the run is already gone', async () => {
    // Why pinned: the preload contract has no null slot, but the host answers
    // null for a run deleted mid-dispatch and every renderer caller ignores the
    // reply, so the adapter passes it through instead of fabricating a run.
    invokeMock.mockResolvedValueOnce({ run: null })
    await expect(
      createAutomationsRealApi().markDispatchResult({ runId: 'run-gone', status: 'completed' })
    ).resolves.toBeNull()
  })

  it('maps snapshotWorkspaceName to automations_snapshot_workspace_name and returns updated', async () => {
    invokeMock.mockResolvedValueOnce({ updated: 3 })
    await expect(
      createAutomationsRealApi().snapshotWorkspaceName({
        workspaceId: 'wt-1',
        displayName: 'Renamed'
      })
    ).resolves.toBe(3)
    expect(invokeMock).toHaveBeenCalledWith('automations_snapshot_workspace_name', {
      args: { workspaceId: 'wt-1', displayName: 'Renamed' }
    })
  })

  it('maps rendererReady to the bare automations_renderer_ready command', async () => {
    invokeMock.mockResolvedValueOnce(null)
    await expect(createAutomationsRealApi().rendererReady()).resolves.toBeUndefined()
    expect(invokeMock).toHaveBeenCalledWith('automations_renderer_ready')
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'Automation run not found.' })
    await expect(
      createAutomationsRealApi().runPrecheck({ automationId: 'auto-1', runId: 'run-1' })
    ).rejects.toThrow('Automation run not found.')
  })
})

describe('automations real adapter events', () => {
  it('forwards automations:changed payloads and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()
    const unsubscribe = createAutomationsRealApi().onChanged(callback)

    expect(listenMock).toHaveBeenCalledWith('automations:changed', expect.any(Function))
    handlers[0]?.({ payload: { reason: 'run' } })
    expect(callback).toHaveBeenCalledWith({ reason: 'run' })

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('forwards automations:dispatch-requested requests and returns an unsubscriber', async () => {
    const unlisten = vi.fn()
    const handlers: Array<(event: { payload: unknown }) => void> = []
    listenMock.mockImplementationOnce(async (_event, callback) => {
      handlers.push(callback as (event: { payload: unknown }) => void)
      return unlisten
    })
    const callback = vi.fn()
    const request = {
      automation: { id: 'auto-1' },
      run: runFixture,
      dispatchToken: 'token-1'
    } as unknown as AutomationDispatchRequest
    const unsubscribe = createAutomationsRealApi().onDispatchRequested(callback)

    expect(listenMock).toHaveBeenCalledWith('automations:dispatch-requested', expect.any(Function))
    handlers[0]?.({ payload: request })
    expect(callback).toHaveBeenCalledWith(request)

    unsubscribe()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})

describe('automations real adapter external fallback', () => {
  it.each(externalMethods)('%s stays on the rejecting fallback', async (method) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const automations = createAutomationsRealApi() as unknown as Record<
      string,
      (args: unknown) => Promise<unknown>
    >
    expect(Object.prototype.hasOwnProperty.call(automations, method)).toBe(false)
    await expect(automations[method]({})).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
