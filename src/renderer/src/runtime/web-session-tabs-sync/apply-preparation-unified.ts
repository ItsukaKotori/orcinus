import { buildTerminalUnifiedTab } from './tab-builders'
import { isAgentSessionTab } from './terminal-surfaces'
import { toWebTerminalSurfaceTabId } from '../web-runtime-session'
import { resolveWebSessionVisibleTabId } from '../web-session-focus-intent'
import { buildHostToLocalTabIdMap, updateHostSessionTabIdMappings } from './layout-groups'
import type { prepareWebSessionTabsSnapshotSessions } from './apply-preparation-sessions'

export function prepareWebSessionTabsSnapshotUnified(
  base: ReturnType<typeof prepareWebSessionTabsSnapshotSessions>
) {
  const {
    state,
    snapshot,
    environmentId,
    worktreeId,
    navigationIntentTab,
    honorSnapshotActiveFocus,
    terminalSurfaceTabs,
    mirroredTerminalTabs,
    mirroredTerminalTabEntries,
    nextTerminalTabs,
    mirroredEditorTabs,
    mirroredAgentTabs,
    readyEditorTabs,
    nextWorktreeOpenFileIds,
    retainedUnifiedTabs,
    existingViewModeByTabId,
    hostGroupIdByTabId,
    targetGroupId
  } = base
  const mirroredTerminalUnifiedTabs = mirroredTerminalTabs.map((entry) =>
    buildTerminalUnifiedTab(
      entry.tab,
      hostGroupIdByTabId.get(entry.hostTabId) ?? targetGroupId,
      environmentId,
      entry.tab.viewMode ?? existingViewModeByTabId.get(entry.tab.id)
    )
  )
  const mirroredEditorUnifiedTabs = mirroredEditorTabs.map((entry) => entry.unifiedTab)
  const mirroredAgentUnifiedTabs = mirroredAgentTabs.map((entry) => entry.unifiedTab)
  const mirroredUnifiedTabs = [
    ...mirroredTerminalUnifiedTabs,
    ...mirroredEditorUnifiedTabs,
    ...mirroredAgentUnifiedTabs
  ]
  const nextUnifiedTabs =
    retainedUnifiedTabs.length + mirroredUnifiedTabs.length > 0
      ? [...retainedUnifiedTabs, ...mirroredUnifiedTabs]
      : null
  const validUnifiedTabIds = new Set(nextUnifiedTabs?.map((tab) => tab.id) ?? [])
  const activeHostTerminalId =
    terminalSurfaceTabs.find((tab) => tab.id === snapshot.activeTabId)?.id ??
    terminalSurfaceTabs.find((tab) => tab.isActive)?.id ??
    null
  const activeHostTerminalParentId =
    terminalSurfaceTabs.find((tab) => tab.id === activeHostTerminalId)?.parentTabId ??
    terminalSurfaceTabs.find((tab) => tab.isActive)?.parentTabId ??
    null
  const activeMirroredTerminalId = activeHostTerminalId
    ? toWebTerminalSurfaceTabId(activeHostTerminalParentId ?? activeHostTerminalId)
    : null
  const activeHostEditor =
    readyEditorTabs.find((tab) => tab.id === snapshot.activeTabId) ??
    readyEditorTabs.find((tab) => tab.isActive) ??
    null
  const activeMirroredEditor = activeHostEditor
    ? (mirroredEditorTabs.find((entry) => entry.hostTabId === activeHostEditor.id) ?? null)
    : null
  const activeMirroredEditorFileId = activeMirroredEditor?.file.id ?? null
  const activeMirroredEditorTabId = activeMirroredEditor?.unifiedTab.id ?? null
  const activeHostAgent =
    snapshot.tabs
      .filter(isAgentSessionTab)
      .find((tab) => tab.id === snapshot.activeTabId || tab.isActive) ?? null
  const activeMirroredAgentTabId = activeHostAgent
    ? (mirroredAgentTabs.find((entry) => entry.hostTabId === activeHostAgent.id)?.unifiedTab.id ??
      null)
    : null
  const intentMirroredTerminalId =
    navigationIntentTab?.type === 'terminal'
      ? toWebTerminalSurfaceTabId(navigationIntentTab.parentTabId)
      : null
  const intentMirroredEditor =
    navigationIntentTab?.type === 'markdown' || navigationIntentTab?.type === 'file'
      ? (mirroredEditorTabs.find((entry) => entry.hostTabId === navigationIntentTab.id) ?? null)
      : null
  const intentMirroredAgent =
    navigationIntentTab?.type === 'agent-session'
      ? (mirroredAgentTabs.find((entry) => entry.hostTabId === navigationIntentTab.id) ?? null)
      : null
  const currentActiveTerminalStillExists =
    state.activeTabIdByWorktree[worktreeId] &&
    (nextTerminalTabs ?? []).some((tab) => tab.id === state.activeTabIdByWorktree[worktreeId])
      ? state.activeTabIdByWorktree[worktreeId]
      : null
  // Why: caller intent targets the requested tab even when an older host leaves its own active tab unchanged.
  const intentTerminalId =
    honorSnapshotActiveFocus && navigationIntentTab?.type === 'terminal'
      ? intentMirroredTerminalId
      : null
  const nextActiveTerminalId =
    intentTerminalId ??
    currentActiveTerminalStillExists ??
    (snapshot.activeTabType === 'terminal'
      ? (activeMirroredTerminalId ?? mirroredTerminalTabEntries[0]?.id)
      : mirroredTerminalTabEntries[0]?.id) ??
    null
  const activeEditorFileIdForWorktree = state.activeFileIdByWorktree[worktreeId]
  const currentActiveEditorStillExists =
    activeEditorFileIdForWorktree && nextWorktreeOpenFileIds.has(activeEditorFileIdForWorktree)
      ? activeEditorFileIdForWorktree
      : null
  const intentEditorFileId = honorSnapshotActiveFocus
    ? (intentMirroredEditor?.file.id ?? null)
    : null
  const nextActiveEditorFileId =
    intentEditorFileId ??
    currentActiveEditorStillExists ??
    (snapshot.activeTabType === 'markdown' || snapshot.activeTabType === 'file'
      ? (activeMirroredEditorFileId ?? mirroredEditorTabs[0]?.file.id)
      : mirroredEditorTabs[0]?.file.id) ??
    null
  const currentVisibleUnifiedTabId = resolveWebSessionVisibleTabId(
    state,
    worktreeId,
    nextUnifiedTabs ?? []
  )
  const currentVisibleStructuredTabId =
    currentVisibleUnifiedTabId &&
    nextUnifiedTabs?.find(
      (tab) => tab.id === currentVisibleUnifiedTabId && tab.contentType === 'agent-session'
    )
      ? currentVisibleUnifiedTabId
      : null
  // Why: a client-initiated activation also drives the visible unified tab, overriding the sticky current-visible tab.
  const intentUnifiedTabId = honorSnapshotActiveFocus
    ? navigationIntentTab?.type === 'terminal'
      ? intentTerminalId
      : navigationIntentTab?.type === 'agent-session'
        ? (intentMirroredAgent?.unifiedTab.id ?? null)
        : navigationIntentTab?.type === 'markdown' || navigationIntentTab?.type === 'file'
          ? (intentMirroredEditor?.unifiedTab.id ?? null)
          : null
    : null
  const nextActiveUnifiedTabId =
    intentUnifiedTabId ??
    currentVisibleUnifiedTabId ??
    (snapshot.activeTabType === 'agent-session'
      ? (activeMirroredAgentTabId ?? mirroredAgentUnifiedTabs[0]?.id ?? nextActiveTerminalId)
      : snapshot.activeTabType === 'markdown' || snapshot.activeTabType === 'file'
        ? (activeMirroredEditorTabId ??
          mirroredEditorTabs[0]?.unifiedTab.id ??
          state.activeTabIdByWorktree[worktreeId] ??
          nextActiveTerminalId)
        : nextActiveTerminalId)
  const mirroredUnifiedIds = new Set(mirroredUnifiedTabs.map((tab) => tab.id))
  const hostToLocalTabId = buildHostToLocalTabIdMap({
    terminalSurfaces: terminalSurfaceTabs,
    terminalTabs: mirroredTerminalTabEntries,
    editorTabs: mirroredEditorTabs,
    agentTabs: mirroredAgentTabs
  })
  updateHostSessionTabIdMappings({
    environmentId,
    worktreeId,
    terminalSurfaces: terminalSurfaceTabs,
    terminalTabs: mirroredTerminalTabEntries,
    editorTabs: mirroredEditorTabs,
    agentTabs: mirroredAgentTabs
  })

  return {
    ...base,
    mirroredTerminalUnifiedTabs,
    mirroredEditorUnifiedTabs,
    mirroredAgentUnifiedTabs,
    mirroredUnifiedTabs,
    nextUnifiedTabs,
    validUnifiedTabIds,
    activeHostTerminalId,
    activeMirroredTerminalId,
    activeHostEditor,
    activeMirroredEditor,
    activeMirroredEditorFileId,
    activeMirroredEditorTabId,
    activeHostAgent,
    activeMirroredAgentTabId,
    intentMirroredTerminalId,
    intentMirroredEditor,
    intentMirroredAgent,
    currentActiveTerminalStillExists,
    intentTerminalId,
    nextActiveTerminalId,
    currentActiveEditorStillExists,
    intentEditorFileId,
    nextActiveEditorFileId,
    currentVisibleUnifiedTabId,
    currentVisibleStructuredTabId,
    intentUnifiedTabId,
    nextActiveUnifiedTabId,
    mirroredUnifiedIds,
    hostToLocalTabId
  }
}
