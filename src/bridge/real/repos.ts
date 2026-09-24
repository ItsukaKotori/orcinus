import type { RepositoryApi } from '../../shared/preload-api/api/repository-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Real `repos` adapter (spec §5.2). Clone/create-remote and the base-ref
 * helpers stay on `withMethodFallback`; the registry reads and mutations map
 * onto the `repos_*` commands.
 */
export function createReposRealApi(): RepositoryApi {
  return withMethodFallback<RepositoryApi>('repos', {
    list: () => invokeCommand('repos_list'),
    add: (args) => invokeCommand('repos_add', { args }),
    update: (args) => invokeCommand('repos_update', { args }),
    remove: (args) => invokeCommand('repos_remove', { args }),
    reorderForHost: (args) => invokeCommand('repos_reorder_for_host', { args }),
    pickFolder: () => invokeCommand('repos_pick_folder'),
    pickFolders: () => invokeCommand('repos_pick_folders'),
    pickDirectory: () => invokeCommand('repos_pick_directory'),
    isGitAvailable: () => invokeCommand('repos_is_git_available'),
    getDefaultCreateProjectParent: () => invokeCommand('repos_get_default_create_project_parent'),
    onChanged: (callback) => subscribeToEvent('repos:changed', () => callback())
  })
}
