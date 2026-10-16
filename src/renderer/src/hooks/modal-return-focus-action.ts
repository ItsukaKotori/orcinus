import type { WorkspaceVisibleTabType } from '../../../shared/tab-types'

// The surface that held focus before a modal (QuickOpen, Cmd+J, ...) opened.
// Captured at open time because Radix steals document focus once the dialog
// mounts, so the raw activeElement is gone by close time.
export type ModalReturnFocusSurface = {
  tabType: WorkspaceVisibleTabType
  worktreeId: string | null
  terminalTabId: string | null
  terminalLeafId: string | null
}

export type ModalReturnFocusAction =
  | { kind: 'terminal'; tabId: string; leafId: string | null }
  | { kind: 'editor' }
  | { kind: 'surface' }
  | { kind: 'none' }

// Why: surfaces need type-specific DOM focus so a hidden xterm cannot steal
// focus from the active editor.
export function resolveModalReturnFocusAction(
  captured: ModalReturnFocusSurface | null
): ModalReturnFocusAction {
  if (!captured) {
    return { kind: 'none' }
  }
  if (captured.tabType === 'terminal' && captured.terminalTabId) {
    return { kind: 'terminal', tabId: captured.terminalTabId, leafId: captured.terminalLeafId }
  }
  if (captured.tabType === 'editor' && captured.worktreeId) {
    return { kind: 'editor' }
  }
  if (captured.worktreeId) {
    return { kind: 'surface' }
  }
  return { kind: 'none' }
}
