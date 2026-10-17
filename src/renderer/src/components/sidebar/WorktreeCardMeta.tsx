import React from 'react'
import { HoverCard, HoverCardTrigger, HoverCardContent } from '@/components/ui/hover-card'
import { Pencil, StickyNote } from 'lucide-react'
import { SelectedTextCopyMenu } from '@/components/SelectedTextCopyMenu'
import { WORKTREE_NATIVE_CONTEXT_MENU_ATTR } from './WorktreeContextMenu'
import {
  WorktreeCardDetailSection,
  WorktreeCardDetailSectionContent
} from './WorktreeCardDetailSection'
import { DetailHeader, MetadataActionIcon } from './WorktreeCardMetadataControls'
import { hasWorktreeCardDetails, WorktreeCardMetaBadges } from './WorktreeCardMetaBadges'
import { useWorktreeCardDetailsHoverControl } from './worktree-card-details-hover-state'
import type {
  WorktreeCardMetaBadgesProps,
  WorktreeCardMetaBadgesRootProps,
  WorktreeCardDetailsHoverProps
} from './worktree-card-meta-types'
import { translate } from '@/i18n/i18n'
import { WorktreeCardCliDetailSection } from './WorktreeCardCliDetailSection'
import { WorktreeCardHoverIdentityHeader } from './WorktreeCardHoverIdentityHeader'
import { CommentMarkdownAsync, preloadCommentMarkdown } from './comment-markdown-lazy'

const COMMENT_MARKDOWN_CLASS_NAME =
  'text-[11.5px] text-foreground break-words leading-normal [&_.comment-md-p]:block [&_.comment-md-p+.comment-md-p]:mt-1'

export type {
  WorktreeCardMetaBadgesProps,
  WorktreeCardMetaBadgesRootProps,
  WorktreeCardDetailsHoverProps
}

// Re-exported so existing importers keep their entry point after the badge
// row moved into its own module.
export { hasWorktreeCardDetails, WorktreeCardMetaBadges }

function hasComment(comment: string | null): boolean {
  return (comment ?? '').trim().length > 0
}

export function WorktreeCardDetailsHover({
  comment,
  cliProvenance,
  children,
  branchName,
  workspaceTitle,
  identityOrder = 'workspace-first',
  workspaceTitleRenameDisabled = false,
  detailsAfter,
  openDelay = 250,
  closeDelay = 120,
  onRenameWorkspaceTitle,
  onWorkspaceTitleEditingChange,
  onEditComment,
  hoverControl
}: WorktreeCardDetailsHoverProps): React.JSX.Element {
  const internalHoverControl = useWorktreeCardDetailsHoverControl()
  const { hoverOpen, handleHoverOpenChange } = hoverControl ?? internalHoverControl
  const [workspaceTitleEditing, setWorkspaceTitleEditing] = React.useState(false)
  const pendingWorkspaceTitleCloseRef = React.useRef(false)
  const handleWorkspaceTitleEditingChange = React.useCallback(
    (editing: boolean): void => {
      setWorkspaceTitleEditing(editing)
      onWorkspaceTitleEditingChange?.(editing)
      if (!editing && pendingWorkspaceTitleCloseRef.current) {
        pendingWorkspaceTitleCloseRef.current = false
        handleHoverOpenChange(false)
      }
    },
    [handleHoverOpenChange, onWorkspaceTitleEditingChange]
  )
  const handleEffectiveHoverOpenChange = React.useCallback(
    (next: boolean): void => {
      if (!next && workspaceTitleEditing) {
        pendingWorkspaceTitleCloseRef.current = true
        return
      }
      pendingWorkspaceTitleCloseRef.current = false
      handleHoverOpenChange(next)
    },
    [handleHoverOpenChange, workspaceTitleEditing]
  )
  const showIdentityHeader = Boolean(branchName || workspaceTitle)

  if (
    !showIdentityHeader &&
    !hasWorktreeCardDetails({
      comment,
      cliProvenance
    }) &&
    !detailsAfter
  ) {
    return children
  }

  return (
    <HoverCard
      open={hoverOpen || workspaceTitleEditing}
      onOpenChange={handleEffectiveHoverOpenChange}
      openDelay={openDelay}
      closeDelay={closeDelay}
    >
      <HoverCardTrigger
        asChild
        onPointerEnter={hasComment(comment) ? preloadCommentMarkdown : undefined}
      >
        {children}
      </HoverCardTrigger>
      <HoverCardContent
        side="right"
        align="start"
        sideOffset={8}
        className="w-80 max-h-[28rem] overflow-y-auto p-3 text-xs scrollbar-sleek"
        {...{ [WORKTREE_NATIVE_CONTEXT_MENU_ATTR]: '' }}
        onClick={(event) => event.stopPropagation()}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        <SelectedTextCopyMenu className="space-y-3">
          {showIdentityHeader && (
            <WorktreeCardHoverIdentityHeader
              branchName={branchName}
              workspaceTitle={workspaceTitle}
              identityOrder={identityOrder}
              workspaceTitleRenameDisabled={workspaceTitleRenameDisabled}
              onRenameWorkspaceTitle={onRenameWorkspaceTitle}
              onWorkspaceTitleEditingChange={handleWorkspaceTitleEditingChange}
            />
          )}

          {cliProvenance && <WorktreeCardCliDetailSection provenance={cliProvenance} />}

          {hasComment(comment) && (
            <WorktreeCardDetailSection>
              <DetailHeader
                icon={<StickyNote className="size-3 text-muted-foreground" />}
                label={translate('auto.components.sidebar.WorktreeCardMeta.93cbea12c2', 'Notes')}
                actions={
                  onEditComment ? (
                    <MetadataActionIcon
                      label={translate(
                        'auto.components.sidebar.WorktreeCardMeta.c7fa72ead0',
                        'Edit notes'
                      )}
                      onClick={onEditComment}
                    >
                      <Pencil className="size-3" />
                    </MetadataActionIcon>
                  ) : null
                }
              />
              <WorktreeCardDetailSectionContent className="space-y-2">
                <CommentMarkdownAsync
                  content={comment ?? ''}
                  className={COMMENT_MARKDOWN_CLASS_NAME}
                  // Mirrors remark-breaks so the fallback keeps the note's line count.
                  fallbackClassName="whitespace-pre-wrap"
                />
              </WorktreeCardDetailSectionContent>
            </WorktreeCardDetailSection>
          )}

          {detailsAfter}
        </SelectedTextCopyMenu>
      </HoverCardContent>
    </HoverCard>
  )
}
