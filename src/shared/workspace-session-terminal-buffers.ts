import type { Repo } from './repo-types'
import type { WorkspaceSessionState } from './workspace-session-state-types'
import { FLOATING_TERMINAL_WORKTREE_ID } from './constants'
import { getRepoIdFromWorktreeId } from './worktree/id'
import { TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT } from './terminal-scrollback-limits'
import { clampUtf8TextTail, isUtf8ByteLengthWithinLimit } from './utf8-byte-limits'

export type RepoConnection = Pick<Repo, 'id' | 'connectionId' | 'executionHostId'>

type RepoTerminalScrollbackOwner = Pick<RepoConnection, 'connectionId' | 'executionHostId'>

function repoNeedsRendererCapturedScrollback(_repo: RepoTerminalScrollbackOwner): boolean {
  // ade has no out-of-process PTY daemon (spec R2): renderer-captured
  // scrollback is the only durable copy for every repo kind, local included.
  // Signature kept — upstream flips this off when a daemon lands.
  return true
}

function shouldPreserveTerminalScrollbackBuffersForRepoMap(
  worktreeId: string | undefined,
  repoById: ReadonlyMap<string, RepoTerminalScrollbackOwner>
): boolean {
  if (worktreeId === undefined || worktreeId === FLOATING_TERMINAL_WORKTREE_ID) {
    return false
  }
  const repoId = getRepoIdFromWorktreeId(worktreeId)
  const repo = repoById.get(repoId)
  if (repo && repoNeedsRendererCapturedScrollback(repo)) {
    return true
  }
  if (!repoById.has(repoId)) {
    // Why: when the repo catalog is not hydrated, treating the worktree as
    // remote avoids losing the only scrollback source a relay/runtime terminal
    // may have.
    return true
  }
  return false
}

export function shouldPreserveTerminalScrollbackBuffers(
  worktreeId: string | undefined,
  repos: readonly RepoConnection[]
): boolean {
  return shouldPreserveTerminalScrollbackBuffersForRepoMap(
    worktreeId,
    new Map(repos.map((repo) => [repo.id, repo] as const))
  )
}

export function capTerminalScrollbackSessionBuffer(buffer: string): string {
  if (isUtf8ByteLengthWithinLimit(buffer, TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT)) {
    return buffer
  }
  return clampUtf8TextTail(buffer, TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT).text
}

function capTerminalScrollbackLeafBuffers(buffers: Record<string, string> | undefined): {
  buffers: Record<string, string> | undefined
  changed: boolean
} {
  if (!buffers) {
    return { buffers: undefined, changed: false }
  }
  let changed = false
  const capped: Record<string, string> = {}
  for (const [leafId, buffer] of Object.entries(buffers)) {
    const next = capTerminalScrollbackSessionBuffer(buffer)
    capped[leafId] = next
    changed ||= next !== buffer
  }
  return { buffers: Object.keys(capped).length > 0 ? capped : undefined, changed }
}

export function pruneLocalTerminalScrollbackBuffers(
  session: WorkspaceSessionState,
  repos: readonly RepoConnection[]
): WorkspaceSessionState {
  let repoById: Map<string, RepoConnection> | null = null
  let worktreeIdByTabId: Map<string, string> | null = null
  const tabsByWorktree = session.tabsByWorktree ?? {}
  const terminalLayoutsByTabIdForRead = session.terminalLayoutsByTabId ?? {}
  let terminalLayoutsByTabId: WorkspaceSessionState['terminalLayoutsByTabId'] | null = null
  for (const [tabId, layout] of Object.entries(terminalLayoutsByTabIdForRead)) {
    if (!layout.buffersByLeafId && !layout.scrollbackRefsByLeafId) {
      continue
    }
    repoById ??= new Map(repos.map((repo) => [repo.id, repo] as const))
    if (!worktreeIdByTabId) {
      worktreeIdByTabId = new Map()
      for (const [worktreeId, tabs] of Object.entries(tabsByWorktree)) {
        for (const tab of tabs) {
          worktreeIdByTabId.set(tab.id, worktreeId)
        }
      }
    }
    const worktreeId = worktreeIdByTabId.get(tabId)
    if (shouldPreserveTerminalScrollbackBuffersForRepoMap(worktreeId, repoById)) {
      const capped = capTerminalScrollbackLeafBuffers(layout.buffersByLeafId)
      if (capped.changed) {
        terminalLayoutsByTabId ??= { ...terminalLayoutsByTabIdForRead }
        terminalLayoutsByTabId[tabId] = { ...layout, buffersByLeafId: capped.buffers }
      }
      continue
    }

    terminalLayoutsByTabId ??= { ...terminalLayoutsByTabIdForRead }
    const layoutWithoutBuffers = { ...layout }
    delete layoutWithoutBuffers.buffersByLeafId
    delete layoutWithoutBuffers.scrollbackRefsByLeafId
    terminalLayoutsByTabId[tabId] = layoutWithoutBuffers
  }

  if (!terminalLayoutsByTabId) {
    return session
  }

  return {
    ...session,
    // Why: with no out-of-process daemon (spec R2), renderer-captured buffers
    // are the only durable scrollback for every repo kind, so this pass only
    // prunes unclassifiable/floating tabs and caps the rest per buffer.
    terminalLayoutsByTabId
  }
}
