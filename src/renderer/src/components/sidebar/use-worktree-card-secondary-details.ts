import { hasWorktreeCardDetails } from './WorktreeCardMetaBadges'
import { usePromptCacheCountdownStartedAt } from './CacheTimer'
import { useWorktreeAgentRows } from './useWorktreeAgentRows'
import type { WorktreeCardProps } from './worktree-card-model'
import type { useWorktreeCardFoundation } from './use-worktree-card-foundation'

type Foundation = ReturnType<typeof useWorktreeCardFoundation>

export function useWorktreeCardSecondaryDetails({
  worktree,
  cardProps,
  newCardStyle,
  compactCards,
  agentActivityDisplayMode,
  settings,
  showStatus,
  showAutomation,
  showCli,
  showComment
}: Pick<WorktreeCardProps, 'worktree'> &
  Pick<
    Foundation,
    | 'cardProps'
    | 'newCardStyle'
    | 'compactCards'
    | 'agentActivityDisplayMode'
    | 'settings'
  > & {
    showStatus: boolean
    showAutomation: boolean
    showCli: boolean
    showComment: boolean
  }) {
  // Why: unread lives in the left status lane, so the Status toggle owns the dot slot and unread emphasis.
  const showUnreadEmphasis = showStatus && worktree.isUnread
  const hoverComment = worktree.comment
  const metaAutomationProvenance = showAutomation ? worktree.automationProvenance : null
  const metaCliProvenance = showCli ? worktree.cliProvenance : null
  const metaComment = showComment ? hoverComment : null
  const showInlineAgentList = cardProps.includes('inline-agents') && (newCardStyle || !compactCards)
  const compactInlineAgentRows = useWorktreeAgentRows(
    worktree.id,
    showInlineAgentList && agentActivityDisplayMode === 'compact'
  )
  const compactInlineAgentRowsVisible =
    showInlineAgentList &&
    agentActivityDisplayMode === 'compact' &&
    compactInlineAgentRows.length > 0
  const showAggregateCacheTimer = !compactCards && !compactInlineAgentRowsVisible
  const hasDetails = hasWorktreeCardDetails({
    comment: metaComment,
    automationProvenance: metaAutomationProvenance,
    cliProvenance: metaCliProvenance
  })
  const cacheStartedAt = usePromptCacheCountdownStartedAt(worktree.id, showAggregateCacheTimer)
  // Why: derived from the settings the card already subscribes to — a third store
  // subscription for this one field costs a listener per card on every store write.
  const cacheTtlMs = showAggregateCacheTimer ? (settings?.promptCacheTtlMs ?? 0) : 0

  return {
    showUnreadEmphasis,
    hoverComment,
    metaAutomationProvenance,
    metaCliProvenance,
    metaComment,
    showInlineAgentList,
    compactInlineAgentRows,
    hasDetails,
    cacheStartedAt,
    cacheTtlMs
  }
}
