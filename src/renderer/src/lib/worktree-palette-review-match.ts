import type { HostedReviewInfo } from '../../../shared/hosted-review'

// title is intentionally optional: rehydrated review/PR caches can hold entries without one.
type SearchableReview = Pick<HostedReviewInfo, 'number' | 'provider'> & { title?: string }
type WorktreePaletteReviewMatch = {
  labelKind: 'pr' | 'mr'
  text: string
  matchRange: { start: number; end: number }
}

export function matchWorktreePaletteReview(
  review: SearchableReview,
  query: string,
  numericQuery: string
): WorktreePaletteReviewMatch | null {
  const numberPrefix = 'PR #'
  const hasMergeRequestSigil = query.startsWith('!')
  const reviewNumericQuery = numericQuery
  const reviewNumberIndex = !hasMergeRequestSigil
    ? String(review.number).indexOf(reviewNumericQuery)
    : -1
  if (reviewNumericQuery && reviewNumberIndex !== -1) {
    return {
      labelKind: 'pr',
      text: `${numberPrefix}${review.number}`,
      matchRange: {
        start: numberPrefix.length + reviewNumberIndex,
        end: numberPrefix.length + reviewNumberIndex + reviewNumericQuery.length
      }
    }
  }

  // Null-safe: a cached review may have no title, so fall back to '' (query is non-empty, so it won't match).
  const title = review.title ?? ''
  const titleIndex = title.toLowerCase().indexOf(query)
  if (titleIndex === -1) {
    return null
  }
  return {
    labelKind: 'pr',
    text: title,
    matchRange: { start: titleIndex, end: titleIndex + query.length }
  }
}
