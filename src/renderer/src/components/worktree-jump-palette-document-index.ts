import { getPaletteHostBadge } from '@/components/cmd-j/palette-host-badge'
import type { SidebarHostOption } from '@/components/sidebar/sidebar-host-options'
import { buildWorktreePaletteDocuments } from '@/lib/worktree-palette-document'
import {
  getPaletteWorktreeIdentity,
  resolvePaletteRepoForWorktree
} from '@/lib/palette-repo-resolution'
import type { PaletteDocument } from '@/lib/palette-match/palette-document'
import type { AppState } from '@/store/types'
import type { Repo } from '../../../shared/repo-types'
import type { Worktree } from '../../../shared/worktree/types'
import type { HostedReviewInfo } from '../../../shared/hosted-review'

export function buildWorktreeJumpPaletteDocumentIndex({
  worktrees,
  repoMap,
  repoByHostIdentity,
  hostOptions,
  hostFilterActive,
  prCache,
  issueCache,
  checksReviewByWorktree
}: {
  worktrees: readonly Worktree[]
  repoMap: ReadonlyMap<string, Repo>
  repoByHostIdentity: ReadonlyMap<string, Repo>
  hostOptions: readonly SidebarHostOption[]
  hostFilterActive: boolean
  prCache: AppState['prCache'] | null
  issueCache: AppState['issueCache'] | null
  checksReviewByWorktree: ReadonlyMap<Worktree, HostedReviewInfo | null>
}): Map<string, PaletteDocument> {
  const hostLabelByWorktreeId = new Map<string, string>()
  for (const worktree of worktrees) {
    const repo = resolvePaletteRepoForWorktree(worktree, repoMap, repoByHostIdentity)
    const badge = getPaletteHostBadge(repo, hostOptions, hostFilterActive)
    if (badge) {
      hostLabelByWorktreeId.set(getPaletteWorktreeIdentity(worktree), badge.label)
    }
  }
  return buildWorktreePaletteDocuments(
    worktrees.filter((worktree) => !worktree.isArchived),
    {
      repoMap,
      repoMapByHostIdentity: repoByHostIdentity,
      prCache,
      issueCache,
      checksReviewByWorktree,
      hostLabelByWorktreeId
    }
  )
}
