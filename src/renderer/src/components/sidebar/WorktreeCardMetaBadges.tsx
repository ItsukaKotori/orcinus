import React from 'react'
import { CalendarClock, SquareTerminal, StickyNote } from 'lucide-react'
import { cn } from '@/lib/utils'
import { MetaIconBadge } from './WorktreeCardMetadataControls'
import type {
  WorktreeCardMetaBadgesProps,
  WorktreeCardMetaBadgesRootProps
} from './worktree-card-meta-types'
import { translate } from '@/i18n/i18n'

function hasComment(comment: string | null): boolean {
  return (comment ?? '').trim().length > 0
}

export function hasWorktreeCardDetails({
  comment,
  automationProvenance,
  cliProvenance
}: WorktreeCardMetaBadgesProps): boolean {
  return Boolean(hasComment(comment) || automationProvenance || cliProvenance)
}

export const WorktreeCardMetaBadges = React.forwardRef<
  HTMLDivElement,
  WorktreeCardMetaBadgesRootProps
>(function WorktreeCardMetaBadges(
  { comment, automationProvenance, cliProvenance, className, ...props },
  ref
): React.JSX.Element | null {
  if (
    !hasWorktreeCardDetails({
      comment,
      automationProvenance,
      cliProvenance
    })
  ) {
    return null
  }

  return (
    // Why: Radix HoverCardTrigger uses `asChild`, so this group must forward
    // trigger props/ref to the actual DOM node for attachment-only hover.
    <div
      ref={ref}
      {...props}
      className={cn('ml-auto flex shrink-0 items-center gap-1 pr-1.5', className)}
      aria-label={translate(
        'auto.components.sidebar.WorktreeCardMeta.3e65e11cc6',
        'Workspace metadata'
      )}
    >
      {hasComment(comment) && (
        <MetaIconBadge
          label={translate(
            'auto.components.sidebar.WorktreeCardMeta.fe075cb851',
            'Workspace notes'
          )}
        >
          <StickyNote className="text-muted-foreground" />
        </MetaIconBadge>
      )}
      {automationProvenance && (
        <MetaIconBadge
          label={translate(
            'auto.components.sidebar.WorktreeCardMeta.automationCreated',
            'Created by automation'
          )}
        >
          <CalendarClock className="text-muted-foreground" />
        </MetaIconBadge>
      )}
      {cliProvenance && (
        <MetaIconBadge
          label={translate(
            'auto.components.sidebar.WorktreeCardMeta.cliCreated',
            'Created by Orca CLI'
          )}
        >
          <SquareTerminal className="text-muted-foreground" />
        </MetaIconBadge>
      )}
    </div>
  )
})
