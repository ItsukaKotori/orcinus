import type React from 'react'

export function TerminalWorkbenchContainer({
  isVisible,
  children
}: {
  isVisible: boolean
  children: React.ReactNode
}): React.JSX.Element {
  return (
    <div
      className={isVisible ? 'flex flex-1 min-w-0 min-h-0' : 'hidden flex-1 min-w-0 min-h-0'}
      // Why: a paintable-but-hidden workbench must stay unreachable by Tab / assistive tech.
      inert={!isVisible}
      aria-hidden={!isVisible}
      data-terminal-workbench-container=""
    >
      {children}
    </div>
  )
}
