import type { PreloadApi } from '../../../../preload/api-types'
import type { RuntimeSyncWindowGraph } from '../../../../shared/runtime-types'
import { callRuntimeEnvelope, getRemoteRuntimeStatus } from './web-runtime-calls'
import {
  getClientForEnvironment,
  manuallyDisconnectedEnvironmentIds,
  requireActiveEnvironment
} from './web-runtime-session'
import { noopUnsubscribe } from './web-storage'

export function createWebRuntimeApi(): NonNullable<Partial<PreloadApi>['runtime']> {
  return {
    syncWindowGraph: async (_graph: RuntimeSyncWindowGraph) => getRemoteRuntimeStatus(),
    getStatus: () => getRemoteRuntimeStatus(),
    call: ({ method, params }) => callRuntimeEnvelope(method, params),
    subscribe: async ({ method, params }, callback) => {
      const environment = requireActiveEnvironment()
      const subscription = await getClientForEnvironment(environment).subscribe(method, params, {
        onResponse: callback
      })
      if (manuallyDisconnectedEnvironmentIds.has(environment.id)) {
        subscription.unsubscribe()
        throw new Error('runtime_manually_disconnected')
      }
      return subscription
    },
    getTerminalFitOverrides: () => Promise.resolve([]),
    getTerminalDrivers: () => Promise.resolve([]),
    restoreTerminalFit: () => Promise.resolve({ restored: false }),
    onTerminalFitOverrideChanged: () => noopUnsubscribe,
    onTerminalDriverChanged: () => noopUnsubscribe
  }
}
