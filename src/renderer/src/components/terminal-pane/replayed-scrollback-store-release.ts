import {
  shouldPreserveTerminalScrollbackBuffers,
  type RepoConnection
} from '../../../../shared/workspace-session-terminal-buffers'

type ReplayedScrollbackReleaseArgs = {
  hasScrollbackRefs: boolean
  worktreeId: string | undefined
  repos: readonly RepoConnection[]
}

/** Whether a replayed pane may drop its store-held scrollback copy now that xterm owns the bytes.
 *  Inverse of the force-park capture guard: keep the copy only where nothing can re-create it. */
export function canReleaseReplayedScrollbackFromStore({
  hasScrollbackRefs,
  worktreeId,
  repos
}: ReplayedScrollbackReleaseArgs): boolean {
  // Every repo kind re-mints its copy at the next park/shutdown capture (spec R2:
  // no daemon, so the renderer capture runs for local worktrees too), and refs
  // re-hydrate from disk — releasing the store copy therefore loses nothing.
  return hasScrollbackRefs || shouldPreserveTerminalScrollbackBuffers(worktreeId, repos)
}
