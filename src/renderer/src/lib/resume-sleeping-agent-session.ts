import { useAppStore } from '@/store'
import {
  agentProviderSessionsEqual,
  type SleepingAgentSessionRecord
} from '../../../shared/agent-session-resume'
import { AGENT_STATUS_STALE_AFTER_MS } from '../../../shared/agent-status-types'
import {
  getProviderSessionClaimKey,
  isPassiveCompletedHibernationEvidence,
  recordPaneIsOwnedByPreservedPane
} from './sleeping-agent-pane-ownership'
import {
  launchSleepingAgentSession,
  type ResumeSleepingAgentSessionsOptions
} from './sleeping-agent-session-launch'
import { isStructuredAgentSyntheticSleepingRecord } from './structured-agent-synthetic-sleeping-record'
import { findUnhydratedHostMirrorForPane } from './host-mirrored-pane-liveness'
import { resolveWorkspaceTerminalHostAuthority } from './workspace-terminal-host-authority'
import { parkUntilHostSessionMirrorHydrates } from '@/runtime/host-session-mirror-hydration'
import { parseExecutionHostId } from '../../../shared/execution-host'
import { getExecutionHostIdForWorktree } from './worktree-runtime-owner'

export type { ResumeSleepingAgentSessionsOptions } from './sleeping-agent-session-launch'

function clearPassiveCompletedRecordsForClaimKey(
  records: readonly SleepingAgentSessionRecord[],
  claimKey: string,
  keepPaneKey: string
): void {
  const state = useAppStore.getState()
  for (const record of records) {
    if (record.paneKey === keepPaneKey || !isPassiveCompletedHibernationEvidence(record)) {
      continue
    }
    if (getProviderSessionClaimKey(record) === claimKey) {
      state.clearSleepingAgentSession(record.paneKey)
    }
  }
}

function getCurrentPaneOwnedClaimKeys(records: readonly SleepingAgentSessionRecord[]): Set<string> {
  const state = useAppStore.getState()
  const keys = new Set<string>()
  for (const record of records) {
    if (
      state.sleepingAgentSessionsByPaneKey[record.paneKey] !== record ||
      isInvalidWorktreeActivationRecord(record) ||
      isPassiveCompletedHibernationEvidence(record)
    ) {
      continue
    }
    if (recordPaneIsOwnedByPreservedPane(record, state)) {
      keys.add(getProviderSessionClaimKey(record))
    }
  }
  return keys
}

function getNewestActiveRecordsByClaimKey(
  records: readonly SleepingAgentSessionRecord[]
): Map<string, SleepingAgentSessionRecord> {
  const newestRecords = new Map<string, SleepingAgentSessionRecord>()
  for (const record of records) {
    const claimKey = getProviderSessionClaimKey(record)
    const current = newestRecords.get(claimKey)
    if (
      !current ||
      record.capturedAt > current.capturedAt ||
      (record.capturedAt === current.capturedAt && record.updatedAt > current.updatedAt)
    ) {
      newestRecords.set(claimKey, record)
    }
  }
  return newestRecords
}

function getAgentStatusTabId(entry: {
  paneKey: string
  tabId?: string | undefined
}): string | null {
  if (entry.tabId) {
    return entry.tabId
  }
  const separatorIndex = entry.paneKey.indexOf(':')
  return separatorIndex === -1 ? null : entry.paneKey.slice(0, separatorIndex)
}

/** True when the workspace's execution host is this client: the agent process
 *  died with the app, so a quit-origin record's session is genuinely closed. */
function workspaceExecutionHostIsThisClient(
  state: ReturnType<typeof useAppStore.getState>,
  worktreeId: string
): boolean {
  const host = parseExecutionHostId(getExecutionHostIdForWorktree(state, worktreeId))
  return host?.kind !== 'ssh' && host?.kind !== 'runtime'
}

/** True when the record's pane still exists in the restored session — including
 *  a husk tab whose PTY binding was cleared (it can still take a replacement
 *  resume tab). False when the tab was closed before quit, so nothing restored. */
function restoredSessionHasRecordPane(
  record: SleepingAgentSessionRecord,
  state: ReturnType<typeof useAppStore.getState>
): boolean {
  const tabId = getAgentStatusTabId(record)
  return tabId !== null && state.terminalLayoutsByTabId[tabId] !== undefined
}

function activeOrQueuedResumeClaimsProviderSession(
  record: SleepingAgentSessionRecord,
  state: ReturnType<typeof useAppStore.getState>,
  samePaneOwnsRecovery: boolean
): boolean {
  const worktreeTabIds = new Set(
    (state.tabsByWorktree[record.worktreeId] ?? []).map((tab) => tab.id)
  )
  for (const entry of Object.values(state.agentStatusByPaneKey)) {
    // Why: only an owned pane needs its record; hidden/live panes still dedupe by status.
    if (samePaneOwnsRecovery && entry.paneKey === record.paneKey) {
      continue
    }
    if (
      worktreeTabIds.has(getAgentStatusTabId(entry) ?? '') &&
      entry.worktreeId === record.worktreeId &&
      entry.agentType === record.agent &&
      entry.state !== 'done' &&
      agentProviderSessionsEqual(record.agent, entry.providerSession, record.providerSession)
    ) {
      return true
    }
  }

  for (const [tabId, startup] of Object.entries(state.pendingStartupByTabId)) {
    if (
      worktreeTabIds.has(tabId) &&
      startup.launchAgent === record.agent &&
      agentProviderSessionsEqual(
        record.agent,
        startup.resumeProviderSession,
        record.providerSession
      )
    ) {
      return true
    }
  }

  for (const [tabId, claim] of Object.entries(state.automaticAgentResumeClaimsByTabId)) {
    if (
      worktreeTabIds.has(tabId) &&
      claim.worktreeId === record.worktreeId &&
      claim.launchAgent === record.agent &&
      agentProviderSessionsEqual(record.agent, claim.providerSession, record.providerSession)
    ) {
      return true
    }
  }
  return false
}

// Why: an interrupted turn is still resumable — `claude --resume` reopens the transcript at the
// prompt — so discarding those records only stranded the session across wake and restart.
function isInvalidWorktreeActivationRecord(record: SleepingAgentSessionRecord): boolean {
  if (isStructuredAgentSyntheticSleepingRecord(record)) {
    return true
  }
  if (!record.origin && record.state === 'done') {
    return true
  }
  return (
    record.state !== 'done' && record.capturedAt - record.updatedAt > AGENT_STATUS_STALE_AFTER_MS
  )
}

function parkWorktreeResumeSweepUntilHostMirrorHydrates(
  worktreeId: string,
  environmentId: string | null,
  options: ResumeSleepingAgentSessionsOptions | undefined
): void {
  if (!environmentId) {
    // No paired runtime owns the workspace, so no verdict is coming; the next
    // activation re-runs this sweep once one does.
    return
  }
  parkUntilHostSessionMirrorHydrates(environmentId, worktreeId, () => {
    // Why: the mirror can settle long after the user moved on, so a replayed
    // resume must not steal the surface they are looking at now.
    const isActive = useAppStore.getState().activeWorktreeId === worktreeId
    // Why `skipClaimKeys` is dropped: it is a park-time snapshot of in-place
    // wakes, and a latch that has since failed must stay resumable here.
    resumeSleepingAgentSessionsForWorktree(worktreeId, {
      ...(options?.onSessionLaunched ? { onSessionLaunched: options.onSessionLaunched } : {}),
      ...(isActive ? {} : { suppressNavigation: true })
    })
  })
}

export function resumeSleepingAgentSessionsForWorktree(
  worktreeId: string,
  options?: ResumeSleepingAgentSessionsOptions
): number {
  const state = useAppStore.getState()
  // Why: every branch below reads local rows as the verdict on what the execution host is running,
  // and before it answers "I hold no pane for this record" is `unverifiable`, not `exited`. Resuming
  // on it forks a second agent onto a transcript the host is still writing (STA-3500). Declining is
  // recoverable — the record survives, and the caller re-runs this sweep once the verdict lands.
  // Paired-runtime workspaces keep their own per-pane mirror park below, which is finer-grained.
  if (resolveWorkspaceTerminalHostAuthority(state, worktreeId) === 'unverifiable') {
    return 0
  }
  const worktreeRecords = Object.values(state.sleepingAgentSessionsByPaneKey)
    .filter((record) => record.worktreeId === worktreeId)
    .sort((a, b) => a.capturedAt - b.capturedAt || a.updatedAt - b.updatedAt)
  const validWorktreeRecords = worktreeRecords.filter(
    (record) => !isInvalidWorktreeActivationRecord(record)
  )
  const activeWorktreeRecords = validWorktreeRecords.filter(
    (record) => !isPassiveCompletedHibernationEvidence(record)
  )
  const activeClaimKeys = new Set(activeWorktreeRecords.map(getProviderSessionClaimKey))
  const newestActiveRecordByClaimKey = getNewestActiveRecordsByClaimKey(activeWorktreeRecords)
  const freshlyLaunchedClaimKeys = new Set<string>()

  let launched = 0
  for (const record of worktreeRecords) {
    const currentState = useAppStore.getState()
    if (currentState.sleepingAgentSessionsByPaneKey[record.paneKey] !== record) {
      continue
    }
    const claimKey = getProviderSessionClaimKey(record)
    // Why: a mounted pane already consumed (or latched) the in-place
    // hibernation wake for this session; its record clears when that spawn
    // succeeds. Launching or clearing here would double-resume the session.
    if (options?.skipClaimKeys?.has(claimKey)) {
      continue
    }
    if (isInvalidWorktreeActivationRecord(record)) {
      state.clearSleepingAgentSession(record.paneKey)
      continue
    }
    const unhydratedMirror = findUnhydratedHostMirrorForPane(record, currentState)
    if (unhydratedMirror) {
      // Why: pane ownership is undecidable until the mirror answers, and every
      // branch below — launch and clear alike — trusts that verdict. Take no
      // action on the record; the replay re-runs this pass with real evidence.
      parkWorktreeResumeSweepUntilHostMirrorHydrates(
        worktreeId,
        unhydratedMirror.environmentId,
        options
      )
      continue
    }
    const isPaneOwned = recordPaneIsOwnedByPreservedPane(record, currentState)
    if (isPassiveCompletedHibernationEvidence(record)) {
      // Why: completed-agent hibernation is passive history; activation should
      // only keep displayable evidence, never start new work from it.
      if (!isPaneOwned || activeClaimKeys.has(claimKey)) {
        state.clearSleepingAgentSession(record.paneKey)
      }
      continue
    }
    if (activeOrQueuedResumeClaimsProviderSession(record, currentState, isPaneOwned)) {
      // Why: main can replay the old wake record after the same provider
      // session was already queued in a fresh tab; clear the stale replay.
      state.clearSleepingAgentSession(record.paneKey)
      continue
    }
    const paneOwnedClaimKeys = getCurrentPaneOwnedClaimKeys(activeWorktreeRecords)
    if (paneOwnedClaimKeys.has(claimKey)) {
      if (!isPaneOwned) {
        state.clearSleepingAgentSession(record.paneKey)
      }
      continue
    }
    if (freshlyLaunchedClaimKeys.has(claimKey)) {
      state.clearSleepingAgentSession(record.paneKey)
      continue
    }
    if (newestActiveRecordByClaimKey.get(claimKey) !== record) {
      state.clearSleepingAgentSession(record.paneKey)
      continue
    }
    if (isPaneOwned) {
      continue
    }
    // Why: quit-origin records describe panes that were still mounted at app
    // quit (agent-session-resume.ts). When that pane did not come back in the
    // restored session (its tab was closed before quitting), activation opening
    // a resume tab would fork a session the user explicitly closed. Such
    // orphaned records are kept indefinitely — the safe direction, since the
    // same session stays manually resumable — until a manual resume claims and
    // clears them. (The stale-record hygiene above only retires records whose
    // intra-record capturedAt/updatedAt delta exceeds
    // AGENT_STATUS_STALE_AFTER_MS; it never ages out wall-clock-old records.)
    // A restored husk tab still launches its
    // replacement here (preserved-pane replacement contract). Remote worktrees
    // are exempt: an ssh/runtime agent survives the relaunch independently, so
    // waking it after the host answers is the designed recovery (STA-3500).
    // Live-origin records also keep launching here: the web runtime's wake
    // replay is this sweep (pinned by
    // resume-sleeping-agent-session-replay.test.ts).
    if (
      record.origin === 'quit' &&
      workspaceExecutionHostIsThisClient(currentState, record.worktreeId) &&
      !restoredSessionHasRecordPane(record, currentState)
    ) {
      continue
    }
    if (launchSleepingAgentSession(record, options)) {
      launched += 1
      freshlyLaunchedClaimKeys.add(claimKey)
      clearPassiveCompletedRecordsForClaimKey(worktreeRecords, claimKey, record.paneKey)
    }
  }
  return launched
}
