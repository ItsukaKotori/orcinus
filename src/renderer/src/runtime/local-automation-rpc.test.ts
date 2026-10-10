import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AutomationListResult } from '../../../shared/automation-list-scope'

const { invokeCommandMock } = vi.hoisted(() => ({ invokeCommandMock: vi.fn() }))

vi.mock('../../../bridge/real/invoke', () => ({ invokeCommand: invokeCommandMock }))

import { callLocalAutomationRpc, isLocalAutomationRpcMethod } from './local-automation-rpc'

const WIRE_OWNER = { selector: { kind: 'self' } }
const WIRE_DESTINATION = {
  selector: { kind: 'ssh', targetId: 'ssh-1', targetGeneration: 7 }
}

beforeEach(() => {
  invokeCommandMock.mockReset()
})

describe('local automation RPC adapter', () => {
  it('recognizes exactly the six intercepted automation methods', () => {
    for (const method of [
      'automation.list',
      'automation.create',
      'automation.update',
      'automation.delete',
      'automation.runNow',
      'automation.runs'
    ]) {
      expect(isLocalAutomationRpcMethod(method)).toBe(true)
    }
    expect(isLocalAutomationRpcMethod('automation.runPrecheck')).toBe(false)
    expect(isLocalAutomationRpcMethod('automations.list')).toBe(false)
    expect(isLocalAutomationRpcMethod('terminal.list')).toBe(false)
  })

  describe('automation.list', () => {
    it('answers the scoped contract with Self items over the bare host list', async () => {
      const automations = [{ id: 'auto-1' }, { id: 'auto-2' }]
      invokeCommandMock.mockResolvedValue(automations)

      const result = await callLocalAutomationRpc<AutomationListResult>('automation.list', {
        selector: { kind: 'ssh', targetId: 'ssh-1', expectedTargetGeneration: 7 }
      })

      expect(invokeCommandMock).toHaveBeenCalledWith('automations_list')
      expect(result).toEqual({
        automations,
        items: [
          { automationId: 'auto-1', selector: { kind: 'self' } },
          { automationId: 'auto-2', selector: { kind: 'self' } }
        ]
      })
    })

    it('treats a missing selector request the same way', async () => {
      invokeCommandMock.mockResolvedValue([])

      await expect(callLocalAutomationRpc('automation.list', undefined)).resolves.toEqual({
        automations: [],
        items: []
      })
      expect(invokeCommandMock).toHaveBeenCalledWith('automations_list')
    })
  })

  describe('automation.create', () => {
    it('renames wire selectors back to projectId/workspaceId and strips fencing', async () => {
      const automation = { id: 'auto-new' }
      invokeCommandMock.mockResolvedValue({ automation })

      const result = await callLocalAutomationRpc<{ automation: { id: string } }>(
        'automation.create',
        {
          name: 'Nightly',
          prompt: 'Do it',
          agentId: 'claude',
          workspaceMode: 'existing',
          repo: 'id:repo-1',
          workspace: 'id:ws-1',
          timezone: 'UTC',
          rrule: 'FREQ=HOURLY;INTERVAL=1',
          dtstart: 1000,
          creationKey: 'key-1',
          destination: WIRE_DESTINATION
        }
      )

      expect(invokeCommandMock).toHaveBeenCalledWith('automations_create', {
        args: {
          input: {
            name: 'Nightly',
            prompt: 'Do it',
            agentId: 'claude',
            workspaceMode: 'existing',
            projectId: 'repo-1',
            workspaceId: 'ws-1',
            timezone: 'UTC',
            rrule: 'FREQ=HOURLY;INTERVAL=1',
            dtstart: 1000,
            creationKey: 'key-1'
          }
        }
      })
      expect(result).toEqual({ automation })
    })

    it('stores no workspace for a per-run workspace create', async () => {
      invokeCommandMock.mockResolvedValue({ automation: { id: 'auto-new' } })

      await callLocalAutomationRpc('automation.create', {
        name: 'Nightly',
        repo: 'id:repo-1',
        workspace: undefined,
        workspaceMode: 'new_per_run'
      })

      expect(invokeCommandMock).toHaveBeenCalledWith('automations_create', {
        args: {
          input: {
            name: 'Nightly',
            workspaceMode: 'new_per_run',
            projectId: 'repo-1',
            workspaceId: null
          }
        }
      })
    })
  })

  describe('automation.update', () => {
    it('strips fencing and maps wire selectors inside updates', async () => {
      const automation = { id: 'auto-1', enabled: false }
      invokeCommandMock.mockResolvedValue({ automation })

      const result = await callLocalAutomationRpc<{ automation: { id: string } }>(
        'automation.update',
        {
          id: 'auto-1',
          updates: { enabled: false, repo: 'id:repo-2', workspace: 'id:ws-2' },
          expectedOwner: WIRE_OWNER,
          destination: WIRE_DESTINATION
        }
      )

      expect(invokeCommandMock).toHaveBeenCalledWith('automations_update', {
        args: {
          id: 'auto-1',
          updates: { enabled: false, projectId: 'repo-2', workspaceId: 'ws-2' }
        }
      })
      expect(result).toEqual({ automation })
    })

    it('clears the workspace when the wire carries an empty workspace selector', async () => {
      invokeCommandMock.mockResolvedValue({ automation: { id: 'auto-1' } })

      await callLocalAutomationRpc('automation.update', {
        id: 'auto-1',
        updates: { workspace: undefined },
        expectedOwner: WIRE_OWNER
      })

      expect(invokeCommandMock).toHaveBeenCalledWith('automations_update', {
        args: {
          id: 'auto-1',
          updates: { workspaceId: null }
        }
      })
    })
  })

  describe('automation.delete and automation.runNow', () => {
    it('forwards only the id to delete', async () => {
      invokeCommandMock.mockResolvedValue({ removed: true })

      await expect(
        callLocalAutomationRpc('automation.delete', { id: 'auto-1', expectedOwner: WIRE_OWNER })
      ).resolves.toEqual({ removed: true })
      expect(invokeCommandMock).toHaveBeenCalledWith('automations_delete', {
        args: { id: 'auto-1' }
      })
    })

    it('forwards only the id to run now', async () => {
      const run = { id: 'run-1', automationId: 'auto-1' }
      invokeCommandMock.mockResolvedValue({ run })

      await expect(
        callLocalAutomationRpc('automation.runNow', { id: 'auto-1', expectedOwner: WIRE_OWNER })
      ).resolves.toEqual({ run })
      expect(invokeCommandMock).toHaveBeenCalledWith('automations_run_now', {
        args: { id: 'auto-1' }
      })
    })
  })

  describe('automation.runs', () => {
    it('keeps only automationId/limit/cursor and returns the page shape', async () => {
      const page = { runs: [{ id: 'run-1' }], nextCursor: 'cursor-2' }
      invokeCommandMock.mockResolvedValue(page)

      const result = await callLocalAutomationRpc('automation.runs', {
        automationId: 'auto-1',
        expectedOwner: WIRE_OWNER,
        limit: 25,
        cursor: 'cursor-1',
        locked: true
      })

      expect(invokeCommandMock).toHaveBeenCalledWith('automations_runs', {
        args: {
          automationId: 'auto-1',
          limit: 25,
          cursor: 'cursor-1'
        }
      })
      expect(result).toEqual(page)
    })

    it('omits absent options instead of sending them as undefined', async () => {
      invokeCommandMock.mockResolvedValue({ runs: [], nextCursor: null })

      await callLocalAutomationRpc('automation.runs', { automationId: 'auto-1' })

      expect(invokeCommandMock).toHaveBeenCalledWith('automations_runs', {
        args: { automationId: 'auto-1' }
      })
    })
  })

  it('rejects unsupported automation methods with method_not_found', async () => {
    await expect(callLocalAutomationRpc('automation.runPrecheck', {})).rejects.toMatchObject({
      name: 'RuntimeRpcCallError',
      code: 'method_not_found'
    })
    expect(invokeCommandMock).not.toHaveBeenCalled()
  })
})
