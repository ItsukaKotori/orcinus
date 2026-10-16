export function isMacPlatform(): boolean {
  return navigator.userAgent.includes('Mac')
}

function terminalLinkActionHintPrefix(showActions: boolean): string {
  return showActions ? 'Click for actions, ' : ''
}

export function getTerminalFileOpenHint(showActions = true): string {
  const prefix = terminalLinkActionHintPrefix(showActions)
  return isMacPlatform()
    ? `${prefix}⌘+click to open, or ⇧⌘+click for default app`
    : `${prefix}Ctrl+click to open, or Shift+Ctrl+click for default app`
}

export function getTerminalOrcaFileOpenHint(showActions = true): string {
  const prefix = showActions ? 'Click for actions or ' : ''
  return isMacPlatform()
    ? `${prefix}⌘+click to open in Orca`
    : `${prefix}Ctrl+click to open in Orca`
}

export type TerminalUrlOpenHintOptions = {
  showActions?: boolean
}

// Why: http links always open in the system browser now that the managed browser is gone.
export function getTerminalUrlOpenHint(options: TerminalUrlOpenHintOptions = {}): string {
  const prefix = terminalLinkActionHintPrefix(options.showActions !== false)
  return isMacPlatform()
    ? `${prefix}⌘+click to open, or ⇧⌘+click for system browser`
    : `${prefix}Ctrl+click to open, or Shift+Ctrl+click for system browser`
}

export function getTerminalWorktreePathOpenHint(
  canOpenWithSystemDefault: boolean,
  showActions = true
): string {
  const prefix = terminalLinkActionHintPrefix(showActions)
  if (!canOpenWithSystemDefault) {
    const directPrefix = showActions ? 'Click for actions or ' : ''
    return isMacPlatform()
      ? `${directPrefix}⌘+click to switch workspace`
      : `${directPrefix}Ctrl+click to switch workspace`
  }

  return isMacPlatform()
    ? `${prefix}⌘+click to switch workspace, or ⇧⌘+click to open in Finder`
    : `${prefix}Ctrl+click to switch workspace, or Shift+Ctrl+click to open folder`
}
