import { FLOATING_TERMINAL_WORKTREE_ID } from '../../../../shared/constants'
import type { RuntimeMobileSessionTabsResult } from '../../../../shared/runtime-types'
import type {
  WebSessionTabsBatchContext,
  WebSessionTabsSnapshotApplyOptions,
  WebSessionTabsSyncState
} from './state'
import { prepareWebSessionTabsSnapshotBase } from './apply-preparation-base'
import { prepareWebSessionTabsSnapshotSessions } from './apply-preparation-sessions'
import { prepareWebSessionTabsSnapshotUnified } from './apply-preparation-unified'
import { prepareWebSessionTabsSnapshotGroups } from './apply-preparation-groups'
import { applyTerminalRecordUpdates } from './apply-terminal-records'
import { applyWorktreeRecordUpdates } from './apply-worktree-records'
import { applyActiveStateUpdates } from './apply-active-state'
import { buildWebSessionTabsFinalPatch } from './apply-final-patch'

/** Reconcile one host frame through the staged terminal/layout pipeline. */
export function applyWebSessionTabsSnapshotWithContext(
  state: WebSessionTabsSyncState,
  rawSnapshot: RuntimeMobileSessionTabsResult,
  environmentId: string,
  now = Date.now(),
  batchContext?: WebSessionTabsBatchContext,
  options?: WebSessionTabsSnapshotApplyOptions
): WebSessionTabsSyncState | Partial<WebSessionTabsSyncState> {
  if (rawSnapshot.worktree === FLOATING_TERMINAL_WORKTREE_ID) {
    return state
  }
  const worktreeId = rawSnapshot.worktree
  const base = prepareWebSessionTabsSnapshotBase(
    state,
    rawSnapshot,
    environmentId,
    worktreeId,
    now,
    batchContext,
    options
  )
  const sessions = prepareWebSessionTabsSnapshotSessions(base)
  const unified = prepareWebSessionTabsSnapshotUnified(sessions)
  const groups = prepareWebSessionTabsSnapshotGroups(unified)
  const terminalRecords = applyTerminalRecordUpdates(groups)
  const worktreeRecords = applyWorktreeRecordUpdates(terminalRecords)
  const activeState = applyActiveStateUpdates(worktreeRecords)
  return buildWebSessionTabsFinalPatch(activeState)
}
