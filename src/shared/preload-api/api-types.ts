import type {
  ClaudeAccountsApi,
  CodexAccountsApi,
  CodexConfigSyncApi,
  GrokAccountsApi,
  MinimaxCredentialsApi
} from './api/agent-account-api'
import type { HooksApi } from './api/agent-hook-api'
import type { SkillsApi } from './api/agent-skill-api'
import type { AgentAwakeApi, AgentStatusApi, AgentTrustApi } from './api/agent-status-api'
import type {
  ClaudeUsageApi,
  CodexUsageApi,
  OpenCodeUsageApi,
  RateLimitsApi
} from './api/agent-usage-api'
import type { AiVaultApi } from './api/ai-vault-api'
import type { AppApi, E2EApi, PlatformApi } from './api/app-api'
import type { CliApi } from './api/cli-install-api'
import type { TerminalPreviewApi } from './api/terminal-preview-api'
import type { EphemeralVmApi } from './api/ephemeral-vm-api'
import type { ExportApi, FilesystemApi } from './api/filesystem-api'
import type { GitInspectionApi } from './api/git-inspection-api'
import type { GitOperationApi } from './api/git-operation-api'
import type { BitbucketApi } from './api/bitbucket-api'
import type { StarNagApi } from './api/star-nag-api'
import type { OrcaProfileApi } from './api/orca-profile-api'
import type {
  DeveloperPermissionsApi,
  MacosTccPromptsApi,
  NotificationsApi
} from './api/os-permission-api'
import type { PluginsApi } from './api/plugin-host-api'
import type { PreflightApi } from './api/preflight-api'
import type { PtyApi } from './api/pty-api'
import type { ProjectGroupsApi, ProjectsApi, RepositoryApi } from './api/repository-api'
import type { RuntimeApi } from './api/runtime-api'
import type { KeybindingsApi, SettingsApi } from './api/settings-api'
import type { ShellApi } from './api/shell-api'
import type { SshApi } from './api/ssh-api'
import type { UiCommandEventApi } from './api/ui-command-event-api'
import type { UpdaterApi } from './api/updater-api'
import type { MemorySnapshot } from '../process-stats-types'
import type { UiWindowApi } from './api/ui-window-api'
import type { WorkspaceCleanupApi, WorkspaceSpaceApi } from './api/workspace-cleanup-api'
import type { WorkspaceSessionApi } from './api/workspace-session-api'
import type { FolderWorkspacesApi, SparsePresetsApi, WorktreeApi } from './api/worktree-api'

// Flattens contracts that share one PreloadApi key: an intersection is not type-identical to the flat shape.
type Merged<T> = { [K in keyof T]: T[K] }

export type StatsApi = {
  getSummary: () => Promise<{
    totalAgentsSpawned: number
    totalPRsCreated: number
    totalAgentTimeMs: number
    firstEventAt: number | null
  }>
}

export type MemoryApi = {
  getSnapshot: () => Promise<MemorySnapshot>
}

export type PreloadApi = {
  app: AppApi
  orcaProfiles: OrcaProfileApi
  platform: PlatformApi
  e2e: E2EApi
  repos: RepositoryApi
  projects: ProjectsApi
  projectGroups: ProjectGroupsApi
  folderWorkspaces: FolderWorkspacesApi
  sparsePresets: SparsePresetsApi
  worktrees: WorktreeApi
  workspaceCleanup: WorkspaceCleanupApi
  workspaceSpace: WorkspaceSpaceApi
  pty: PtyApi
  export: ExportApi
  bitbucket: BitbucketApi
  starNag: StarNagApi
  settings: SettingsApi
  agentAwake: AgentAwakeApi
  keybindings: KeybindingsApi
  codexAccounts: CodexAccountsApi
  claudeAccounts: ClaudeAccountsApi
  cli: CliApi
  codexConfigSync: CodexConfigSyncApi
  agentTrust: AgentTrustApi
  preflight: PreflightApi
  notifications: NotificationsApi
  terminalPreview: TerminalPreviewApi
  macosTccPrompts: MacosTccPromptsApi
  developerPermissions: DeveloperPermissionsApi
  shell: ShellApi
  skills: SkillsApi
  hooks: HooksApi
  ephemeralVm: EphemeralVmApi
  cache: WorkspaceSessionApi['cache']
  session: WorkspaceSessionApi['session']
  remoteWorkspace: WorkspaceSessionApi['remoteWorkspace']
  notebook: FilesystemApi['notebook']
  stats: StatsApi
  memory: MemoryApi
  claudeUsage: ClaudeUsageApi
  codexUsage: CodexUsageApi
  openCodeUsage: OpenCodeUsageApi
  aiVault: AiVaultApi
  fs: FilesystemApi['fs']
  git: Merged<GitInspectionApi & GitOperationApi>
  ui: Merged<UiCommandEventApi & UiWindowApi>
  updater: UpdaterApi
  runtime: RuntimeApi['runtime']
  runtimeEnvironments: RuntimeApi['runtimeEnvironments']
  rateLimits: RateLimitsApi
  minimaxCredentials: MinimaxCredentialsApi
  grokAccounts: GrokAccountsApi
  ssh: SshApi
  wsl: RuntimeApi['wsl']
  pwsh: RuntimeApi['pwsh']
  gitBash: RuntimeApi['gitBash']
  plugins: PluginsApi
  agentStatus: AgentStatusApi
}

export type { ClaudeUsageApi, CodexUsageApi, OpenCodeUsageApi } from './api/agent-usage-api'
export type { AiVaultApi } from './api/ai-vault-api'
export type { AppApi } from './api/app-api'
export type { ExportApi } from './api/filesystem-api'
export type {
  PluginHostInstallResult,
  PluginHostInstallSource,
  PluginHostListEntry,
  PluginHostLogLine,
  PluginHostPanel,
  PluginHostStatus,
  PluginHostTab,
  PluginMarketplaceHostInstallPreview,
  PluginMarketplaceHostListing,
  PluginMarketplaceHostSourceState,
  PluginOpenTabRequest,
  PluginProjectLockEntry,
  PluginProjectLockState,
  PluginProjectLockStatus,
  PluginRef,
  PluginScope
} from './api/plugin-host-api'
export type {
  PreflightApi,
  PreflightRuntimeContext,
  PreflightStatus,
  RefreshAgentsResult
} from './api/preflight-api'
export type {
  PtyManagementApi,
  PtyManagementMacTccAttributionHealth,
  PtyManagementSession
} from './api/pty-management-api'
export type {
  ShellOpenExternalEditorRequest,
  ShellOpenExternalEditorResult,
  ShellOpenLocalPathResult
} from './api/shell-api'

declare global {
  // oxlint-disable-next-line typescript-eslint/consistent-type-definitions -- declaration merging requires interface
  interface Window {
    api: PreloadApi
  }
}
