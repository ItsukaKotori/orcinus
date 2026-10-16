import type { WebSessionTabsSyncState } from './state'
import type { applyActiveStateUpdates } from './apply-active-state'
import { buildMirroredAgentStatusPatch } from './agent-status-patch'
import {
  buildRemirroredClosedTabMarkerLiftPatch,
  buildRetractedMirroredTabSweepPatch
} from './agent-status-primitives'
import { isWebSessionTabsWorktreeRemovalFrame } from './session-tabs-inventory-absence'

type FinalPatchContext = ReturnType<typeof applyActiveStateUpdates>

/** Build the minimal store patch after all reconciliation stages have completed. */
export function buildWebSessionTabsFinalPatch(
  context: FinalPatchContext
): WebSessionTabsSyncState | Partial<WebSessionTabsSyncState> {
  const {
    state,
    snapshot,
    worktreeId,
    now,
    batchContext,
    currentTerminalTabs,
    terminalSurfaceTabs,
    mirroredTerminalTabs,
    nextTabsByWorktree,
    removedTerminalResourceIds,
    mirroredTerminalIds,
    nextOpenFiles,
    nextUnifiedTabsByWorktree,
    nextGroupsByWorktree,
    nextActiveGroupIdByWorktree,
    nextLayoutByWorktree,
    nextTabBarOrderByWorktree,
    nextPtyIdsByTabId,
    nextTerminalLayoutsByTabId,
    nextUnreadTerminalTabs,
    pendingStartupByTabId,
    nextPendingStartupByTabId,
    automaticAgentResumeClaimsByTabId,
    nextAutomaticAgentResumeClaimsByTabId,
    nextActiveTabIdByWorktree,
    nextActiveFileIdByWorktree,
    nextActiveTabId,
    nextActiveFileId,
    nextActiveTabType,
    nextActiveTabTypeByWorktree
  } = context

  const agentStatusPatch = buildMirroredAgentStatusPatch(
    state,
    currentTerminalTabs,
    terminalSurfaceTabs,
    mirroredTerminalTabs,
    now,
    batchContext
  )
  // A tombstone clears all environments' view of a worktree; it is not a terminal retraction.
  const retractedTabSweepPatch = isWebSessionTabsWorktreeRemovalFrame(snapshot)
    ? null
    : buildRetractedMirroredTabSweepPatch(
        state,
        worktreeId,
        nextTabsByWorktree,
        agentStatusPatch,
        removedTerminalResourceIds,
        batchContext
      )
  const remirroredClosedTabLiftPatch = buildRemirroredClosedTabMarkerLiftPatch(
    retractedTabSweepPatch?.recentlyClosedAgentStatusTabIds ??
      state.recentlyClosedAgentStatusTabIds,
    mirroredTerminalIds
  )

  const patch: Partial<WebSessionTabsSyncState> = {
    ...agentStatusPatch,
    ...retractedTabSweepPatch,
    ...remirroredClosedTabLiftPatch,
    ...(nextOpenFiles !== state.openFiles ? { openFiles: nextOpenFiles } : {}),
    ...(nextTabsByWorktree !== state.tabsByWorktree ? { tabsByWorktree: nextTabsByWorktree } : {}),
    ...(nextUnifiedTabsByWorktree !== state.unifiedTabsByWorktree
      ? { unifiedTabsByWorktree: nextUnifiedTabsByWorktree }
      : {}),
    ...(nextGroupsByWorktree !== state.groupsByWorktree
      ? { groupsByWorktree: nextGroupsByWorktree }
      : {}),
    ...(nextActiveGroupIdByWorktree !== state.activeGroupIdByWorktree
      ? { activeGroupIdByWorktree: nextActiveGroupIdByWorktree }
      : {}),
    ...(nextLayoutByWorktree !== state.layoutByWorktree
      ? { layoutByWorktree: nextLayoutByWorktree }
      : {}),
    ...(nextTabBarOrderByWorktree !== state.tabBarOrderByWorktree
      ? { tabBarOrderByWorktree: nextTabBarOrderByWorktree }
      : {}),
    ...(nextPtyIdsByTabId !== state.ptyIdsByTabId ? { ptyIdsByTabId: nextPtyIdsByTabId } : {}),
    ...(nextTerminalLayoutsByTabId !== state.terminalLayoutsByTabId
      ? { terminalLayoutsByTabId: nextTerminalLayoutsByTabId }
      : {}),
    ...(nextUnreadTerminalTabs !== state.unreadTerminalTabs
      ? { unreadTerminalTabs: nextUnreadTerminalTabs }
      : {}),
    ...(nextPendingStartupByTabId !== pendingStartupByTabId
      ? { pendingStartupByTabId: nextPendingStartupByTabId }
      : {}),
    ...(nextAutomaticAgentResumeClaimsByTabId !== automaticAgentResumeClaimsByTabId
      ? { automaticAgentResumeClaimsByTabId: nextAutomaticAgentResumeClaimsByTabId }
      : {}),
    ...(nextActiveTabIdByWorktree !== state.activeTabIdByWorktree
      ? { activeTabIdByWorktree: nextActiveTabIdByWorktree }
      : {}),
    ...(nextActiveFileIdByWorktree !== state.activeFileIdByWorktree
      ? { activeFileIdByWorktree: nextActiveFileIdByWorktree }
      : {}),
    ...(nextActiveTabId !== state.activeTabId ? { activeTabId: nextActiveTabId } : {}),
    ...(nextActiveFileId !== state.activeFileId ? { activeFileId: nextActiveFileId } : {}),
    ...(nextActiveTabType !== state.activeTabType ? { activeTabType: nextActiveTabType } : {}),
    ...(nextActiveTabTypeByWorktree !== state.activeTabTypeByWorktree
      ? { activeTabTypeByWorktree: nextActiveTabTypeByWorktree }
      : {})
  }
  return Object.keys(patch).length === 0 ? state : patch
}
