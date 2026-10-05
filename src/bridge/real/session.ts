import type { PreloadApi } from '../../preload/api-types'
import { getDefaultWorkspaceSession } from '../../shared/constants'
import type {
  WorkspaceSessionPatch,
  WorkspaceSessionState
} from '../../shared/workspace-session-state-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Workspace session state (spec §3.2): opaque JSON documents over opaque
 * `String` payloads. `readTerminalScrollback` stays null — the refs lane was
 * the deleted remote-mirror domain's; inline `buffersByLeafId` is the restore
 * source (spec R1).
 *
 * Why only the `session` key: `PreloadApi` flattens the workspace-session
 * contract into three sibling domains (`session` / `cache` / `remoteWorkspace`),
 * and real mode spreads the mock inventory under the real domains, so
 * `cache` and `remoteWorkspace` keep their Phase 0 mock implementations
 * (spec §2.2 keeps them out of 2A scope).
 */
export function createSessionRealApi(): Pick<PreloadApi, 'session'> {
  return {
    session: withMethodFallback<PreloadApi['session']>('session', {
      // Read-side normalization: the store keeps sparse rows (a first-run empty
      // store answers `{}`), but the contract is a full WorkspaceSessionState and
      // unguarded consumers in the hydration chain read top-level fields directly.
      // Stored rows win over the canonical defaults; set/patch/flush stay sparse.
      get: async () => {
        const stored = JSON.parse(await invokeCommand<string>('session_get')) as Partial<WorkspaceSessionState>
        return { ...getDefaultWorkspaceSession(), ...stored } as WorkspaceSessionState
      },
      set: async (args: WorkspaceSessionState) => {
        await invokeCommand('session_set', { args: JSON.stringify(args) })
      },
      patch: async (args: WorkspaceSessionPatch) => {
        await invokeCommand('session_patch', { args: JSON.stringify(args) })
      },
      flush: async () => {
        await invokeCommand('session_flush')
      },
      readTerminalScrollback: () => null,
      // Fire-and-forget: the void contract has no error lane (same precedent
      // as `pty.write`); persistence failures surface on the next `get`.
      setSync: (args: WorkspaceSessionState) => {
        void invokeCommand('session_set', { args: JSON.stringify(args) }).catch(() => {})
      }
    })
  }
}

export type SessionFlushHandler = () => Promise<void>

let flushHandler: SessionFlushHandler | null = null
let flushSubscriptionStarted = false

/**
 * Quit handshake (spec §3.4): the host prevents exit, emits
 * `session:flush-requested`, and waits for `session_flush_ack` (2s timeout).
 * The ack flows regardless of handler outcome — persistence failures must not
 * wedge the quit.
 */
export function registerSessionFlushHandler(handler: SessionFlushHandler): () => void {
  flushHandler = handler
  if (!flushSubscriptionStarted) {
    flushSubscriptionStarted = true
    subscribeToEvent('session:flush-requested', () => {
      const current = flushHandler
      const run = current ? current() : Promise.resolve()
      void run
        .catch(() => {})
        .finally(() => {
          void invokeCommand('session_flush_ack').catch(() => {})
        })
    })
  }
  return () => {
    if (flushHandler === handler) {
      flushHandler = null
    }
  }
}
