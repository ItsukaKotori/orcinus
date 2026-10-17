import type {
  AutomationWorkspaceProvenance,
  CliWorkspaceProvenance
} from '../../../../shared/worktree/types'
import type { WorktreeCardDetailsHoverControl } from './worktree-card-details-hover-state'

export type WorktreeCardMetaBadgesProps = {
  comment: string | null
  automationProvenance?: AutomationWorkspaceProvenance | null
  cliProvenance?: CliWorkspaceProvenance | null
}

export type WorktreeCardMetaBadgesRootProps = WorktreeCardMetaBadgesProps &
  React.HTMLAttributes<HTMLDivElement>

export type WorktreeCardDetailsHoverProps = WorktreeCardMetaBadgesProps & {
  children: React.ReactElement
  branchName?: string
  workspaceTitle?: string
  identityOrder?: 'workspace-first' | 'branch-first'
  workspaceTitleRenameDisabled?: boolean
  detailsAfter?: React.ReactNode
  openDelay?: number
  closeDelay?: number
  onRenameWorkspaceTitle?: (displayName: string) => Promise<void> | void
  onWorkspaceTitleEditingChange?: (editing: boolean) => void
  onEditComment?: (event: React.MouseEvent) => void
  onOpenAutomation?: (event: React.MouseEvent) => void
  onOpenAutomationRun?: (event: React.MouseEvent) => void
  hoverControl?: WorktreeCardDetailsHoverControl
}
