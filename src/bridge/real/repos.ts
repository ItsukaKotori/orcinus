import type { RepositoryApi } from '../../shared/preload-api/api/repository-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Real `repos` adapter (spec §5.2). The registry reads and mutations, local
 * scratch-project creation, and the base-ref helpers map onto the `repos_*`
 * commands; clone/create-remote (and the host-scoped variants) stay on
 * `withMethodFallback`. `create` resolves the `{ repo } | { error }` contract
 * union untouched — callers branch on `'error' in result`.
 */
export function createReposRealApi(): RepositoryApi {
  return withMethodFallback<RepositoryApi>('repos', {
    list: () => invokeCommand('repos_list'),
    add: (args) => invokeCommand('repos_add', { args }),
    create: (args) => invokeCommand('repos_create', { args }),
    getBaseRefDefault: (args) => invokeCommand('repos_get_base_ref_default', { args }),
    searchBaseRefs: (args) => invokeCommand('repos_search_base_refs', { args }),
    searchBaseRefDetails: (args) => invokeCommand('repos_search_base_ref_details', { args }),
    update: (args) => invokeCommand('repos_update', { args }),
    remove: (args) => invokeCommand('repos_remove', { args }),
    reorderForHost: (args) => invokeCommand('repos_reorder_for_host', { args }),
    pickFolder: () => invokeCommand('repos_pick_folder'),
    pickFolders: () => invokeCommand('repos_pick_folders'),
    pickDirectory: () => invokeCommand('repos_pick_directory'),
    isGitAvailable: () => invokeCommand('repos_is_git_available'),
    getDefaultCreateProjectParent: () => invokeCommand('repos_get_default_create_project_parent'),
    onChanged: (callback) => subscribeToEvent('repos:changed', () => callback()),
    // Why: the renderer subscribes unconditionally, but A never clones, so the
    // subscription must still hand back a working unsubscribe handle.
    onCloneProgress: () => () => {}
  })
}
