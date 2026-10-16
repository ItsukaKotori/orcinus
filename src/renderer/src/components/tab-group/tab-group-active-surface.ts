import type { Tab, WorkspaceVisibleTabType } from '../../../../shared/tab-types'
import { toVisibleTabType } from '../../../../shared/tab-types'

export type TabGroupActiveSurface = {
  activeTabId: string | null
  activeFileId: string | null
  activeTabType: WorkspaceVisibleTabType
}

/**
 * Maps the group's active unified tab onto the TabBar's active-surface props.
 * Terminal rows key off their backing entity id; editor-family rows
 * off the unified tab id; self-backed rows (agent sessions, plugin tabs) off
 * the unified tab id so the strip highlight matches the rendered surface.
 * `activeTabType` collapses editor-family kinds to `'editor'` and keeps
 * `'plugin'` distinct instead of mislabelling plugin tabs as editor rows.
 */
export function resolveTabGroupActiveSurface(activeTab: Tab | null): TabGroupActiveSurface {
  if (!activeTab) {
    return {
      activeTabId: null,
      activeFileId: null,
      activeTabType: 'editor'
    }
  }

  const activeTabType = toVisibleTabType(activeTab.contentType)
  return {
    activeTabId:
      activeTabType === 'terminal'
        ? activeTab.entityId
        : activeTabType === 'agent-session' || activeTabType === 'plugin'
          ? activeTab.id
          : null,
    activeFileId: activeTabType === 'editor' ? activeTab.id : null,
    activeTabType
  }
}
