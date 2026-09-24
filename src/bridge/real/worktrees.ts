import type { WorktreeApi } from '../../shared/preload-api/api/worktree-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Real `worktrees` adapter (spec §5.3): A only projects the registry, so
 * `list`/`listAll`/`onChanged` are real and every git-mutating or
 * detected-worktree surface stays on `withMethodFallback`.
 */
export function createWorktreesRealApi(): WorktreeApi {
  return withMethodFallback<WorktreeApi>('worktrees', {
    list: (args) => invokeCommand('worktrees_list', { args }),
    listAll: () => invokeCommand('worktrees_list_all'),
    onChanged: (callback) => subscribeToEvent('worktrees:changed', callback)
  })
}
