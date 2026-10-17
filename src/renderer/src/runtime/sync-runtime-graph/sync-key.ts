import { getSystemPrefersDark } from '@/lib/terminal-theme'
import type { AppState } from '@/store/types'
import { EMPTY_AGENT_STATUS_BY_PANE_KEY, EMPTY_LAYOUT_BY_WORKTREE } from './graph-state'
import {
  buildRuntimeMobileAgentStatusProjection,
  buildRuntimeMobileAgentStatusProjectionForTests,
  resetRuntimeMobileAgentStatusProjectionCacheForTests,
  AGENT_STATUS_SYNC_UPDATED_AT_BUCKET_MS_FOR_TESTS
} from './agent-status-projection'
import {
  buildRuntimeMobileEditorDraftsProjection,
  buildRuntimeMobileOpenFilesProjection,
  buildRuntimeMobileTabsProjection
} from './sync-projections'
import { resolveMobileTerminalTheme } from './mobile-terminal-theme'
import type { RuntimeMobileSessionSyncKey } from './types'

export {
  AGENT_STATUS_SYNC_UPDATED_AT_BUCKET_MS_FOR_TESTS,
  buildRuntimeMobileAgentStatusProjectionForTests,
  resetRuntimeMobileAgentStatusProjectionCacheForTests
}
export type { RuntimeMobileSessionSyncKey } from './types'

function getTerminalThemeSystemPrefersDark(
  state: Pick<AppState, 'settings'>,
  systemPrefersDark: boolean | null | undefined
): boolean | null {
  return state.settings?.theme === 'system' ? (systemPrefersDark ?? null) : null
}

export function canSkipRuntimeMobileSessionSyncKeyBuild(
  state: AppState,
  previousState: AppState,
  systemPrefersDark?: boolean,
  previousSystemPrefersDark: boolean | null | undefined = systemPrefersDark
): boolean {
  const currentThemePreference = getTerminalThemeSystemPrefersDark(state, systemPrefersDark)
  const previousThemePreference = getTerminalThemeSystemPrefersDark(
    previousState,
    previousSystemPrefersDark
  )
  return (
    currentThemePreference === previousThemePreference &&
    state.tabsByWorktree === previousState.tabsByWorktree &&
    state.groupsByWorktree === previousState.groupsByWorktree &&
    state.activeGroupIdByWorktree === previousState.activeGroupIdByWorktree &&
    state.layoutByWorktree === previousState.layoutByWorktree &&
    state.unifiedTabsByWorktree === previousState.unifiedTabsByWorktree &&
    state.tabBarOrderByWorktree === previousState.tabBarOrderByWorktree &&
    state.activeFileId === previousState.activeFileId &&
    state.activeFileIdByWorktree === previousState.activeFileIdByWorktree &&
    state.activeTabType === previousState.activeTabType &&
    state.activeTabTypeByWorktree === previousState.activeTabTypeByWorktree &&
    state.openFiles === previousState.openFiles &&
    state.editorDrafts === previousState.editorDrafts &&
    state.settings === previousState.settings &&
    state.activeTabId === previousState.activeTabId &&
    state.terminalLayoutsByTabId === previousState.terminalLayoutsByTabId &&
    state.runtimePaneTitlesByTabId === previousState.runtimePaneTitlesByTabId &&
    state.folderWorkspaces === previousState.folderWorkspaces &&
    state.agentStatusEpoch === previousState.agentStatusEpoch &&
    state.agentStatusByPaneKey === previousState.agentStatusByPaneKey
  )
}

export function getRuntimeMobileSessionSyncKey(
  state: AppState,
  previousState?: AppState,
  previousKey?: RuntimeMobileSessionSyncKey,
  systemPrefersDark = getSystemPrefersDark()
): RuntimeMobileSessionSyncKey {
  const canReusePrevious = previousState !== undefined && previousKey !== undefined
  const themeSystemPrefersDark = getTerminalThemeSystemPrefersDark(state, systemPrefersDark)
  const agentStatusByPaneKey = state.agentStatusByPaneKey ?? EMPTY_AGENT_STATUS_BY_PANE_KEY
  const previousAgentStatusByPaneKey = previousState
    ? (previousState.agentStatusByPaneKey ?? EMPTY_AGENT_STATUS_BY_PANE_KEY)
    : EMPTY_AGENT_STATUS_BY_PANE_KEY
  return {
    terminalLayoutsByTabId: state.terminalLayoutsByTabId,
    runtimePaneTitlesByTabId: state.runtimePaneTitlesByTabId,
    folderWorkspaces: state.folderWorkspaces,
    groupsByWorktree: state.groupsByWorktree,
    activeGroupIdByWorktree: state.activeGroupIdByWorktree,
    layoutByWorktree: state.layoutByWorktree ?? EMPTY_LAYOUT_BY_WORKTREE,
    unifiedTabsByWorktree: state.unifiedTabsByWorktree,
    tabBarOrderByWorktree: state.tabBarOrderByWorktree,
    activeFileId: state.activeFileId,
    activeFileIdByWorktree: state.activeFileIdByWorktree,
    activeTabType: state.activeTabType,
    activeTabTypeByWorktree: state.activeTabTypeByWorktree,
    activeTabId: state.activeTabId,
    agentStatusEpoch: state.agentStatusEpoch ?? 0,
    agentStatusProjection:
      canReusePrevious && agentStatusByPaneKey === previousAgentStatusByPaneKey
        ? previousKey.agentStatusProjection
        : buildRuntimeMobileAgentStatusProjection(agentStatusByPaneKey),
    generatedTabTitlesEnabled: state.settings?.tabAutoGenerateTitle === true,
    systemPrefersDark: themeSystemPrefersDark,
    terminalThemeProjection:
      canReusePrevious &&
      state.settings === previousState.settings &&
      previousKey.systemPrefersDark === themeSystemPrefersDark
        ? previousKey.terminalThemeProjection
        : JSON.stringify(resolveMobileTerminalTheme(state, systemPrefersDark) ?? null),
    tabsProjection:
      canReusePrevious && state.tabsByWorktree === previousState.tabsByWorktree
        ? previousKey.tabsProjection
        : buildRuntimeMobileTabsProjection(state.tabsByWorktree),
    openFilesProjection:
      canReusePrevious && state.openFiles === previousState.openFiles
        ? previousKey.openFilesProjection
        : buildRuntimeMobileOpenFilesProjection(state.openFiles),
    editorDraftsProjection:
      canReusePrevious && state.editorDrafts === previousState.editorDrafts
        ? previousKey.editorDraftsProjection
        : buildRuntimeMobileEditorDraftsProjection(state.editorDrafts)
  }
}

export function runtimeMobileSessionSyncKeysEqual(
  a: RuntimeMobileSessionSyncKey,
  b: RuntimeMobileSessionSyncKey
): boolean {
  return (
    a.terminalLayoutsByTabId === b.terminalLayoutsByTabId &&
    a.runtimePaneTitlesByTabId === b.runtimePaneTitlesByTabId &&
    a.folderWorkspaces === b.folderWorkspaces &&
    a.groupsByWorktree === b.groupsByWorktree &&
    a.activeGroupIdByWorktree === b.activeGroupIdByWorktree &&
    a.layoutByWorktree === b.layoutByWorktree &&
    a.unifiedTabsByWorktree === b.unifiedTabsByWorktree &&
    a.tabBarOrderByWorktree === b.tabBarOrderByWorktree &&
    a.activeFileId === b.activeFileId &&
    a.activeFileIdByWorktree === b.activeFileIdByWorktree &&
    a.activeTabType === b.activeTabType &&
    a.activeTabTypeByWorktree === b.activeTabTypeByWorktree &&
    a.activeTabId === b.activeTabId &&
    a.agentStatusEpoch === b.agentStatusEpoch &&
    a.agentStatusProjection === b.agentStatusProjection &&
    a.generatedTabTitlesEnabled === b.generatedTabTitlesEnabled &&
    a.systemPrefersDark === b.systemPrefersDark &&
    a.terminalThemeProjection === b.terminalThemeProjection &&
    a.tabsProjection === b.tabsProjection &&
    a.openFilesProjection === b.openFilesProjection &&
    a.editorDraftsProjection === b.editorDraftsProjection
  )
}
