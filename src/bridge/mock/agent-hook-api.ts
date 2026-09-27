// Phase 0 mock; A has no host hooks service, so the workspace setup-script
// probes answer benignly (the prompt card treats a rejection as a check
// failure) and the rest rejects loudly.
import type { HooksApi } from '../../shared/preload-api/api/agent-hook-api'
import { withMethodFallback } from '../unimplemented-fallback'

export function createHooksApi(): HooksApi {
  return withMethodFallback<HooksApi>('hooks', {
    check: async () => ({ status: 'ok', hasHooks: false, hooks: null, mayNeedUpdate: false }),
    inspectSetupScriptImports: async () => []
  })
}
