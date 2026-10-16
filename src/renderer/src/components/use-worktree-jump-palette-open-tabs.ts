import { useMemo } from 'react'
import {
  buildSearchableWorkspaceTabs,
  searchWorkspaceTabs,
  type SearchableWorkspaceTab
} from '@/lib/workspace-tab-palette-search'
import { comparePaletteRankedItems } from '@/lib/cmd-j-section-leadership'
import { getPaletteWorktreeIdentity } from '@/lib/palette-repo-resolution'
import type {
  OpenTabPaletteItem,
  WorkspaceTabPaletteItem,
  WorktreePaletteItem
} from './worktree-jump-palette-model'
import type { WorktreeJumpPaletteFilter } from './use-worktree-jump-palette-filter'
import type { WorktreeJumpPaletteLocalState } from './use-worktree-jump-palette-local-state'
import type { WorktreeJumpPaletteStoreState } from './use-worktree-jump-palette-store-state'
import type { WorktreeJumpPaletteWorktrees } from './use-worktree-jump-palette-worktrees'
import {
  encodePaletteIdentity,
  type PaletteSearchContext
} from '@/lib/palette-match/palette-ranking'
import {
  buildOpenTabPaletteItems,
  buildWorkspaceTabPaletteItems
} from './worktree-jump-palette-open-tab-items'

const EMPTY_WORKSPACE_TAB_ENTRIES: SearchableWorkspaceTab[] = []

type WorktreeJumpPaletteOpenTabsInput = WorktreeJumpPaletteStoreState &
  WorktreeJumpPaletteWorktrees &
  Pick<WorktreeJumpPaletteFilter, 'repoMap' | 'repoByHostIdentity'> &
  Pick<WorktreeJumpPaletteLocalState, 'deferredQuery'> & {
    paletteSearchContext: PaletteSearchContext
  }

export function useWorktreeJumpPaletteOpenTabs({
  paletteStatusInputsActive,
  browserSortedWorktrees,
  allWorktrees,
  repoMap,
  repoByHostIdentity,
  worktreeOrder,
  activeWorktreeId,
  activeWorkspaceExecutionHostId,
  activeTabType,
  unifiedTabsByWorktree,
  activeGroupIdByWorktree,
  groupsByWorktree,
  tabsByWorktree,
  openFiles,
  agentStatusByPaneKey,
  retainedAgentsByPaneKey,
  sleepingAgentSessionsByPaneKey,
  activeTabId,
  activeTabIdByWorktree,
  activeFileId,
  activeFileIdByWorktree,
  activeTabTypeByWorktree,
  settings,
  terminalLayoutsByTabId,
  paneForegroundAgentByPaneKey,
  deferredQuery,
  paletteSearchContext,
  hasQuery,
  worktreeMatches,
  resolveWorktree
}: WorktreeJumpPaletteOpenTabsInput) {
  const workspaceTabEntries = useMemo<SearchableWorkspaceTab[]>(() => {
    if (!paletteStatusInputsActive) {
      return EMPTY_WORKSPACE_TAB_ENTRIES
    }
    return buildSearchableWorkspaceTabs({
      worktrees: browserSortedWorktrees,
      ownershipWorktrees: allWorktrees,
      repoMap,
      repoMapByHostIdentity: repoByHostIdentity,
      worktreeOrder,
      unifiedTabsByWorktree,
      tabsByWorktree,
      openFiles,
      agentStatusByPaneKey,
      retainedAgentsByPaneKey,
      sleepingAgentSessionsByPaneKey,
      activeGroupIdByWorktree,
      groupsByWorktree,
      activeWorktreeId,
      activeWorkspaceExecutionHostId,
      activeTabType,
      activeTabId,
      activeTabIdByWorktree,
      activeFileId,
      activeFileIdByWorktree,
      activeTabTypeByWorktree,
      generatedTitlesEnabled: settings?.tabAutoGenerateTitle === true,
      terminalLayoutsByTabId,
      paneForegroundAgentByPaneKey
    })
  }, [
    paletteStatusInputsActive,
    activeFileId,
    activeFileIdByWorktree,
    activeGroupIdByWorktree,
    activeTabId,
    activeTabIdByWorktree,
    activeTabType,
    activeTabTypeByWorktree,
    activeWorktreeId,
    activeWorkspaceExecutionHostId,
    allWorktrees,
    agentStatusByPaneKey,
    browserSortedWorktrees,
    groupsByWorktree,
    openFiles,
    repoMap,
    repoByHostIdentity,
    retainedAgentsByPaneKey,
    settings?.tabAutoGenerateTitle,
    sleepingAgentSessionsByPaneKey,
    paneForegroundAgentByPaneKey,
    tabsByWorktree,
    terminalLayoutsByTabId,
    unifiedTabsByWorktree,
    worktreeOrder
  ])
  const workspaceTabMatches = useMemo(
    () =>
      searchWorkspaceTabs(workspaceTabEntries, deferredQuery.trim(), {
        context: paletteSearchContext
      }),
    [workspaceTabEntries, deferredQuery, paletteSearchContext]
  )
  const worktreeItems = useMemo<WorktreePaletteItem[]>(() => {
    const items = worktreeMatches
      .map((match) => {
        const worktree = resolveWorktree(match.worktreeId, match.worktreeHostId)
        return worktree
          ? {
              id: encodePaletteIdentity(['worktree', getPaletteWorktreeIdentity(worktree)]),
              type: 'worktree' as const,
              match,
              worktree
            }
          : null
      })
      .filter((item): item is WorktreePaletteItem => item !== null)
    if (!hasQuery) {
      return items
    }
    const orderByIdentity = new Map(
      items.map((item, index) => [getPaletteWorktreeIdentity(item.worktree), index])
    )
    return items.sort((left, right) =>
      comparePaletteRankedItems(
        {
          rank: left.match.rank,
          order: orderByIdentity.get(getPaletteWorktreeIdentity(left.worktree)) ?? 0,
          identity: left.id,
          activity: left.match.activity
        },
        {
          rank: right.match.rank,
          order: orderByIdentity.get(getPaletteWorktreeIdentity(right.worktree)) ?? 0,
          identity: right.id,
          activity: right.match.activity
        }
      )
    )
  }, [hasQuery, resolveWorktree, worktreeMatches])
  const workspaceTabItems = useMemo<WorkspaceTabPaletteItem[]>(
    () => buildWorkspaceTabPaletteItems(workspaceTabMatches),
    [workspaceTabMatches]
  )
  const openTabItems = useMemo<OpenTabPaletteItem[]>(
    () => buildOpenTabPaletteItems({ workspaceTabItems }),
    [workspaceTabItems]
  )

  return {
    workspaceTabEntries,
    worktreeItems,
    workspaceTabItems,
    openTabItems
  }
}

export type WorktreeJumpPaletteOpenTabs = ReturnType<typeof useWorktreeJumpPaletteOpenTabs>
