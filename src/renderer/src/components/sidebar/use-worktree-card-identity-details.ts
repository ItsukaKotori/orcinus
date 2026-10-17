import { getWorktreeGitIdentityDisplay } from '@/lib/worktree-git-identity-display'
import { isFolderRepo } from '../../../../shared/repo-kind'
import { parseWorkspaceKey } from '../../../../shared/workspace-scope'
import type { WorktreeCardProps } from './worktree-card-model'
import type { useWorktreeCardFoundation } from './use-worktree-card-foundation'

type Foundation = ReturnType<typeof useWorktreeCardFoundation>

export function useWorktreeCardIdentityDetails({
  worktree,
  repo,
  projectGroups,
  cardProps,
  newCardStyle
}: Pick<WorktreeCardProps, 'worktree' | 'repo'> &
  Pick<Foundation, 'projectGroups' | 'cardProps' | 'newCardStyle'>) {
  const gitIdentityDisplay = getWorktreeGitIdentityDisplay(worktree)
  const detachedHeadDisplay = gitIdentityDisplay?.kind === 'detached' ? gitIdentityDisplay : null
  const branch = gitIdentityDisplay?.kind === 'branch' ? gitIdentityDisplay.branchName : ''
  const workspaceScope = parseWorkspaceKey(worktree.id)
  const folderWorkspaceId =
    workspaceScope?.type === 'folder' ? workspaceScope.folderWorkspaceId : null
  const isFolder = repo ? isFolderRepo(repo) : folderWorkspaceId !== null
  // Why: project groups gate folder workspaces, so folder paths stay hidden from identity surfaces until that capability exists.
  const hasProjectGroups = projectGroups.length > 0
  const branchIdentityDisplay = !isFolder && branch.length > 0 ? branch : undefined
  const folderPathIdentityDisplay =
    isFolder && hasProjectGroups && worktree.path.trim().length > 0 ? worktree.path : undefined
  const identityDisplay = branchIdentityDisplay ?? folderPathIdentityDisplay
  const hasPathIdentityEnabled = cardProps.includes('branch')
  const showIdentityInNewCard = newCardStyle && hasPathIdentityEnabled && Boolean(identityDisplay)
  const folderMetaRowContent = newCardStyle
    ? hasPathIdentityEnabled && Boolean(folderPathIdentityDisplay)
    : isFolder

  return {
    detachedHeadDisplay,
    branch,
    folderWorkspaceId,
    isFolder,
    branchIdentityDisplay,
    folderPathIdentityDisplay,
    identityDisplay,
    showIdentityInNewCard,
    folderMetaRowContent
  }
}
