import type { TabsSlice, TabsSliceGet, TabsSliceSet } from './tabs-slice-contract'
import { buildActiveSurfacePatch } from './tabs-surface'

/** Stable id for the (single) Git Log tab of a worktree — dedupe key. */
export function gitLogTabId(worktreeId: string): string {
  return `git-log/${worktreeId}`
}

export function createTabsGitLogActions(
  set: TabsSliceSet,
  get: TabsSliceGet
): Pick<TabsSlice, 'openGitLogTab'> {
  return {
    openGitLogTab: ({ worktreeId }) => {
      const existing = (get().unifiedTabsByWorktree[worktreeId] ?? []).find(
        (tab) => tab.contentType === 'git-log' && tab.gitLog?.worktreeId === worktreeId
      )
      if (existing) {
        // Only the active worktree may take focus (mirrors openPluginTab).
        if (get().activeWorktreeId === worktreeId) {
          get().activateTab(existing.id, { worktreeId })
          set((state) => buildActiveSurfacePatch(state, worktreeId, existing.groupId))
        }
        return existing
      }
      const id = gitLogTabId(worktreeId)
      const shouldActivate = get().activeWorktreeId === worktreeId
      const created = get().createUnifiedTab(worktreeId, 'git-log', {
        id,
        entityId: id,
        label: 'Git Log',
        gitLog: { worktreeId },
        activate: shouldActivate
      })
      if (shouldActivate) {
        set((state) => buildActiveSurfacePatch(state, worktreeId, created.groupId))
      }
      return created
    }
  }
}
