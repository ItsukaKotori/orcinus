// Phase 0 mock; A has no hosted-review service, so the per-worktree branch poll
// answers "no review" instead of rejecting (rejections spammed console.error
// and blanked the sidebar badges). Mutations reject loudly.
import type { HostedReviewApi } from '../../shared/preload-api/api/hosted-review-api'
import { withMethodFallback } from '../unimplemented-fallback'

export function createHostedReviewApi(): HostedReviewApi {
  return withMethodFallback<HostedReviewApi>('hostedReview', {
    forBranch: async () => null
  })
}
