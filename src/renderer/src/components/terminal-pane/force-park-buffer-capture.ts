import {
  shouldPreserveTerminalScrollbackBuffers,
  type RepoConnection
} from '../../../../shared/workspace-session-terminal-buffers'
import { captureTerminalShutdownBuffersBestEffort } from './shutdown-buffer-captures'

type ForceParkedWorktreeCaptureArgs = {
  worktreeId: string
  tabIds: readonly string[]
  repos: readonly RepoConnection[]
}

/** Serialize a force-parked worktree's panes before eviction unmounts them.
 *  Returns whether the episode covered every tab; false leaves it unmarked so a later episode retries. */
export function captureForceParkedWorktreeBuffers({
  worktreeId,
  tabIds,
  repos
}: ForceParkedWorktreeCaptureArgs): boolean {
  // Why capture for every repo kind (spec R2): with no daemon, renderer-captured
  // buffers are the only durable scrollback, so a force-parked local pane must
  // re-mint them exactly like a remote one before eviction unmounts it.
  if (!shouldPreserveTerminalScrollbackBuffers(worktreeId, repos)) {
    return true
  }
  const { requested, captured } = captureTerminalShutdownBuffersBestEffort(tabIds, {
    includeLocalBuffers: false
  })
  return captured === requested
}
