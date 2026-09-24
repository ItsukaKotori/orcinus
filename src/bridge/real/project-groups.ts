import type { ProjectGroupsApi } from '../../shared/preload-api/api/repository-api'
import type { NestedRepoScanResult } from '../../shared/project-group-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/**
 * Real `projectGroups` adapter (spec §5.2). All nine contract methods are
 * backed by commands; progress events carry the full scan snapshot, which is
 * projected down to the contract's `{ scanId, scan }` payload.
 */
export function createProjectGroupsRealApi(): ProjectGroupsApi {
  return withMethodFallback<ProjectGroupsApi>('projectGroups', {
    list: () => invokeCommand('project_groups_list'),
    create: (args) => invokeCommand('project_groups_create', { args }),
    update: (args) => invokeCommand('project_groups_update', { args }),
    delete: (args) => invokeCommand('project_groups_delete', { args }),
    moveProject: (args) => invokeCommand('project_groups_move_project', { args }),
    scanNested: (args) => invokeCommand('project_groups_scan_nested', { args }),
    cancelNestedScan: (args) => invokeCommand('project_groups_cancel_nested_scan', { args }),
    importNested: (args) => invokeCommand('project_groups_import_nested', { args }),
    onNestedScanProgress: (callback) =>
      subscribeToEvent<{ scanId: string; scan: NestedRepoScanResult }>(
        'project-groups:scan-nested-progress',
        (payload) => callback({ scanId: payload.scanId, scan: payload.scan })
      )
  })
}
