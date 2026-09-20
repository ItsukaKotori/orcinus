import type {
  LocalhostWorktreeLabelResult,
  LocalhostWorktreeLabelRoute
} from '../../localhost-worktree-labels'
import type {
  WorkspacePortAdvertisedUrlChangedEvent,
  WorkspacePortKillRequest,
  WorkspacePortKillResult,
  WorkspacePortScanRequest,
  WorkspacePortScanResult
} from '../../workspace-ports'

export type WorkspacePortsApi = {
  scan: (args: WorkspacePortScanRequest) => Promise<WorkspacePortScanResult>
  kill: (args: WorkspacePortKillRequest) => Promise<WorkspacePortKillResult>
  onAdvertisedUrlChanged: (
    callback: (event: WorkspacePortAdvertisedUrlChangedEvent) => void
  ) => () => void
}

export type LocalhostWorktreeLabelsApi = {
  register: (args: LocalhostWorktreeLabelRoute) => Promise<LocalhostWorktreeLabelResult>
}
