import type { PreloadApi } from '../../shared/preload-api/api-types'
import type {
  PreflightRuntimeContext,
  RefreshAgentsResult
} from '../../shared/preload-api/api/preflight-api'
import { createGhExecClient, defaultGhExecutor } from '@/lib/github/gh-exec-client'
import { createGhReadinessProbe } from '@/lib/github/preflight-gh'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

// Why module-level: the landing cards probe `check` from several mounts, and a
// gh spawn per mount is wasteful; the probe caches its answer for 60s.
const ghReadinessProbe = createGhReadinessProbe({
  client: createGhExecClient(defaultGhExecutor())
})

/**
 * Real `preflight` adapter (spec §5.10). Git availability reuses the `repos`
 * machine probe (`repos_is_git_available`) so the landing setup card reflects the
 * host instead of the all-negative Phase 0 mock; gh readiness comes from the
 * cached `gh auth status` probe. Agent detection goes through
 * `preflight_refresh_agents` (login-shell PATH hydration + claude/codex probe,
 * process-cached after the first call); the remote/WSL runtime context is
 * accepted for contract parity and ignored by the local host.
 */
export function createPreflightRealApi(): PreloadApi['preflight'] {
  return withMethodFallback<PreloadApi['preflight']>('preflight', {
    check: async () => ({
      git: { installed: await invokeCommand<boolean>('repos_is_git_available') },
      gh: await ghReadinessProbe()
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
