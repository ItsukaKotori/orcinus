import { runDeferredSessionReattachChoice } from './deferred-session-reattach-choice'
import type { ConnectPanePtySession } from './connect-pane-pty-session'

export function runDeferredSessionAttach(session: ConnectPanePtySession): void {
  // Why: the SSH-deferred attach flow retired with SSH support. Local and
  // runtime-owned panes always go straight to the reattach choice.
  if (session.connectionId) {
    return
  }
  runDeferredSessionReattachChoice(session)
}
