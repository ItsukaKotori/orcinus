import type { Repo } from '../../../../../../shared/repo-types'
import type { ProjectOrderBy } from '../../../../../../shared/ui-chrome-types'
import type { WorkspaceStatusDefinition, Worktree } from '../../../../../../shared/worktree/types'
import {
  getWorkspaceStatus,
  getWorkspaceStatusFromGroupKey,
  getWorkspaceStatusGroupKey
} from '../../workspace-status'
import {
  compareFolderWorkspacesForDisplay,
  getFolderWorkspaceLaneKey,
  type RenderableFolderWorkspace
} from './folder-workspace-lanes'
import { addRepoIdToGroup, getProjectGroupingForRepo } from './project-grouping'
import type {
  OrderedGroupEntry,
  ProjectGroupingIndex,
  WorktreeGroupEntry
} from './project-grouping'
import type {
  ImportedWorktreesCardCandidate,
  NewExternalWorktreesInboxCandidate,
  PendingCreationRef,
  WorktreeGroupBy
} from './row-types'
import { getManualOrderAnchorRepo, sortProjectEntries } from './section-order'

/** Lane label for a lane a folder workspace opened before any worktree did. */
function getLaneLabelForKey(
  key: string,
  groupBy: Exclude<WorktreeGroupBy, 'repo'>,
  workspaceStatuses: readonly WorkspaceStatusDefinition[]
): string {
  if (groupBy === 'workspace-status') {
    const status = getWorkspaceStatusFromGroupKey(key, workspaceStatuses)
    return workspaceStatuses.find((entry) => entry.id === status)?.label ?? status ?? key
  }
  return key
}

/** Buckets worktrees into their sections and returns them in render order. */
export function buildOrderedGroups(args: {
  groupBy: WorktreeGroupBy
  naturalWorktrees: readonly Worktree[]
  repoMap: Map<string, Repo>
  workspaceStatuses: readonly WorkspaceStatusDefinition[]
  projectIndex: ProjectGroupingIndex | null
  placeholderRepoIds: ReadonlySet<string>
  importedWorktreesByRepo: ReadonlyMap<string, ImportedWorktreesCardCandidate>
  newExternalWorktreesInboxByRepo: ReadonlyMap<string, NewExternalWorktreesInboxCandidate>
  pendingByRepo: ReadonlyMap<string, PendingCreationRef[]>
  repoOrder: Map<string, number> | undefined
  projectOrderBy: ProjectOrderBy
  folderWorkspaces?: readonly RenderableFolderWorkspace[]
}): OrderedGroupEntry[] {
  const {
    groupBy,
    naturalWorktrees,
    repoMap,
    workspaceStatuses,
    projectIndex,
    placeholderRepoIds,
    importedWorktreesByRepo,
    newExternalWorktreesInboxByRepo,
    pendingByRepo,
    repoOrder,
    projectOrderBy,
    folderWorkspaces = []
  } = args

  const grouped = new Map<string, WorktreeGroupEntry>()
  for (const w of naturalWorktrees) {
    let repo: Repo | undefined
    let key: string
    let label: string
    if (groupBy === 'repo') {
      const grouping = getProjectGroupingForRepo(w.repoId, repoMap, projectIndex)
      key = grouping.key
      label = grouping.label
      repo = grouping.repo
    } else {
      // Why: 'none' returns before grouping runs, so the remaining mode here is
      // workspace-status; folder lanes arrive via the dedicated loop below.
      const workspaceStatus = getWorkspaceStatus(w, workspaceStatuses)
      key = getWorkspaceStatusGroupKey(workspaceStatus)
      label =
        workspaceStatuses.find((status) => status.id === workspaceStatus)?.label ?? workspaceStatus
    }
    if (!grouped.has(key)) {
      grouped.set(key, { label, items: [], repo, repoIds: new Set() })
    }
    const group = grouped.get(key)!
    group.items.push(w)
    addRepoIdToGroup(group, w.repoId)
  }
  // Why: folder workspaces are not worktrees, so they never appear in the loop
  // above. Bucketing them here — and creating the lane when no worktree opened
  // one — is what lets a folder workspace be the sole occupant of a lane (#15362).
  if (groupBy !== 'repo') {
    for (const pair of folderWorkspaces) {
      const key = getFolderWorkspaceLaneKey(pair, groupBy, workspaceStatuses)
      if (!grouped.has(key)) {
        grouped.set(key, {
          label: getLaneLabelForKey(key, groupBy, workspaceStatuses),
          items: [],
          repo: undefined,
          repoIds: new Set()
        })
      }
      const group = grouped.get(key)!
      group.folderWorkspaces ??= []
      group.folderWorkspaces.push(pair)
    }
    for (const group of grouped.values()) {
      group.folderWorkspaces?.sort((left, right) =>
        compareFolderWorkspacesForDisplay(left.folderWorkspace, right.folderWorkspace)
      )
    }
  }
  if (groupBy === 'repo') {
    for (const repoId of placeholderRepoIds) {
      const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex)
      if (!grouping.repo) {
        continue
      }
      const key = grouping.key
      if (!grouped.has(key)) {
        // Why: repos can arrive before worktree scans, but stale IDs passed by
        // older snapshots must not render an "Unknown" project header.
        grouped.set(key, {
          label: grouping.label,
          items: [],
          repo: grouping.repo,
          repoIds: new Set([repoId])
        })
      } else {
        addRepoIdToGroup(grouped.get(key)!, repoId)
      }
    }
  }
  if (groupBy === 'repo') {
    for (const [repoId, candidate] of importedWorktreesByRepo) {
      const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex)
      const key = grouping.key
      if (!grouped.has(key)) {
        grouped.set(key, {
          label: grouping.label,
          items: [],
          repo: grouping.repo ?? candidate.repo,
          repoIds: new Set([repoId])
        })
      } else if (grouped.has(key)) {
        addRepoIdToGroup(grouped.get(key)!, repoId)
      }
    }
  }
  if (groupBy === 'repo') {
    for (const [repoId, candidate] of newExternalWorktreesInboxByRepo) {
      const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex)
      const key = grouping.key
      if (!grouped.has(key)) {
        // Why: the default policy removes pinned worktrees from natural groups,
        // but actionable inbox rows still need a project section to render in.
        grouped.set(key, {
          label: grouping.label,
          items: [],
          repo: grouping.repo ?? candidate.repo,
          repoIds: new Set([repoId])
        })
      } else if (grouped.has(key)) {
        addRepoIdToGroup(grouped.get(key)!, repoId)
      }
    }
  }
  if (groupBy === 'repo') {
    for (const repoId of pendingByRepo.keys()) {
      const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex)
      const key = grouping.key
      if (!grouped.has(key)) {
        // Why: creating the first worktree in a repo leaves it with no group yet;
        // ensure one so the in-progress row nests under its repo instead of being
        // dropped.
        grouped.set(key, {
          label: grouping.label,
          items: [],
          repo: grouping.repo,
          repoIds: new Set([repoId])
        })
      } else {
        addRepoIdToGroup(grouped.get(key)!, repoId)
      }
    }
  }

  const orderedGroups: OrderedGroupEntry[] = []
  if (groupBy === 'workspace-status') {
    // Why: status grouping is opt-in while the board drawer remains the wider
    // all-lanes drag target; keep the sidebar compact by omitting empty lanes.
    for (const status of workspaceStatuses) {
      const key = getWorkspaceStatusGroupKey(status.id)
      const group = grouped.get(key)
      if (group) {
        orderedGroups.push([key, group])
      }
    }
  } else {
    for (const group of grouped.values()) {
      // Why: logical project headers can contain multiple host setup repos.
      // Use the repo that anchors manual order so drag and actions target the
      // same persisted order source the row sorter reads.
      group.repo = getManualOrderAnchorRepo(group, repoMap, repoOrder)
    }
    // Why: project header order is its own user choice (projectOrderBy),
    // decoupled from workspace sortBy. Manual uses the canonical repoOrder so
    // header drag has a stable source of truth; Recent follows activity.
    const entries = sortProjectEntries(Array.from(grouped.entries()), projectOrderBy, repoOrder)
    // Why: large imported repo sets can have one group per repo; spreading
    // those entries into push can exceed V8's argument limit.
    for (const entry of entries) {
      orderedGroups.push(entry)
    }
  }
  return orderedGroups
}
