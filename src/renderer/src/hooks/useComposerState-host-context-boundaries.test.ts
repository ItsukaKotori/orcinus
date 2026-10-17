import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  getInitialAutoManagedWorkspaceName,
  isExplicitWorkspaceNameInput,
  resolveSmartGitHubCreateNames,
  resolveInitialWorkspaceRunSeed
} from './useComposerState'

function readComposerModule(fileName: string): string {
  return readFileSync(join(__dirname, 'composer-state', fileName), 'utf8')
}

const COMPOSER_SOURCE = {
  cardProps: readComposerModule('composer-card-props.ts'),
  derived: readComposerModule('derived-composer-state.ts'),
  folderSubmit: readComposerModule('folder-submit-orchestration.ts'),
  fullCreation: readComposerModule('full-creation-execution.ts'),
  fullCreationStartup: readComposerModule('full-creation-startup.ts'),
  fullSubmitOrchestration: readComposerModule('full-submit-orchestration.ts'),
  fullSubmitPreparation: readComposerModule('full-submit-preparation.ts'),
  fullSubmitSourcePreparation: readComposerModule('full-submit-source-preparation.ts'),
  hostRuntime: readComposerModule('host-runtime-effects.ts'),
  initialTarget: readComposerModule('initial-target-state.ts'),
  issueSource: readComposerModule('issue-source-actions.ts'),
  linkedItemLookup: readComposerModule('linked-item-lookup-effects.ts'),
  navigation: readComposerModule('composer-navigation-actions.ts'),
  projectTarget: readComposerModule('project-target-actions.ts'),
  providerRuntime: readComposerModule('provider-runtime-sync.ts'),
  quickCreation: readComposerModule('quick-creation-execution.ts'),
  quickSubmitAction: readComposerModule('quick-submit-action.ts'),
  quickSubmitPreparation: readComposerModule('quick-submit-preparation.ts'),
  quickSubmitSourcePreparation: readComposerModule('quick-submit-source-preparation.ts'),
  runtimeTarget: readComposerModule('runtime-target-selection.ts'),
  sourceContext: readComposerModule('source-context-state.ts'),
  sourceIdentity: readComposerModule('source-identity-actions.ts'),
  targetChange: readComposerModule('target-change-actions.ts'),
  quickStartup: readComposerModule('quick-startup-plan.ts'),
  workspaceIdentity: readComposerModule('workspace-identity-state.ts')
} as const
const RECIPE_OPTIONS_SOURCE = readFileSync(
  join(__dirname, 'useEphemeralVmRecipeOptions.ts'),
  'utf8'
)

function sourceBetween(source: string, startPattern: string, endPattern: string): string {
  const start = source.indexOf(startPattern)
  expect(start).toBeGreaterThanOrEqual(0)
  const end = source.indexOf(endPattern, start + startPattern.length)
  expect(end).toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('useComposerState host-context boundaries', () => {
  it('treats typed workspace names as user-authored, not auto-managed', () => {
    expect(isExplicitWorkspaceNameInput({ name: 'keep-my-name', lastAutoName: '' })).toBe(true)
    expect(
      isExplicitWorkspaceNameInput({
        name: 'keep-my-name',
        lastAutoName: 'keep-my-name'
      })
    ).toBe(false)
    expect(isExplicitWorkspaceNameInput({ name: '#1234', lastAutoName: '' })).toBe(false)
    expect(
      isExplicitWorkspaceNameInput({
        name: 'https://github.com/stablyai/orca/pull/1234',
        lastAutoName: ''
      })
    ).toBe(false)
  })

  it('does not auto-own arbitrary prefilled names', () => {
    expect(
      getInitialAutoManagedWorkspaceName({
        initialName: 'keep-my-name',
        initialLinkedWorkItem: null
      })
    ).toBe('')
  })

  it('preserves explicit names when a linked PR start point resolves at submit time', () => {
    expect(
      resolveSmartGitHubCreateNames({
        resolutionKind: 'pr-start-point',
        smartWorkspaceName: 'title-derived-name',
        smartDisplayName: 'Title derived name',
        fallbackWorkspaceName: 'edited workspace',
        nameIsAutoManaged: false
      })
    ).toEqual({ workspaceName: 'edited workspace', displayName: undefined })
  })

  it('keeps smart GitHub names for auto-managed PR start-point submissions', () => {
    expect(
      resolveSmartGitHubCreateNames({
        resolutionKind: 'pr-start-point',
        smartWorkspaceName: 'title-derived-name',
        smartDisplayName: 'Title derived name',
        fallbackWorkspaceName: 'https://github.com/stablyai/orca/pull/6772',
        nameIsAutoManaged: true
      })
    ).toEqual({ workspaceName: 'title-derived-name', displayName: 'Title derived name' })
  })

  it('auto-owns linked-item generated prefilled names', () => {
    expect(
      getInitialAutoManagedWorkspaceName({
        initialName: 'fix-workspace-name',
        initialLinkedWorkItem: {
          type: 'issue',
          provider: 'github',
          number: 1234,
          title: 'Fix workspace name',
          url: 'https://github.com/stablyai/orca/issues/1234'
        }
      })
    ).toBe('fix-workspace-name')
  })



  it('clears only repo-scoped linked work items when the repo or project changes', () => {
    // Why: Linear and Jira issues are workspace-scoped context — a repo or
    // project switch must keep them attached. Jira used to be dropped because
    // this path special-cased Linear only.
    const repoChangeSection = COMPOSER_SOURCE.targetChange
    expect(repoChangeSection).toContain(
      '!shouldPreserveWorkspaceSourceOnRepoChange(linkedWorkItem)'
    )

    const folderSourceSection = COMPOSER_SOURCE.targetChange
    expect(folderSourceSection).toContain('!shouldPreserveWorkspaceSourceOnRepoChange(current)')

    // No switch path may gate the linked-item clear on a Linear-only predicate
    // again. (isLinearLinkedWorkItem itself may still appear — it drives the
    // separate Linear branch-name feature — but never the preservation decision.)
    expect(COMPOSER_SOURCE.targetChange).not.toContain('if (!preserveLinearLinkedWorkItem)')
  })

  it('does not use local SSH gates for runtime-owned folder targets', () => {
    const targetSection = COMPOSER_SOURCE.runtimeTarget
    expect(targetSection).toContain("parsedFolderTargetHost?.kind === 'runtime'")
    expect(targetSection).toContain('connectionId: folderTargetConnectionId')
    expect(COMPOSER_SOURCE.runtimeTarget).not.toContain('folderSourceConnectionId')
  })

  it('routes folder target runtime ownership through detection, path status, and create', () => {
    const targetSection = COMPOSER_SOURCE.runtimeTarget
    expect(targetSection).toContain('folderTargetRuntimeEnvironmentId')
    expect(targetSection).toContain("{ kind: 'runtime' as const")
    expect(targetSection).toContain('useFolderWorkspaceComposerPathStatus(')
    expect(targetSection).toContain('folderTargetRuntimeEnvironmentId')
    expect(targetSection).toContain('useDetectedAgents(folderTargetAgentDetectionTarget)')

    const submitSection = COMPOSER_SOURCE.folderSubmit
    expect(submitSection).toContain('isRemote: folderTargetIsRemote')
    expect(submitSection).toContain(
      "launchSource: telemetrySource === 'onboarding' ? 'onboarding' : 'new_workspace_composer'"
    )
    expect(submitSection).toContain('runtimeEnvironmentId: folderTargetRuntimeEnvironmentId')
  })

  it('detects composer agents against the repo host: SSH, then runtime, then local (#7082)', () => {
    // Why: a repo owned by a paired runtime must show the runtime's agents, not
    // the local machine's. SSH stays first priority; runtime falls through before
    // local so an SSH repo never double-detects. Regression guard for #7082.
    const selectorSection = COMPOSER_SOURCE.workspaceIdentity
    expect(selectorSection).toContain('if (isRemote) {')
    expect(selectorSection).toContain('s.remoteDetectedAgentIds[connectionId]')
    expect(selectorSection).toContain('if (runtimeEnvironmentId) {')
    expect(selectorSection).toContain('s.runtimeDetectedAgentIds[runtimeEnvironmentId]')
    expect(selectorSection).toContain('return s.detectedAgentIds')
    // SSH branch is checked before the runtime branch.
    expect(selectorSection.indexOf('if (isRemote) {')).toBeLessThan(
      selectorSection.indexOf('if (runtimeEnvironmentId) {')
    )

    expect(COMPOSER_SOURCE.workspaceIdentity).toContain(
      'const runtimeEnvironmentId = selectedRepoSettings?.activeRuntimeEnvironmentId?.trim() || null'
    )

    // Detection effect fans out to the same three hosts in the same order and
    // re-runs when the runtime environment changes.
    const detectSection = sourceBetween(
      COMPOSER_SOURCE.hostRuntime,
      'const detect = isRemote',
      '// Per-repo: load yaml hooks'
    )
    expect(detectSection).toContain('ensureRemoteDetectedAgents(connectionId!)')
    expect(detectSection).toContain('ensureRuntimeDetectedAgents(runtimeEnvironmentId)')
    expect(detectSection).toContain('ensureDetectedAgents()')
    expect(detectSection).toMatch(
      /\}, \[\s*connectionId,\s*runtimeEnvironmentId,\s*isRemote,\s*selectedRepoSshStatus,\s*disabledTuiAgents/
    )
  })

  it('seeds initial workspace run target from the task source context', () => {
    expect(
      resolveInitialWorkspaceRunSeed({
        initialTaskSourceContext: {
          projectId: 'logical-project',
          hostId: 'ssh:builder',
          projectHostSetupId: 'setup-builder'
        }
      })
    ).toEqual({
      projectId: 'logical-project',
      hostId: 'ssh:builder',
      projectHostSetupId: 'setup-builder'
    })

    expect(
      resolveInitialWorkspaceRunSeed({
        draftProjectId: 'draft-project',
        draftHostId: 'local',
        draftProjectHostSetupId: 'setup-local',
        initialTaskSourceContext: {
          projectId: 'logical-project',
          hostId: 'ssh:builder',
          projectHostSetupId: 'setup-builder'
        }
      })
    ).toEqual({
      projectId: 'draft-project',
      hostId: 'local',
      projectHostSetupId: 'setup-local'
    })

    const section = COMPOSER_SOURCE.initialTarget

    expect(section).toContain('resolveInitialWorkspaceRunSeed')
    expect(section).toContain('initialTaskSourceContext')
    expect(section).toContain('projectId: initialRunSeed.projectId')
    expect(section).toContain('hostId: initialRunSeed.hostId')
    expect(section).toContain('projectHostSetupId: initialRunSeed.projectHostSetupId')
  })

  it('saves setup startup policy before creating a workspace', () => {
    const persistSection = COMPOSER_SOURCE.providerRuntime
    expect(persistSection).toContain('setupAgentStartupPolicySaveRef.current')
    expect(persistSection).toContain('pendingSave?.repoId === currentRepo.id')
    expect(persistSection).toContain('pendingSave.policy === policy')
    expect(persistSection).toContain('await pendingSave.promise')
    expect(persistSection).toContain('continue')
    expect(COMPOSER_SOURCE.providerRuntime).toContain('setupAgentStartupPolicyDraftRef.current')

    const fullSubmit = COMPOSER_SOURCE.fullCreation
    const fullPolicySave = fullSubmit.indexOf('persistSetupAgentStartupPolicy()')
    const fullCreate = fullSubmit.indexOf('const result = await createWorktree(')
    expect(fullPolicySave).toBeGreaterThanOrEqual(0)
    expect(fullCreate).toBeGreaterThan(fullPolicySave)

    const quickSubmit = COMPOSER_SOURCE.quickCreation
    const quickPolicySave = quickSubmit.indexOf('persistSetupAgentStartupPolicy()')
    const quickCreate = quickSubmit.indexOf('const request = buildQuickCreationRequest(')
    expect(quickPolicySave).toBeGreaterThanOrEqual(0)
    expect(quickCreate).toBeGreaterThan(quickPolicySave)
  })

  it('clears branch reuse state when manually editing the branch name', () => {
    const section = COMPOSER_SOURCE.sourceIdentity

    expect(section).toContain('resolveComposerManualBranchNameChange')
    expect(section).toContain('setReuseEligibleBranch(null)')
    expect(section).toContain('setReuseSelectedBranch(false)')
    expect(section).toContain("branchAutoNameRef.current = ''")
  })

  it('forces repo-scoped source reset when returning from folder target to a repo with the same id', () => {
    const handleRepoChange = COMPOSER_SOURCE.targetChange
    expect(handleRepoChange).toContain('forceResetStartFrom?: boolean')
    expect(handleRepoChange).toContain('value === repoId && !options.forceResetStartFrom')

    const handleProjectChange = COMPOSER_SOURCE.projectTarget
    expect(handleProjectChange).toContain(
      'handleRepoChange(nextRepoId, { forceResetStartFrom: isProjectGroupTarget })'
    )
  })







  it('selects a project by its own host instead of pinning the current host', () => {
    // Regression: passing the current host as a hard `hostId` made picking a
    // project set up only on a different host a silent no-op. The current host
    // must be a preference (focusedHostScope), with a fallback to any ready host.
    const handleProjectChange = COMPOSER_SOURCE.projectTarget
    expect(handleProjectChange).toContain('focusedHostScope: preferredHostId ?? workspaceHostScope')
    expect(handleProjectChange).not.toContain('hostId: preferredHostId')
  })





  it('surfaces folder submit smart-resolution failures through create error UI', () => {
    const section = COMPOSER_SOURCE.folderSubmit
    expect(section).toContain('catch (error)')
    expect(section).toContain('const formattedError = formatWorkspaceCreateError(error)')
    expect(section).toContain('setCreateError(formattedError)')
    expect(section).toContain('toast.error(getWorkspaceCreateErrorToastMessage(formattedError))')
    expect(section).toContain('if (!folderWorkspaceCreated)')
    expect(section).toContain('setCreateError({')
  })

  it('uses submit-time smart metadata for both folder launch mode and startup content', () => {
    const section = COMPOSER_SOURCE.folderSubmit
    expect(section).toContain(
      'const submitLinkedWorkItem = smartGitHubMetadata?.linkedWorkItem ?? linkedWorkItem'
    )
    expect(section).toContain('linkedWorkItem: submitLinkedWorkItem')
  })



  it('preserves Jira linked items when switching from repo target to folder target', () => {
    const section = COMPOSER_SOURCE.projectTarget
    expect(section).toContain('!shouldPreserveWorkspaceSourceOnRepoChange(linkedWorkItem)')
  })



  it('resolves quick-create base refs through the worktree-create precedence helper', () => {
    const section = COMPOSER_SOURCE.quickSubmitPreparation
    expect(section).not.toContain('repoWorktreeBaseRef: selectedRepo.worktreeBaseRef')
    expect(section).not.toContain('getRuntimeRepoBaseRefDefault')
  })

  it('plans new workspace agent startup from the selected repo runtime', () => {
    expect(COMPOSER_SOURCE.runtimeTarget).toContain(
      'const selectedRepoAgentLaunchPlatform = useMemo'
    )
    expect(COMPOSER_SOURCE.runtimeTarget).toContain('getLocalRepoProjectExecutionRuntimeContext')
    expect(COMPOSER_SOURCE.runtimeTarget).toContain(
      'getAgentLaunchPlatformForRepo(selectedRepo, projectRuntime)'
    )

    const fullStartupPlan = sourceBetween(
      COMPOSER_SOURCE.fullSubmitPreparation,
      'const startupPlan = buildAgentStartupPlan({',
      'const shouldSeedInitialAgentStatus ='
    )
    expect(fullStartupPlan).toContain('platform: selectedRepoAgentLaunchPlatform')
    expect(fullStartupPlan).not.toContain('platform: CLIENT_PLATFORM')
    expect(COMPOSER_SOURCE.fullCreation).toContain('startupDraft: startupPlan.draftPrompt')

    const quickStartupPlan = sourceBetween(
      COMPOSER_SOURCE.quickCreation,
      'buildQuickComposerStartup({',
      'const startupPolicySettlement ='
    )
    expect(quickStartupPlan).toContain('platform: selectedRepoAgentLaunchPlatform')
    expect(quickStartupPlan).not.toContain('platform: CLIENT_PLATFORM')
  })

  // Why: activation no longer rebuilds a startup from `createdWithAgent`, so this
  // caller's own `startup` is the only thing that launches the agent it planned.
  it('passes its own startup to activation when submit planned an agent', () => {
    const activation = COMPOSER_SOURCE.fullCreation + COMPOSER_SOURCE.fullCreationStartup

    expect(activation).toContain('...(startup ? { startup } : {})')
    expect(activation).toContain('backendStartupTerminalSpawned: true')
    expect(activation).toContain('command: args.startupPlan.launchCommand')
    expect(activation).toContain('launchAgent: args.agent')
    // The removed activation-time fallback must not come back through this caller.
    expect(COMPOSER_SOURCE.fullCreation).not.toContain('buildCreatedAgentReopenStartup')
  })

  it('prepares linked quick-create drafts for the selected default agent', () => {
    const quickSubmit = COMPOSER_SOURCE.quickCreation + COMPOSER_SOURCE.quickStartup

    expect(quickSubmit).toContain(
      'const promptLinkedWorkItem = agent === null ? null : submitLinkedWorkItem'
    )
    expect(quickSubmit).toContain('resolveQuickCreateLinkedWorkItemPrompt(promptLinkedWorkItem')
    expect(quickSubmit).not.toContain('explicitAgentChoice')
    expect(quickSubmit).not.toContain('shouldPrepareQuickLinkedWorkItemAgentPrompt')
    expect(COMPOSER_SOURCE.quickCreation).not.toContain('resolveQuickWorkspaceSubmitAgent')
  })

  it('keeps sentinel-based Jira and Linear starts out of issue-command templates', () => {
    const submitSources =
      COMPOSER_SOURCE.derived +
      COMPOSER_SOURCE.fullSubmitSourcePreparation +
      COMPOSER_SOURCE.fullSubmitPreparation +
      COMPOSER_SOURCE.fullCreation +
      COMPOSER_SOURCE.quickSubmitPreparation +
      COMPOSER_SOURCE.quickCreation
    expect(submitSources).not.toContain('isOrcaCliAvailableForLaunch')
    expect(submitSources).not.toContain('hasGeneratedLinearSourceContext')
    expect(submitSources).not.toContain('shouldDraftGeneratedLinearContext')
    expect(COMPOSER_SOURCE.derived).toMatch(
      /willApplyIssueCommandAsPrompt[\s\S]*canUseIssueCommandForLinkedItemProvider\(linkedWorkItemProvider\)/
    )

    const previewSection = COMPOSER_SOURCE.derived
    expect(previewSection).toContain(
      'canUseIssueCommandForLinkedItemProvider(linkedWorkItemProvider)'
    )

    const fullSubmit =
      COMPOSER_SOURCE.fullSubmitSourcePreparation +
      COMPOSER_SOURCE.fullSubmitPreparation +
      COMPOSER_SOURCE.fullCreation +
      COMPOSER_SOURCE.fullCreationStartup
    expect(fullSubmit).toContain(
      'canUseIssueCommandForLinkedItemProvider(submitLinkedWorkItemProvider)'
    )
    expect(fullSubmit).toMatch(
      /submitShouldRunIssueAutomation[\s\S]*canUseIssueCommandForLinkedItemProvider\(submitLinkedWorkItemProvider\)/
    )
    expect(fullSubmit).toContain('prompt: submitStartupPrompt')
    expect(fullSubmit).toContain('const shouldSeedInitialAgentStatus =')
    expect(fullSubmit).toContain('...(args.shouldSeedInitialAgentStatus')

    const quickSubmit =
      COMPOSER_SOURCE.quickSubmitPreparation +
      COMPOSER_SOURCE.quickCreation +
      COMPOSER_SOURCE.quickStartup
    expect(quickSubmit).toContain('startupPlan.draftPrompt = draftPrompt')
  })



  it('gates per-workspace environment recipe discovery behind the experimental setting', () => {
    const recipeLoadSection = COMPOSER_SOURCE.runtimeTarget
    expect(recipeLoadSection).toContain('settings?.experimentalEphemeralVms === true')
    expect(recipeLoadSection).toContain('useEphemeralVmRecipeOptions')
    expect(recipeLoadSection).toContain('enabled: ephemeralVmsEnabled')
    expect(RECIPE_OPTIONS_SOURCE).toContain('args.enabled &&')
    expect(RECIPE_OPTIONS_SOURCE).toContain('window.api.ephemeralVm')
    expect(RECIPE_OPTIONS_SOURCE).toContain('window.api.plugins.onChanged')
    expect(RECIPE_OPTIONS_SOURCE).toContain('requestGeneration')

    const submitSection = COMPOSER_SOURCE.quickCreation
    expect(submitSection).toContain(
      'const activeEphemeralVmRecipeId = ephemeralVmsEnabled ? selectedEphemeralVmRecipeId : null'
    )
    expect(submitSection).toContain('recipeId: activeEphemeralVmRecipeId')

    const cardPropsSection = COMPOSER_SOURCE.cardProps
    expect(cardPropsSection).toContain('ephemeralVmRecipes:')
    expect(cardPropsSection).toContain('!ephemeralVmsEnabled')
    expect(cardPropsSection).toContain('selectedEphemeralVmRecipeId:')
    expect(cardPropsSection).toContain('ephemeralVmRecipeError:')
  })
})
