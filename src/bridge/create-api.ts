import type { PreloadApi } from '../preload/api-types'
import { createAgentAwakeApi } from './mock/agent-awake-api'
import { createHooksApi } from './mock/agent-hook-api'
import { createAgentStatusApi } from './mock/agent-status-api'
import { createAppApi } from './mock/app-api'
import { createAutomationsApi } from './mock/automations-api'
import { createBrowserApi } from './mock/browser-api'
import { createCacheApi } from './mock/cache-api'
import { createCliApi } from './mock/cli-api'
import { createDocPreviewApi } from './mock/doc-preview-api'
import { createEphemeralVmApi } from './mock/ephemeral-vm-api'
import { createFolderWorkspacesApi } from './mock/folder-workspaces-api'
import { createFsApi } from './mock/fs-api'
import { createGhApi } from './mock/gh-api'
import { createGitApi } from './mock/git-api'
import { createHostedReviewApi } from './mock/hosted-review-api'
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
import { createAppRealApi } from './real/app'
import { createFolderWorkspacesRealApi } from './real/folder-workspaces'
import { createFsRealApi } from './real/fs'
import { createPlatformRealApi } from './real/platform'
import { createProjectGroupsRealApi } from './real/project-groups'
import { createProjectsRealApi } from './real/projects'
import { createReposRealApi } from './real/repos'
import { createSettingsRealApi } from './real/settings'
import { createUiRealApi } from './real/ui'
import { createWorktreesRealApi } from './real/worktrees'
import { withUnimplementedFallback } from './unimplemented-fallback'

export type AdeApiMode = 'real' | 'mock'

export type AdeApiOptions = {
  mode?: AdeApiMode
}

type RealDomains = Pick<
  PreloadApi,
  | 'app'
  | 'folderWorkspaces'
  | 'fs'
  | 'platform'
  | 'projectGroups'
  | 'projects'
  | 'repos'
  | 'settings'
  | 'ui'
  | 'worktrees'
>

/**
 * The Phase 0 mock inventory. Real mode starts from this map and replaces the
 * ten ported domains, so both modes share one namespace list and cannot drift.
 */
function createMockDomains(): Partial<PreloadApi> {
  return {
    agentAwake: createAgentAwakeApi(),
    agentStatus: createAgentStatusApi(),
    app: createAppApi(),
    automations: createAutomationsApi(),
    browser: createBrowserApi(),
    cache: createCacheApi(),
    cli: createCliApi(),
    docPreview: createDocPreviewApi(),
    ephemeralVm: createEphemeralVmApi(),
    folderWorkspaces: createFolderWorkspacesApi(),
    fs: createFsApi(),
    gh: createGhApi(),
    git: createGitApi(),
    hooks: createHooksApi(),
    hostedReview: createHostedReviewApi(),
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
}

function createRealDomains(): RealDomains {
  return {
    app: createAppRealApi(),
    folderWorkspaces: createFolderWorkspacesRealApi(),
    fs: createFsRealApi(),
    platform: createPlatformRealApi(),
    projectGroups: createProjectGroupsRealApi(),
    projects: createProjectsRealApi(),
    repos: createReposRealApi(),
    settings: createSettingsRealApi(),
    ui: createUiRealApi(),
    worktrees: createWorktreesRealApi()
  }
}

function resolveMode(options?: AdeApiOptions): AdeApiMode {
  return options?.mode ?? (import.meta.env.VITE_ADE_BRIDGE === 'mock' ? 'mock' : 'real')
}

/** Full mock bridge for `VITE_ADE_BRIDGE=mock`, browser dev, and mock-expecting tests. */
export function createMockAdeApi(): PreloadApi {
  // SAFETY: the mock inventory plus the Proxy fallback covers every namespace, so
  // the partial behaves as a full PreloadApi at call sites.
  return withUnimplementedFallback(createMockDomains())
}

export function createAdeApi(options?: AdeApiOptions): PreloadApi {
  if (resolveMode(options) === 'mock') {
    return createMockAdeApi()
  }
  // SAFETY: the ten ported domains plus the mock inventory cover every namespace;
  // the Proxy fallback keeps unlisted names rejecting as unimplemented.
  const partial: Partial<PreloadApi> = { ...createMockDomains(), ...createRealDomains() }
  return withUnimplementedFallback(partial)
}
