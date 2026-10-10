import type {
  AutomationDispatchRequest,
  AutomationPrecheckResult,
  AutomationRun
} from '../../shared/automations-types'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import type { AutomationsChangedPayload } from '../../shared/runtime-client-events'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Real `automations` adapter: the host-backed halves of the contract only.
 * CRUD rides the local `automation.*` runtime RPC, so this surface carries the
 * dispatch-loop plumbing (precheck, dispatch-result write-back, workspace-name
 * snapshot, renderer arming) and the two host events. The external-manager
 * scope and `retainExternalScopes` stay on the rejecting fallback until their
 * host lands.
 */
export function createAutomationsRealApi(): PreloadApi['automations'] {
  return withMethodFallback<PreloadApi['automations']>('automations', {
    runPrecheck: async ({ automationId, runId }) => {
      const response = await invokeCommand<{ result: AutomationPrecheckResult | null }>(
        'automations_run_precheck',
        { args: { automationId, runId } }
      )
      return response.result
    },
    markDispatchResult: async (result) => {
      const response = await invokeCommand<{ run: AutomationRun | null }>(
        'automations_mark_dispatch_result',
        { args: { runId: result.runId, result } }
      )
      // Why: the host answers `{run: null}` when the run is already gone (its
      // automation was deleted mid-dispatch). The preload contract has no null
      // slot, but every renderer caller ignores the reply, so pass the honest
      // value through instead of fabricating or rejecting.
      return response.run as AutomationRun
    },
    snapshotWorkspaceName: async ({ workspaceId, displayName }) => {
      const response = await invokeCommand<{ updated: number }>(
        'automations_snapshot_workspace_name',
        { args: { workspaceId, displayName } }
      )
      return response.updated
    },
    rendererReady: async () => {
      await invokeCommand('automations_renderer_ready')
    },
    onDispatchRequested: (callback) =>
      subscribeToEvent<AutomationDispatchRequest>('automations:dispatch-requested', callback),
    onChanged: (callback) =>
      subscribeToEvent<AutomationsChangedPayload>('automations:changed', callback)
  })
}
