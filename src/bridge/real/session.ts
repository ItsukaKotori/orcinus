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
type SleepingRecords = NonNullable<WorkspaceSessionState['sleepingAgentSessionsByPaneKey']>

/**
 * Keeps the earliest-capturedAt record per `providerSession.id` and drops later
 * duplicates (records without a providerSession id pass through; ties keep the
 * first record in Object iteration order).
 */
function dedupeSleepingRecordsByProviderId(records: SleepingRecords): SleepingRecords {
  const kept: SleepingRecords = {}
  const claimByProviderId = new Map<string, { paneKey: string; record: SleepingRecords[string] }>()
  for (const [paneKey, record] of Object.entries(records)) {
    const providerId = record.providerSession?.id
    if (providerId === undefined) {
      kept[paneKey] = record
      continue
    }
    const claim = claimByProviderId.get(providerId)
    if (claim === undefined) {
      claimByProviderId.set(providerId, { paneKey, record })
      kept[paneKey] = record
      continue
    }
    if (record.capturedAt < claim.record.capturedAt) {
      // The stored order is paneKey order, not claim order: the earlier record may
      // sit under a later key, so evict the later duplicate from its own key.
      delete kept[claim.paneKey]
      claimByProviderId.set(providerId, { paneKey, record })
      kept[paneKey] = record
    }
  }
  return kept
}

export function createSessionRealApi(): Pick<PreloadApi, 'session'> {
  return {
    session: withMethodFallback<PreloadApi['session']>('session', {
      // Read-side normalization: the store keeps sparse rows (a first-run empty
      // store answers `{}`), but the contract is a full WorkspaceSessionState and
      // unguarded consumers in the hydration chain read top-level fields directly.
      // Stored rows win over the canonical defaults; set/patch/flush stay sparse.
      get: async () => {
        const stored = JSON.parse(await invokeCommand<string>('session_get')) as Partial<WorkspaceSessionState>
        // Why dedupe on read: capture-side claiming (agent-transcript-capture)
        // prevents NEW duplicates, but rows written by pre-dedupe builds can hold
        // the same providerSession id under several paneKeys — restore would then
        // resume the same session from multiple panes forever.
        const sleeping = stored.sleepingAgentSessionsByPaneKey
        return {
          ...getDefaultWorkspaceSession(),
          ...stored,
          ...(sleeping ? { sleepingAgentSessionsByPaneKey: dedupeSleepingRecordsByProviderId(sleeping) } : {})
        } as WorkspaceSessionState
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
