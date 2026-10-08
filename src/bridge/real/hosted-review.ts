import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { createGhExecClient, defaultGhExecutor } from '@/lib/github/gh-exec-client'
import { createHostedReviewClient } from '@/lib/github/hosted-review'
import { createPRForBranchLookup } from '@/lib/github/pr-for-branch'
import { createRepoIdentityResolver } from '@/lib/github/repo-identity'
import { invokeCommand } from './invoke'

/**
 * Real `hostedReview` adapter (spec §3.2): 2D.1 ports the read-only
 * `forBranch` poll through the GitHub PR lookup; creation and eligibility stay
 * on the rejecting method fallback until 2D.2.
 */
export function createHostedReviewRealApi(): PreloadApi['hostedReview'] {
  const client = createGhExecClient(defaultGhExecutor())
  const readRemoteUrls = (worktreePath: string) =>
    invokeCommand<{ name: string; url: string }[]>('git_remote_urls', {
      args: { worktreePath }
    })
  const identity = createRepoIdentityResolver({ client, readRemoteUrls })
  const lookup = createPRForBranchLookup({ client, identity })
  const hostedReview = createHostedReviewClient({ identity, lookup })

  return withMethodFallback<PreloadApi['hostedReview']>('hostedReview', {
    forBranch: (args) => hostedReview.forBranch(args)
  })
}
