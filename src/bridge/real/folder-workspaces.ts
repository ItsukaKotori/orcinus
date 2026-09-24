import type { FolderWorkspacesApi } from '../../shared/preload-api/api/worktree-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/** Real `folderWorkspaces` adapter (spec §5.2): all five contract methods are backed by commands. */
export function createFolderWorkspacesRealApi(): FolderWorkspacesApi {
  return withMethodFallback<FolderWorkspacesApi>('folderWorkspaces', {
    list: () => invokeCommand('folder_workspaces_list'),
    getPathStatus: (args) => invokeCommand('folder_workspaces_get_path_status', { args }),
    create: (args) => invokeCommand('folder_workspaces_create', { args }),
    update: (args) => invokeCommand('folder_workspaces_update', { args }),
    delete: (args) => invokeCommand('folder_workspaces_delete', { args })
  })
}
