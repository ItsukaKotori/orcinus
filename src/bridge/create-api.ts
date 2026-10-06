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
import { createNotificationsApi } from './mock/notifications-api'
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
import { createAgentStatusRealApi } from './real/agent-status'
import { createAppRealApi } from './real/app'
import { createFolderWorkspacesRealApi } from './real/folder-workspaces'
import { createFsRealApi } from './real/fs'
import { createGitRealApi } from './real/git'
import { createNotificationsRealApi } from './real/notifications'
import { createOnboardingRealApi } from './real/onboarding'
import { createPlatformRealApi } from './real/platform'
import { createPreflightRealApi } from './real/preflight'
import { createProjectGroupsRealApi } from './real/project-groups'
import { createProjectsRealApi } from './real/projects'
import { createPtyRealApi } from './real/pty'
import { createReposRealApi } from './real/repos'
import { createSessionRealApi } from './real/session'
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
  | 'agentStatus'
  | 'app'
  | 'folderWorkspaces'
  | 'fs'
  | 'git'
  | 'notifications'
  | 'onboarding'
  | 'platform'
  | 'preflight'
  | 'projectGroups'
  | 'projects'
  | 'pty'
  | 'repos'
  | 'session'
  | 'settings'
  | 'ui'
  | 'worktrees'
>

/**
 * The Phase 0 mock inventory. Real mode starts from this map and replaces the
 * ported domains, so both modes share one namespace list and cannot drift.
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
    notifications: createNotificationsApi(),
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
    agentStatus: createAgentStatusRealApi(),
    app: createAppRealApi(),
    folderWorkspaces: createFolderWorkspacesRealApi(),
    fs: createFsRealApi(),
    git: createGitRealApi(),
    notifications: createNotificationsRealApi(),
    onboarding: createOnboardingRealApi(),
    platform: createPlatformRealApi(),
    preflight: createPreflightRealApi(),
    projectGroups: createProjectGroupsRealApi(),
    projects: createProjectsRealApi(),
    pty: createPtyRealApi(),
    repos: createReposRealApi(),
    // Why `.session`: PreloadApi flattens the workspace-session contract into
    // sibling domains, so the real `session` key takes the sub-surface while the
    // factory's `Pick<PreloadApi, 'session'>` wrapper stays the parity anchor.
    session: createSessionRealApi().session,
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
  // SAFETY: the ported domains plus the mock inventory cover every namespace;
  // the Proxy fallback keeps unlisted names rejecting as unimplemented.
  const partial: Partial<PreloadApi> = { ...createMockDomains(), ...createRealDomains() }
  return withUnimplementedFallback(partial)
}
