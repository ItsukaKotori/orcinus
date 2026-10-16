import { createElement } from 'react'
import { cn } from '@/lib/utils'
import { getReviewStateIcon } from '@/components/github/review-state-presentation'
import { PullRequestIcon } from './WorktreeCardHelpers'
import type { WorktreeCardPrDisplay } from './worktree-card-pr-display'

export function getReviewLabel(_review: WorktreeCardPrDisplay): 'MR' | 'PR' {
  return 'PR'
}

export function getProviderName(review: WorktreeCardPrDisplay): string {
  if (review.provider === 'bitbucket') {
    return 'Bitbucket'
  }
  if (review.provider === 'azure-devops') {
    return 'Azure DevOps'
  }
  if (review.provider === 'gitea') {
    return 'Gitea'
  }
  return 'GitHub'
}

// Why: checks only gate a review that is actually open; draft/closed/merged keep
// their state tone so the glyph agrees with its tooltip. A stateless row (folder
// cards render one while a linked review is loading or its details failed) has no
// state glyph to contradict, so it still flags problems — but never claims success,
// since emerald would assert an open review we have not confirmed.
function getCheckTone(review: WorktreeCardPrDisplay): string | null {
  if (review.state && review.state !== 'open') {
    return null
  }
  if (review.status === 'failure') {
    return 'text-rose-500/85'
  }
  if (review.status === 'pending') {
    return 'text-amber-500/85'
  }
  if (review.state === 'open' && review.status === 'success') {
    return 'text-emerald-500/80'
  }
  return null
}

function getStateTone(state: WorktreeCardPrDisplay['state']): string {
  if (state === 'merged') {
    return 'text-purple-600/70 dark:text-purple-400/70'
  }
  if (state === 'open') {
    return 'text-emerald-500/80'
  }
  if (state === 'closed') {
    return 'text-muted-foreground/60'
  }
  if (state === 'draft') {
    return 'text-muted-foreground/50'
  }
  return 'text-muted-foreground opacity-70'
}

export function ReviewIcon({
  review,
  className
}: {
  review: WorktreeCardPrDisplay
  className?: string
}): React.JSX.Element {
  const Icon = getReviewStateIcon(review.state) ?? PullRequestIcon
  return createElement(Icon, {
    className: cn(className, getCheckTone(review) ?? getStateTone(review.state))
  })
}
