import type { PreloadApi } from '../preload/api-types'
import { createAgentAwakeApi } from './mock/agent-awake-api'
import { createAgentStatusApi } from './mock/agent-status-api'
import { createAppApi } from './mock/app-api'
import { createAutomationsApi } from './mock/automations-api'
import { createBrowserApi } from './mock/browser-api'
import { createCacheApi } from './mock/cache-api'
import { createCliApi } from './mock/cli-api'
import { createCrashReportsApi } from './mock/crash-reports-api'
import { createDocPreviewApi } from './mock/doc-preview-api'
import { createFolderWorkspacesApi } from './mock/folder-workspaces-api'
import { createFsApi } from './mock/fs-api'
import { createGhApi } from './mock/gh-api'
import { createJiraApi } from './mock/jira-api'
import { createKeybindingsApi } from './mock/keybindings-api'
import { createLinearApi } from './mock/linear-api'
import { createMacosTccPromptsApi } from './mock/macos-tcc-prompts-api'
import { createMemoryApi } from './mock/memory-api'
import { createOnboardingApi } from './mock/onboarding-api'
import { createOrcaProfilesApi } from './mock/orca-profiles-api'
import { createPlatformApi } from './mock/platform-api'
import { createPluginsApi } from './mock/plugins-api'
import { createPreflightApi } from './mock/preflight-api'
import { createProjectGroupsApi } from './mock/project-groups-api'
import { createProjectsApi } from './mock/projects-api'
import { createPtyApi } from './mock/pty-api'
import { createRateLimitsApi } from './mock/rate-limits-api'
import { createReposApi } from './mock/repos-api'
import { createRuntimeApi } from './mock/runtime-events-api'
import { createRuntimeEnvironmentsApi } from './mock/runtime-environments-api'
import { createSettingsApi } from './mock/settings-api'
import { createSkillsApi } from './mock/skills-api'
import { createSshApi } from './mock/ssh-api'
import { createStarNagApi } from './mock/star-nag-api'
import { createUiApi } from './mock/ui-api'
import { createUpdaterApi } from './mock/updater-api'
import { createRemoteWorkspaceApi, createSessionApi } from './mock/workspace-session-api'
import { createWorkspacePortsApi } from './mock/workspace-ports-api'
import { createWorkspaceSpaceApi } from './mock/workspace-space-api'
import { createWorktreesApi } from './mock/worktrees-api'
import { withUnimplementedFallback } from './unimplemented-fallback'

export function createAdeApi(): PreloadApi {
  // SAFETY: mock 域覆盖 Phase 0 启动与 UI 所需命名空间；其余经 Proxy 兜底为“未实现”拒绝，
  // 因此断言为 PreloadApi 在运行期仍保持“调用必为函数”的契约。
  const partial: Partial<PreloadApi> = {
    agentAwake: createAgentAwakeApi(),
    agentStatus: createAgentStatusApi(),
    app: createAppApi(),
    automations: createAutomationsApi(),
    browser: createBrowserApi(),
    cache: createCacheApi(),
    cli: createCliApi(),
    crashReports: createCrashReportsApi(),
    docPreview: createDocPreviewApi(),
    folderWorkspaces: createFolderWorkspacesApi(),
    fs: createFsApi(),
    gh: createGhApi(),
    jira: createJiraApi(),
    keybindings: createKeybindingsApi(),
    linear: createLinearApi(),
    macosTccPrompts: createMacosTccPromptsApi(),
    memory: createMemoryApi(),
    onboarding: createOnboardingApi(),
    orcaProfiles: createOrcaProfilesApi(),
    platform: createPlatformApi(),
    plugins: createPluginsApi(),
    preflight: createPreflightApi(),
    projectGroups: createProjectGroupsApi(),
    projects: createProjectsApi(),
    pty: createPtyApi(),
    rateLimits: createRateLimitsApi(),
    repos: createReposApi(),
    remoteWorkspace: createRemoteWorkspaceApi(),
    runtime: createRuntimeApi(),
    runtimeEnvironments: createRuntimeEnvironmentsApi(),
    session: createSessionApi(),
    settings: createSettingsApi(),
    skills: createSkillsApi(),
    ssh: createSshApi(),
    starNag: createStarNagApi(),
    ui: createUiApi(),
    updater: createUpdaterApi(),
    workspacePorts: createWorkspacePortsApi(),
    workspaceSpace: createWorkspaceSpaceApi(),
    worktrees: createWorktreesApi()
  }
  return withUnimplementedFallback(partial)
}
