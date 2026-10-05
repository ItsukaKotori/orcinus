import type { PreloadApi } from '../../shared/preload-api/api-types'
import type {
  PreflightRuntimeContext,
  RefreshAgentsResult
} from '../../shared/preload-api/api/preflight-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * Real `preflight` adapter (spec §5.10). Git availability reuses the `repos`
 * machine probe (`repos_is_git_available`) so the landing setup card reflects the
 * host instead of the all-negative Phase 0 mock; gh credentials are not wired
 * yet and report unavailable. Agent detection goes through
 * `preflight_refresh_agents` (login-shell PATH hydration + claude/codex probe,
 * process-cached after the first call); the remote/WSL runtime context is
 * accepted for contract parity and ignored by the local host.
 */
export function createPreflightRealApi(): PreloadApi['preflight'] {
  return withMethodFallback<PreloadApi['preflight']>('preflight', {
    check: async () => ({
      git: { installed: await invokeCommand<boolean>('repos_is_git_available') },
      gh: { installed: false, authenticated: false }
    }),
    detectAgents: (args?: PreflightRuntimeContext) =>
      invokeCommand<RefreshAgentsResult>('preflight_refresh_agents', {
        args
      }).then(r => r.agents),
    refreshAgents: (args?: PreflightRuntimeContext) =>
      invokeCommand<RefreshAgentsResult>('preflight_refresh_agents', { args }),
    resolveAgentProviderSession: (args) =>
      invokeCommand('agent_sessions_resolve_capture', { args })
  })
}
