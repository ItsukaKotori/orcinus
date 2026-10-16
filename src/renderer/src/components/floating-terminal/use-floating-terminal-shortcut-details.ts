import { useShortcutKeyDetails } from '@/hooks/useShortcutLabel'

export function useFloatingTerminalShortcutDetails() {
  const newTerminalShortcut = useShortcutKeyDetails('tab.newTerminal')
  const newMarkdownShortcut = useShortcutKeyDetails('tab.newMarkdown')
  const openMarkdownShortcut = useShortcutKeyDetails('tab.openMarkdown')
  const closeShortcut = useShortcutKeyDetails('tab.close')

  return {
    newTerminalShortcut,
    newMarkdownShortcut,
    openMarkdownShortcut,
    closeShortcut
  }
}
