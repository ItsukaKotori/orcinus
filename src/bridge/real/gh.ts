import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { computeAuthDiagnostic, parseAuthStatus } from '@/lib/github/auth-diagnose'
import { createGhExecClient, defaultGhExecutor } from '@/lib/github/gh-exec-client'
import { createPRChecksClient } from '@/lib/github/pr-checks'
import { createPRForBranchLookup } from '@/lib/github/pr-for-branch'
import { createRateLimitClient } from '@/lib/github/rate-limit'
import { createRepoIdentityResolver } from '@/lib/github/repo-identity'
import { invokeCommand } from './invoke'

const noopUnsubscribe = (): void => {}

/**
 * Real `gh` adapter (spec §3.2): the Phase 2D.1 read-only surface backed by the
 * Rust `gh_exec` runner. Identity resolution, PR-for-branch lookup, checks and
 * the rate-limit snapshot are composed from the renderer-side clients; every
 * mutating or unported method stays on the rejecting method fallback.
 */
export function createGhRealApi(): PreloadApi['gh'] {
  const client = createGhExecClient(defaultGhExecutor())
  const readRemoteUrls = (worktreePath: string) =>
    invokeCommand<{ name: string; url: string }[]>('git_remote_urls', {
      args: { worktreePath }
    })
  const identity = createRepoIdentityResolver({ client, readRemoteUrls })
  const lookup = createPRForBranchLookup({ client, identity })
  const checks = createPRChecksClient({ client, identity })
  const rateLimit = createRateLimitClient({ client })

  return withMethodFallback<PreloadApi['gh']>('gh', {
    diagnoseAuth: async (args) => {
      const host = args?.host
      let accounts: ReturnType<typeof parseAuthStatus> = []
      let ghAvailable = true
      try {
        const result = await client.run(['auth', 'status'])
        accounts = parseAuthStatus(`${result.stdout}\n${result.stderr}`)
      } catch {
        ghAvailable = false
      }
      const env = await invokeCommand<{ token: 'GH_TOKEN' | 'GITHUB_TOKEN' | null }>(
        'gh_env_probe'
      )
      return computeAuthDiagnostic({
        accounts,
        ghAvailable,
        envTokenInProcess: env.token,
        requiredHost: host ?? null
      })
    },
    repoSlug: (args) => identity.getRepoSlug(args.repoPath),
    repoUpstream: (args) => identity.getRepoUpstream(args.repoPath),
    prForBranch: (args) =>
      lookup.getPRForBranch({
        worktreePath: args.repoPath,
        branch: args.branch,
        ...(args.linkedPRNumber !== undefined ? { linkedPRNumber: args.linkedPRNumber } : {}),
        ...(args.fallbackPRNumber !== undefined ? { fallbackPRNumber: args.fallbackPRNumber } : {}),
        ...(args.acceptMergedFallbackPR !== undefined
          ? { acceptMergedFallbackPR: args.acceptMergedFallbackPR }
          : {}),
        ...(args.currentHeadOid !== undefined ? { currentHeadOid: args.currentHeadOid } : {})
      }),
    refreshPRNow: (args) => {
      const candidate = args.candidate
      return lookup.getPRForBranchOutcome({
        worktreePath: candidate.repoPath,
        branch: candidate.branch,
        ...(candidate.linkedPRNumber !== undefined
          ? { linkedPRNumber: candidate.linkedPRNumber }
          : {}),
        ...(candidate.fallbackPRNumber !== undefined
          ? { fallbackPRNumber: candidate.fallbackPRNumber }
          : {}),
        ...(candidate.currentHeadOid !== undefined
          ? { currentHeadOid: candidate.currentHeadOid }
          : {})
      })
    },
    prChecks: async (args) => {
      const repo = args.prRepo ?? (await identity.getRepoSlug(args.repoPath))
      if (!repo) {
        return []
      }
      return checks.getPRChecks({
        repo,
        prNumber: args.prNumber,
        ...(args.headSha !== undefined ? { headSha: args.headSha } : {}),
        ...(args.noCache !== undefined ? { noCache: args.noCache } : {})
      })
    },
    prCheckDetails: async (args) => {
      const repo = args.prRepo ?? (await identity.getRepoSlug(args.repoPath))
      if (!repo) {
        return null
      }
      return checks.getPRCheckDetails({
        repo,
        ...(args.checkRunId !== undefined ? { checkRunId: args.checkRunId } : {}),
        ...(args.workflowRunId !== undefined ? { workflowRunId: args.workflowRunId } : {}),
        ...(args.checkName !== undefined ? { checkName: args.checkName } : {}),
        ...(args.url != null ? { url: args.url } : {})
      })
    },
    rateLimit: (args) => rateLimit.getRateLimit(args ?? undefined),
    onPRRefreshEvent: () => noopUnsubscribe
  })
}
