import type { TerminalTab } from '../../../../shared/terminal-tab-types'

export function terminalTabEqual(a: TerminalTab, b: TerminalTab): boolean {
  return (
    a.id === b.id &&
    a.ptyId === b.ptyId &&
    a.worktreeId === b.worktreeId &&
    a.title === b.title &&
    a.defaultTitle === b.defaultTitle &&
    a.quickCommandLabel === b.quickCommandLabel &&
    a.startupCwd === b.startupCwd &&
    a.generatedTitle === b.generatedTitle &&
    a.aiVaultTitle?.agent === b.aiVaultTitle?.agent &&
    a.aiVaultTitle?.sessionId === b.aiVaultTitle?.sessionId &&
    a.aiVaultTitle?.title === b.aiVaultTitle?.title &&
    a.customTitle === b.customTitle &&
    a.color === b.color &&
    a.sortOrder === b.sortOrder &&
    a.createdAt === b.createdAt &&
    a.generation === b.generation &&
    a.shellOverride === b.shellOverride &&
    a.launchAgent === b.launchAgent &&
    a.pendingActivationSpawn === b.pendingActivationSpawn
  )
}

export function sameTerminalTabs(
  a: readonly TerminalTab[] | undefined,
  b: readonly TerminalTab[] | null
): boolean {
  const left = a ?? []
  const right = b ?? []
  if (left.length !== right.length) {
    return false
  }
  return left.every((tab, index) => terminalTabEqual(tab, right[index]!))
}
