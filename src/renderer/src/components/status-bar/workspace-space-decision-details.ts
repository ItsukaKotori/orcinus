import type {
  AgentStatusEntry,
  MigrationUnsupportedPtyEntry
} from '../../../../shared/agent-status-types'
import type { ExecutionHostId } from '../../../../shared/execution-host'
import type { Repo } from '../../../../shared/repo-types'
import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import type { Worktree } from '../../../../shared/worktree/types'
import type { WorkspaceSpaceWorktree } from '../../../../shared/workspace-space-types'
import { countWorkspaceSpaceActiveAgents } from './workspace-space-presentation'
import { getWorkspaceSpaceWorktreeIdentity } from './workspace-space-delete-selection'

export type WorkspaceDecisionDetails = {
  isActive: boolean
  canOpenWorkspace: boolean
  terminalTabCount: number
  liveTerminalCount: number
  activeAgentCount: number
  completedAgentCount: number
  openEditorFileCount: number
  dirtyEditorBufferCount: number
  browserTabCount: number
  changedFileCount: number | null
  branchStatus: string | null
  linearIssueLabel: string | null
}

export type WorkspaceDecisionInputs = {
  repoMap: ReadonlyMap<string, Repo>
  worktreeMap: ReadonlyMap<string, Worktree>
  repos?: readonly Repo[]
  tabsByWorktree: Record<string, readonly TerminalTab[]>
  ptyIdsByTabId: Record<string, string[]>
  agentStatusByPaneKey: Record<string, AgentStatusEntry>
  migrationUnsupportedByPtyId: Record<string, MigrationUnsupportedPtyEntry>
  runtimePaneTitlesByTabId: Record<string, Record<number, string>>
  retainedAgentsByPaneKey: Record<string, { worktreeId: string; entry: AgentStatusEntry }>
  openFiles: readonly { id: string; worktreeId: string; isDirty: boolean }[]
  editorDrafts: Record<string, string>
  gitStatusByWorktree: Record<string, readonly unknown[]>
  gitStatusByWorktreeIdentity?: ReadonlyMap<string, readonly { path: string }[]>
  remoteStatusesByWorktree: Record<string, { hasUpstream: boolean; ahead: number; behind: number }>
  settings: import('../../../../shared/global-settings-types').GlobalSettings | null
  activeWorktreeId: string | null
  activeWorkspaceExecutionHostId: ExecutionHostId | null
  now: number
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`
}

function countLiveTerminals(
  tabs: readonly TerminalTab[],
  ptyIdsByTabId: Record<string, string[]>
): number {
  return tabs.filter((tab) => (ptyIdsByTabId[tab.id]?.length ?? 0) > 0).length
}

function getBranchStatus(
  status: { hasUpstream: boolean; ahead: number; behind: number } | undefined
): string | null {
  if (!status?.hasUpstream) {
    return null
  }
  if (status.ahead === 0 && status.behind === 0) {
    return 'Synced with upstream'
  }
  const parts: string[] = []
  if (status.ahead > 0) {
    parts.push(`${status.ahead} ahead`)
  }
  if (status.behind > 0) {
    parts.push(`${status.behind} behind`)
  }
  return parts.join(', ')
}

export function getWorkspaceDecisionDetails(
  worktree: WorkspaceSpaceWorktree,
  inputs: WorkspaceDecisionInputs
): WorkspaceDecisionDetails {
  const workspaceRecord = inputs.worktreeMap.get(worktree.worktreeId)
  const tabs = inputs.tabsByWorktree[worktree.worktreeId] ?? []
  const openFiles = inputs.openFiles.filter((file) => file.worktreeId === worktree.worktreeId)
  const dirtyEditorBufferCount = openFiles.filter(
    (file) => file.isDirty || inputs.editorDrafts[file.id] !== undefined
  ).length
  const gitEntries = inputs.gitStatusByWorktreeIdentity
    ? inputs.gitStatusByWorktreeIdentity.get(getWorkspaceSpaceWorktreeIdentity(worktree))
    : inputs.gitStatusByWorktree[worktree.worktreeId]
  return {
    isActive:
      inputs.activeWorktreeId === worktree.worktreeId &&
      (inputs.activeWorkspaceExecutionHostId === null ||
        inputs.activeWorkspaceExecutionHostId === worktree.executionHostId),
    canOpenWorkspace: workspaceRecord !== undefined,
    terminalTabCount: tabs.length,
    liveTerminalCount: countLiveTerminals(tabs, inputs.ptyIdsByTabId),
    activeAgentCount: countWorkspaceSpaceActiveAgents({
      worktreeId: worktree.worktreeId,
      tabs,
      agentStatusByPaneKey: inputs.agentStatusByPaneKey,
      migrationUnsupportedByPtyId: inputs.migrationUnsupportedByPtyId,
      runtimePaneTitlesByTabId: inputs.runtimePaneTitlesByTabId,
      ptyIdsByTabId: inputs.ptyIdsByTabId,
      now: inputs.now
    }),
    completedAgentCount: Object.values(inputs.retainedAgentsByPaneKey).filter(
      (entry) => entry.worktreeId === worktree.worktreeId && entry.entry.state === 'done'
    ).length,
    openEditorFileCount: openFiles.length,
    dirtyEditorBufferCount,
    browserTabCount: 0,
    changedFileCount: gitEntries ? gitEntries.length : null,
    branchStatus: getBranchStatus(inputs.remoteStatusesByWorktree[worktree.worktreeId]),
    linearIssueLabel: workspaceRecord?.linkedLinearIssue ?? null
  }
}
