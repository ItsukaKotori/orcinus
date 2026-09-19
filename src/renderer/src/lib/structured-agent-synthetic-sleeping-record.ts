import type { SleepingAgentSessionRecord } from '../../../shared/agent-session-resume'
import { parsePaneKey } from '../../../shared/stable-pane-id'

function structuredAgentSessionTabId(sessionId: string): string {
  return `structured-agent-session-${sessionId}`
}

/** Old structured projections persisted their desktop id as if a terminal could resume it. */
export function isStructuredAgentSyntheticSleepingRecord(
  record: SleepingAgentSessionRecord
): boolean {
  const pane = parsePaneKey(record.paneKey)
  return (
    pane !== null &&
    record.providerSession.key === 'session_id' &&
    structuredAgentSessionTabId(record.providerSession.id) === pane.tabId
  )
}
