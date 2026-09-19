import type { PersistedState } from '../../../shared/persisted-state-types'
import {
  getDefaultUIState,
  normalizeAgentActivityDisplayMode,
  normalizeWorktreeCardProperties
} from '../../../shared/constants'
import { normalizeWorkspaceStatuses } from '../../../shared/workspace-statuses'
import { normalizeUsagePercentageDisplay } from '../../../shared/usage-percentage-display'
import { normalizeStatusBarUsageMode } from '../../../shared/status-bar-usage-mode'
import { clampMarkdownTocPanelWidth } from '../../../shared/markdown-toc-panel-width'
import { clampCombinedDiffFileTreeWidth } from '../../../shared/combined-diff-file-tree-width'
import {
  normalizeVisibleExecutionHostIds,
  normalizeExecutionHostOrder
} from '../../../shared/execution-host'
import { normalizeManualRepoOrder } from '../../../shared/manual-repo-order'
import { normalizeBrowserPageZoomLevel } from '../../../shared/browser-page-zoom'
import { normalizeFeatureInteractions } from '../../../shared/feature-interactions'
import { mergeWorkspaceCleanupUIState } from '../../../shared/workspace-cleanup-ui-state'
import { persistedUIValuesEqual } from '../../../shared/persisted-ui-equality'
import {
  PROTECTED_SECRET_SLOT,
  type ProtectedSecretPersistence
} from '../../protected-secret-persistence'
import {
  normalizeGroupBy,
  normalizeProjectOrderBy,
  normalizeRightSidebarExplorerView,
  normalizeRightSidebarTab,
  normalizeShowDotfilesByWorktree,
  normalizeSortBy
} from './ui-selection-normalization'
import { mergeFeatureInteractions } from './ui-interaction-merge'

export type UIUpdateOperations = {
  state: PersistedState
  removeRetainedBlob: (
    slot: Parameters<ProtectedSecretPersistence['removeRetainedBlob']>[0]
  ) => void
  setActiveView: (activeView: PersistedState['ui']['activeView'] | undefined) => boolean
  getUI: () => PersistedState['ui']
  scheduleSave: () => void
  notifyUIChanged: () => void
}

export function updatePersistedUI(
  operations: UIUpdateOperations,
  updates: Partial<PersistedState['ui']>
): void {
  if ('browserKagiSessionLink' in updates && !updates.browserKagiSessionLink) {
    operations.removeRetainedBlob(PROTECTED_SECRET_SLOT.browserKagiSessionLink)
  }
  const { activeView, ...durableUpdates } = updates
  const activeViewChanged = operations.setActiveView(activeView)
  if (Object.keys(durableUpdates).length === 0) {
    if (activeViewChanged) {
      operations.notifyUIChanged()
    }
    return
  }
  const currentUI = {
    ...getDefaultUIState(),
    ...operations.state.ui
  }
  const previousUI = {
    ...operations.getUI(),
    // Why: the legacy field stays unchanged as a migration/downgrade
    // fallback; the profile sidecar is authoritative in current builds.
    activeView: currentUI.activeView
  }
  const nextRightSidebarTab =
    updates.rightSidebarTab !== undefined
      ? normalizeRightSidebarTab(updates.rightSidebarTab)
      : normalizeRightSidebarTab(operations.state.ui?.rightSidebarTab)
  const nextRightSidebarExplorerView =
    updates.rightSidebarExplorerView !== undefined
      ? normalizeRightSidebarExplorerView(
          updates.rightSidebarExplorerView,
          nextRightSidebarTab
        )
      : updates.rightSidebarTab === 'search'
        ? 'search'
        : normalizeRightSidebarExplorerView(
            operations.state.ui?.rightSidebarExplorerView,
            nextRightSidebarTab
          )
  const nextUI = {
    ...currentUI,
    ...durableUpdates,
    workspaceCleanup: mergeWorkspaceCleanupUIState(
      currentUI.workspaceCleanup,
      durableUpdates.workspaceCleanup
    ),
    groupBy: durableUpdates.groupBy
      ? normalizeGroupBy(durableUpdates.groupBy)
      : normalizeGroupBy(operations.state.ui?.groupBy),
    sortBy: durableUpdates.sortBy
      ? normalizeSortBy(durableUpdates.sortBy)
      : normalizeSortBy(operations.state.ui?.sortBy),
    projectOrderBy: updates.projectOrderBy
      ? normalizeProjectOrderBy(updates.projectOrderBy)
      : normalizeProjectOrderBy(operations.state.ui?.projectOrderBy),
    activeView: currentUI.activeView,
    rightSidebarTab: nextRightSidebarTab,
    rightSidebarExplorerView: nextRightSidebarExplorerView,
    worktreeCardProperties:
      updates.worktreeCardProperties !== undefined
        ? normalizeWorktreeCardProperties(updates.worktreeCardProperties)
        : normalizeWorktreeCardProperties(operations.state.ui?.worktreeCardProperties),
    agentActivityDisplayMode:
      updates.agentActivityDisplayMode !== undefined
        ? normalizeAgentActivityDisplayMode(updates.agentActivityDisplayMode)
        : normalizeAgentActivityDisplayMode(operations.state.ui?.agentActivityDisplayMode),
    workspaceStatuses:
      updates.workspaceStatuses !== undefined
        ? normalizeWorkspaceStatuses(updates.workspaceStatuses)
        : normalizeWorkspaceStatuses(operations.state.ui?.workspaceStatuses),
    usagePercentageDisplay: normalizeUsagePercentageDisplay(
      updates.usagePercentageDisplay ?? operations.state.ui?.usagePercentageDisplay
    ),
    statusBarUsageMode: normalizeStatusBarUsageMode(
      updates.statusBarUsageMode ?? operations.state.ui?.statusBarUsageMode
    ),
    markdownTocPanelWidth: clampMarkdownTocPanelWidth(
      updates.markdownTocPanelWidth ?? operations.state.ui?.markdownTocPanelWidth
    ),
    combinedDiffFileTreeWidth: clampCombinedDiffFileTreeWidth(
      updates.combinedDiffFileTreeWidth ?? operations.state.ui?.combinedDiffFileTreeWidth
    ),
    visibleWorkspaceHostIds:
      updates.visibleWorkspaceHostIds !== undefined
        ? normalizeVisibleExecutionHostIds(updates.visibleWorkspaceHostIds)
        : normalizeVisibleExecutionHostIds(operations.state.ui?.visibleWorkspaceHostIds),
    workspaceHostOrder:
      updates.workspaceHostOrder !== undefined
        ? normalizeExecutionHostOrder(updates.workspaceHostOrder)
        : normalizeExecutionHostOrder(operations.state.ui?.workspaceHostOrder),
    manualRepoOrder:
      updates.manualRepoOrder !== undefined
        ? normalizeManualRepoOrder(updates.manualRepoOrder)
        : normalizeManualRepoOrder(operations.state.ui?.manualRepoOrder),
    browserDefaultZoomLevel: normalizeBrowserPageZoomLevel(
      updates.browserDefaultZoomLevel ?? operations.state.ui?.browserDefaultZoomLevel
    ),
    showDotfilesByWorktree:
      updates.showDotfilesByWorktree !== undefined
        ? normalizeShowDotfilesByWorktree(updates.showDotfilesByWorktree)
        : normalizeShowDotfilesByWorktree(operations.state.ui?.showDotfilesByWorktree),
    // Why: runtime RPCs and the renderer both record education state; merge so a stale renderer snapshot can't erase runtime-only interactions.
    featureInteractions:
      updates.featureInteractions !== undefined
        ? mergeFeatureInteractions(
            operations.state.ui?.featureInteractions,
            updates.featureInteractions
          )
        : normalizeFeatureInteractions(operations.state.ui?.featureInteractions)
  }
  if (persistedUIValuesEqual(previousUI, nextUI)) {
    if (activeViewChanged) {
      operations.notifyUIChanged()
    }
    return
  }
  operations.state.ui = nextUI
  operations.scheduleSave()
  operations.notifyUIChanged()
}
