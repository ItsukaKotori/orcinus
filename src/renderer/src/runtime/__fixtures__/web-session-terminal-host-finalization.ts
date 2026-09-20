import type { RuntimeMobileSessionTabsResult } from '../../../../shared/runtime-types'
import { dropRetirementProofsForLiveSurfaces } from '../../../../shared/terminal-retirement-proof-ledger'

/** Run terminal fixtures through the host's retirement filter before the renderer consumes them. */
export function finalizeHostTerminalSnapshot(
  snapshot: RuntimeMobileSessionTabsResult
): RuntimeMobileSessionTabsResult {
  if (snapshot.tabGroups !== undefined || snapshot.tabGroupLayout !== undefined) {
    throw new Error('This fixture only supports ungrouped terminal snapshot finalization')
  }
  const tabs = snapshot.tabs
  const active =
    tabs.find((tab) => tab.isActive && tab.id === snapshot.activeTabId) ??
    tabs.find((tab) => tab.isActive) ??
    (snapshot.activeTabId ? (tabs[0] ?? null) : null)
  const normalizedTabs =
    active && !tabs.some((tab) => tab.isActive)
      ? tabs.map((tab) => (tab.id === active.id ? { ...tab, isActive: true } : tab))
      : tabs
  return {
    worktree: snapshot.worktree,
    publicationEpoch: snapshot.publicationEpoch,
    snapshotVersion: snapshot.snapshotVersion,
    activeGroupId: null,
    activeTabId: active?.id ?? null,
    activeTabType: active?.type ?? null,
    ...(snapshot.retiredTerminalSurfaces
      ? {
          retiredTerminalSurfaces: dropRetirementProofsForLiveSurfaces(
            snapshot.retiredTerminalSurfaces,
            snapshot.tabs
          )
        }
      : {}),
    tabs: normalizedTabs
  }
}
