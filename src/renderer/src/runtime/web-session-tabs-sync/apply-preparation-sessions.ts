import { buildWebSessionExistingTabIndex } from '../web-session-existing-tab-index'
import { chooseTargetGroupId, buildHostGroupIdByTabId } from './tab-group-layout-tree'
import { buildMirroredEditorTabs } from './tab-builders'
import { buildMirroredAgentTabs, isReadyEditorTab } from './terminal-surfaces'
import type { prepareWebSessionTabsSnapshotBase } from './apply-preparation-base'
import type { OpenFile } from '../../store/slices/editor'
import {
  advanceWebSessionOpenFilesIndex,
  firstOpenFileByIdForWorktree,
  sameOpenFiles,
  webSessionOpenFilesForWorktree
} from './state-equality-files'

/**
 * Reconcile the session's editor and agent tabs (and the unified-tab retention they feed)
 * off one host frame. Browser rows were removed with the embedded browser; hosts can no
 * longer publish them, so there is nothing left to mirror here.
 */
export function prepareWebSessionTabsSnapshotSessions(
  base: ReturnType<typeof prepareWebSessionTabsSnapshotBase>
) {
  const {
    state,
    snapshot,
    environmentId,
    worktreeId,
    now,
    mirroredTerminalTabEntries,
    mirroredTerminalIds,
    removedTerminalIds,
    reconcilesNonAgentTabs,
    batchContext
  } = base
  const targetGroupId = chooseTargetGroupId(state, snapshot)
  const hostGroupIdByTabId = buildHostGroupIdByTabId(snapshot.tabGroups)
  const currentUnifiedTabs = state.unifiedTabsByWorktree[worktreeId] ?? []
  const existingTabIndex = buildWebSessionExistingTabIndex({ unifiedTabs: currentUnifiedTabs })
  const readyEditorTabs = reconcilesNonAgentTabs ? snapshot.tabs.filter(isReadyEditorTab) : []
  const worktreeOpenFiles = webSessionOpenFilesForWorktree(state, worktreeId, batchContext)
  const mirroredEditorTabs = buildMirroredEditorTabs(
    snapshot,
    environmentId,
    firstOpenFileByIdForWorktree(worktreeOpenFiles),
    existingTabIndex,
    hostGroupIdByTabId,
    targetGroupId,
    mirroredTerminalTabEntries.length,
    now
  )
  const mirroredAgentTabs = buildMirroredAgentTabs(
    snapshot,
    hostGroupIdByTabId,
    targetGroupId,
    mirroredTerminalTabEntries.length + mirroredEditorTabs.length,
    currentUnifiedTabs,
    now
  )
  const mirroredEditorFileIds = new Set(mirroredEditorTabs.map((entry) => entry.file.id))
  const mirroredEditorHostTabIds = new Set(mirroredEditorTabs.map((entry) => entry.hostTabId))
  const removedEditorFileIds = new Set(
    (reconcilesNonAgentTabs ? worktreeOpenFiles : [])
      .filter(
        (file) =>
          file.runtimeEnvironmentId === environmentId &&
          (file.mode === 'edit' || file.mode === 'markdown-preview') &&
          // Why: only cull host-mirrored tabs; locally opened files have no host counterpart, so their omission isn't a close signal.
          file.mirroredFromRuntimeSession === true &&
          !mirroredEditorFileIds.has(file.id)
      )
      .map((file) => file.id)
  )
  const isReplacedOpenFile = (file: OpenFile): boolean =>
    file.runtimeEnvironmentId === environmentId &&
    (removedEditorFileIds.has(file.id) || mirroredEditorFileIds.has(file.id))
  const replacedOpenFileCount = worktreeOpenFiles.filter(isReplacedOpenFile).length
  // Why: both consumers below ask only about this worktree, so the surviving ids answer
  // them in worktree scope instead of walking every open file in the app.
  const nextWorktreeOpenFileIds = new Set<string>(
    worktreeOpenFiles.filter((file) => !isReplacedOpenFile(file)).map((file) => file.id)
  )
  for (const fileId of mirroredEditorFileIds) {
    nextWorktreeOpenFileIds.add(fileId)
  }
  const mirroredOpenFiles = mirroredEditorTabs.map((entry) => entry.file)
  const nextOpenFiles = (() => {
    // Why: with nothing to drop or mirror, rebuilding reproduces the array exactly, so
    // skip the global rebuild the equality check below would have thrown away anyway.
    if (replacedOpenFileCount === 0 && mirroredOpenFiles.length === 0) {
      return state.openFiles
    }
    const retained = state.openFiles.filter(
      (file) =>
        !(
          file.worktreeId === worktreeId &&
          file.runtimeEnvironmentId === environmentId &&
          (removedEditorFileIds.has(file.id) || mirroredEditorFileIds.has(file.id))
        )
    )
    const next = [...retained, ...mirroredOpenFiles]
    return sameOpenFiles(state.openFiles, next) ? state.openFiles : next
  })()
  advanceWebSessionOpenFilesIndex(batchContext, nextOpenFiles, worktreeId)
  const retainedUnifiedTabs = currentUnifiedTabs.filter((tab) => {
    if (tab.contentType === 'agent-session') {
      return false
    }
    if (tab.contentType === 'editor') {
      return (
        !removedEditorFileIds.has(tab.entityId) &&
        !mirroredEditorFileIds.has(tab.entityId) &&
        !mirroredEditorHostTabIds.has(tab.id)
      )
    }
    if (tab.contentType !== 'terminal') {
      return true
    }
    if (removedTerminalIds.has(tab.entityId) || removedTerminalIds.has(tab.id)) {
      return false
    }
    return !mirroredTerminalIds.has(tab.entityId) && !mirroredTerminalIds.has(tab.id)
  })
  const existingViewModeByTabId = new Map(
    currentUnifiedTabs
      .filter((tab) => tab.contentType === 'terminal' && tab.viewMode)
      .map((tab) => [tab.id, tab.viewMode] as const)
  )
  return {
    ...base,
    targetGroupId,
    hostGroupIdByTabId,
    currentUnifiedTabs,
    existingTabIndex,
    readyEditorTabs,
    worktreeOpenFiles,
    mirroredEditorTabs,
    mirroredAgentTabs,
    mirroredEditorFileIds,
    mirroredEditorHostTabIds,
    removedEditorFileIds,
    nextWorktreeOpenFileIds,
    nextOpenFiles,
    retainedUnifiedTabs,
    existingViewModeByTabId
  }
}
