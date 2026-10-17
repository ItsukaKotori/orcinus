import type { Terminal } from '@xterm/xterm'
import type { LinkHandlerDeps } from './terminal-link-handlers'
import { isTerminalOwnedLinkGesture } from './terminal-link-activation'
import { handleOscLink } from './terminal-osc-link-routing'
import {
  findHttpLinkAtTerminalMouseEvent,
  handleTerminalHttpLink
} from './terminal-url-link-hit-testing'
import type { HttpLinkSourceOwner } from '@/lib/http-link-routing'
import type { TerminalLinkActionContext } from './terminal-link-action-request'

type TerminalWebLinkClickDeps = Pick<
  LinkHandlerDeps,
  'worktreeId' | 'worktreePath' | 'startupCwd' | 'runtimeEnvironmentId' | 'terminalHomePath'
> & {
  terminal: Terminal | null
  sourceOwner?: HttpLinkSourceOwner
  linkActionContext?: TerminalLinkActionContext | null
}

export function handleTerminalWebLinkClick(
  url: string,
  event: MouseEvent | undefined,
  deps: TerminalWebLinkClickDeps
): boolean {
  if (!event || !isTerminalOwnedLinkGesture(event)) {
    return false
  }

  let handled: boolean
  const completeUrl = deps.terminal ? findHttpLinkAtTerminalMouseEvent(deps.terminal, event) : null
  if (completeUrl) {
    handled = handleTerminalHttpLink(completeUrl, event, {
      sourceOwner:
        deps.sourceOwner ??
        (deps.runtimeEnvironmentId
          ? { kind: 'runtime', runtimeEnvironmentId: deps.runtimeEnvironmentId }
          : { kind: 'local' }),
      linkActionContext: deps.linkActionContext
    })
    // Why: WebLinksAddon only knows the physical row; Orca's logical hit-test
    // preserves the complete URL rendered across hard-wrapped TUI rows.
    event.preventDefault()
  } else {
    handled = handleOscLink(url, event, deps)
  }

  if (handled) {
    // Why: link navigation can steal focus before xterm's mouseup cleanup;
    // clearing selection also detaches its pending drag-selection listeners.
    deps.terminal?.clearSelection()
  }
  return handled
}
