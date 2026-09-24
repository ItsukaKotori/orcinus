import type { FilesystemApi } from '../../shared/preload-api/api/filesystem-api'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Real `fs` adapter (spec §5.1). Implemented methods map 1:1 onto the
 * `fs_*` commands; the download, log-tail, and external-import surfaces stay on
 * `withMethodFallback` until their subprojects land.
 */
export function createFsRealApi(): FilesystemApi['fs'] {
  return withMethodFallback<FilesystemApi['fs']>('fs', {
    readDir: (args) => invokeCommand('fs_read_dir', { args }),
    readFile: (args) => invokeCommand('fs_read_file', { args }),
    writeFile: (args) => invokeCommand('fs_write_file', { args }),
    createFile: (args) => invokeCommand('fs_create_file', { args }),
    createDir: (args) => invokeCommand('fs_create_dir', { args }),
    rename: (args) => invokeCommand('fs_rename', { args }),
    copy: (args) => invokeCommand('fs_copy', { args }),
    deletePath: (args) => invokeCommand('fs_delete_path', { args }),
    stat: (args) => invokeCommand('fs_stat', { args }),
    pathExists: (args) => invokeCommand('fs_path_exists', { args }),
    pathsExist: (args) => invokeCommand('fs_paths_exist', { args }),
    listFiles: (args) => invokeCommand('fs_list_files', { args }),
    cancelListFiles: (args) => invokeCommand('fs_cancel_list_files', { args }),
    search: (args) => invokeCommand('fs_search', { args }),
    watchWorktree: (args) => invokeCommand('fs_watch_worktree', { args }),
    unwatchWorktree: (args) => invokeCommand('fs_unwatch_worktree', { args }),
    listMarkdownDocuments: (args) => invokeCommand('fs_list_markdown_documents', { args }),
    authorizeExternalPath: (args) => invokeCommand('fs_authorize_external_path', { args }),
    onFsChanged: (callback) => subscribeToEvent('fs:changed', callback),
    // Why: the renderer subscribes unconditionally, but A has no log-tail
    // surface yet, so the subscription must still hand back an unsubscriber.
    onLocalLogTailChanged: () => () => {}
  })
}
