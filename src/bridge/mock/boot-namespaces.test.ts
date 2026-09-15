import { describe, expect, it } from 'vitest'
import { createAgentAwakeApi } from './agent-awake-api'
import { createAgentStatusApi } from './agent-status-api'
import { createBrowserApi } from './browser-api'
import { createCacheApi } from './cache-api'
import { createCliApi } from './cli-api'
import { createFolderWorkspacesApi } from './folder-workspaces-api'
import { createGhApi } from './gh-api'
import { createJiraApi } from './jira-api'
import { createKeybindingsApi } from './keybindings-api'
import { createLinearApi } from './linear-api'
import { createMacosTccPromptsApi } from './macos-tcc-prompts-api'
import { createMemoryApi } from './memory-api'
import { createOnboardingApi } from './onboarding-api'
import { createOrcaProfilesApi } from './orca-profiles-api'
import { createPreflightApi } from './preflight-api'
import { createProjectGroupsApi } from './project-groups-api'
import { createProjectsApi } from './projects-api'
import { createPtyApi } from './pty-api'
import { createReposApi } from './repos-api'
import { createRuntimeApi } from './runtime-events-api'
import { createRuntimeEnvironmentsApi } from './runtime-environments-api'
import { createSkillsApi } from './skills-api'
import { createSshApi } from './ssh-api'
import { createUpdaterApi } from './updater-api'
import { createRemoteWorkspaceApi, createSessionApi } from './workspace-session-api'

describe('Phase 0 boot namespace mocks', () => {
  it('session: startup hydration read and persistence patch resolve', async () => {
    const session = createSessionApi()
    await expect(session.get()).resolves.toBeDefined()
    await expect(session.patch({})).resolves.toBeUndefined()
  })

  it('remoteWorkspace: client id, upload, and change subscription are benign', async () => {
    const remoteWorkspace = createRemoteWorkspaceApi()
    await expect(remoteWorkspace.clientId()).resolves.toBe('mock-client-id')
    await expect(
      remoteWorkspace.setForConnectedTargets({
        expectedRevisionsByTargetId: {},
        expectedHostObservationTokensByTargetId: {}
      })
    ).resolves.toEqual([])
    const unsubscribe = remoteWorkspace.onChanged(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })

  it('onboarding: get resolves the default state', async () => {
    const onboarding = createOnboardingApi()
    await expect(onboarding.get()).resolves.toBeDefined()
  })

  it('cli: getInstallStatus resolves a not-installed status', async () => {
    const cli = createCliApi()
    await expect(cli.getInstallStatus()).resolves.toMatchObject({ state: 'not_installed' })
  })

  it('runtimeEnvironments: list, snapshots, and status subscription are benign', async () => {
    const runtimeEnvironments = createRuntimeEnvironmentsApi()
    await expect(runtimeEnvironments.list()).resolves.toEqual([])
    await expect(runtimeEnvironments.getStatusSnapshots()).resolves.toEqual([])
    const unsubscribe = runtimeEnvironments.onStatusChanged(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })

  it('repos: list resolves empty and onChanged subscribes synchronously', async () => {
    const repos = createReposApi()
    await expect(repos.list()).resolves.toEqual([])
    const unsubscribe = repos.onChanged(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })
})

describe('Phase 0 boot query mocks', () => {
  it('agentAwake: onChanged returns an unsubscribe synchronously, not a Promise', async () => {
    const agentAwake = createAgentAwakeApi()
    const unsubscribe = agentAwake.onChanged(() => {})
    // Why pinned: the status bar calls this from an effect and invokes the result during cleanup;
    // a Promise here crashes the segment (the P0-1 WebKit failure mode).
    expect(unsubscribe).not.toBeInstanceOf(Promise)
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
    await expect(agentAwake.getStatus()).resolves.toEqual({ mode: 'off', active: false })
  })

  it('pty: listSessions resolves empty and onSpawned returns an unsubscribe', async () => {
    const pty = createPtyApi()
    await expect(pty.listSessions()).resolves.toEqual([])
    const unsubscribe = pty.onSpawned(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })

  it('agentStatus: snapshots resolve empty and drop is fire-and-forget', async () => {
    const agentStatus = createAgentStatusApi()
    await expect(agentStatus.getSnapshot()).resolves.toEqual([])
    await expect(agentStatus.getMigrationUnsupportedSnapshot()).resolves.toEqual([])
    expect(agentStatus.drop('pane-1')).toBeUndefined()
  })

  it('gh: checkOrcaStarred resolves null (web fallback) and refresh reports are handled', async () => {
    const gh = createGhApi()
    await expect(gh.checkOrcaStarred()).resolves.toBeNull()
    await expect(
      gh.reportVisiblePRRefreshCandidates({ candidates: [], generation: 0 })
    ).resolves.toBe(false)
  })

  it('runtime: status and driver queries resolve benign empty values', async () => {
    const runtime = createRuntimeApi()
    await expect(runtime.getStatus()).resolves.toMatchObject({ graphStatus: 'ready' })
    await expect(
      runtime.syncWindowGraph({ tabs: [], leaves: [], rendererGeneration: 'test' })
    ).resolves.toMatchObject({ graphStatus: 'ready' })
    await expect(runtime.getTerminalFitOverrides()).resolves.toEqual([])
    await expect(runtime.getTerminalDrivers()).resolves.toEqual([])
    await expect(runtime.getBrowserDrivers()).resolves.toEqual([])
    await expect(runtime.getBrowserRemoteViewerPages?.()).resolves.toEqual([])
    await expect(runtime.getClientHostedBrowserRows()).resolves.toEqual([])
  })

  it('catalog queries: projects, project groups, folder workspaces, skills, and ssh resolve empty', async () => {
    await expect(createProjectsApi().list()).resolves.toEqual([])
    await expect(createProjectsApi().listHostSetups()).resolves.toEqual([])
    await expect(createProjectGroupsApi().list()).resolves.toEqual([])
    await expect(createFolderWorkspacesApi().list()).resolves.toEqual([])
    await expect(createSkillsApi().discover()).resolves.toMatchObject({ skills: [], sources: [] })
    await expect(createSshApi().listTargets()).resolves.toEqual([])
    await expect(createSshApi().listRemovedTargetLabels()).resolves.toEqual({})
  })

  it('keybindings: get resolves the default (no file) snapshot', async () => {
    const snapshot = await createKeybindingsApi().get()
    expect(snapshot.exists).toBe(false)
    expect(snapshot.overrides).toEqual({})
    expect(snapshot.diagnostics).toEqual([])
  })

  it('orcaProfiles: list and authStatus report the local unconfigured profile', async () => {
    const orcaProfiles = createOrcaProfilesApi()
    const list = await orcaProfiles.list()
    expect(list.multiProfileUi).toBe(false)
    expect(list.profiles).toHaveLength(1)
    await expect(orcaProfiles.authStatus()).resolves.toMatchObject({
      state: 'unconfigured',
      configured: false
    })
  })

  it('preflight: check reports not-probed tools and refreshAgents reports no agents', async () => {
    const preflight = createPreflightApi()
    await expect(preflight.check()).resolves.toEqual({
      git: { installed: false },
      gh: { installed: false, authenticated: false }
    })
    await expect(preflight.refreshAgents()).resolves.toMatchObject({ agents: [] })
  })

  it('integrations: linear and jira statuses report disconnected', async () => {
    await expect(createLinearApi().status()).resolves.toEqual({ connected: false, viewer: null })
    await expect(createJiraApi().status()).resolves.toEqual({ connected: false, viewer: null })
  })

  it('cache: getGitHub resolves empty pr/issue maps', async () => {
    await expect(createCacheApi().getGitHub()).resolves.toEqual({ pr: {}, issue: {} })
  })

  it('memory: getSnapshot resolves a well-formed zero snapshot', async () => {
    const snapshot = await createMemoryApi().getSnapshot()
    expect(snapshot.worktrees).toEqual([])
    expect(snapshot.app.history).toEqual([])
    expect(snapshot.host.memoryUsagePercent).toBe(0)
    expect(snapshot.totalMemory).toBe(0)
    expect(typeof snapshot.collectedAt).toBe('number')
  })

  it('updater: getVersion resolves the shell version and status is idle', async () => {
    const updater = createUpdaterApi()
    await expect(updater.getVersion()).resolves.toBe('0.0.1')
    await expect(updater.getStatus()).resolves.toEqual({ state: 'idle' })
  })

  it('browser: sessionListProfiles resolves empty', async () => {
    await expect(createBrowserApi().sessionListProfiles()).resolves.toEqual([])
  })

  it('macosTccPrompts: consumePending resolves null', async () => {
    await expect(createMacosTccPromptsApi().consumePending()).resolves.toBeNull()
  })
})
