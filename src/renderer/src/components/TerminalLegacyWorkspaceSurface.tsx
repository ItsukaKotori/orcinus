import { TerminalLegacyTerminalPanes } from './TerminalLegacyTerminalPanes'
import { TerminalLegacyEditorSurface } from './TerminalLegacyEditorSurface'
import type { TerminalController } from './use-terminal-controller'

export function TerminalLegacyWorkspaceSurface({
  controller
}: {
  controller: TerminalController
}): React.JSX.Element | null {
  if (controller.effectiveActiveLayout || controller.anyMountedWorktreeHasLayout) {
    return null
  }
  return (
    <>
      <TerminalLegacyTerminalPanes controller={controller} />
      <TerminalLegacyEditorSurface controller={controller} />
    </>
  )
}
