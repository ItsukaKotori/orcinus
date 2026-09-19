import type { TuiAgent } from '../../../shared/tui-agent'
import type { AppState } from '@/store/types'
import { TUI_AGENT_CONFIG } from '../../../shared/tui-agent-config'
import { isTuiAgentEnabled, pickTuiAgent } from '../../../shared/tui-agent-selection'
import { buildDirectWorkItemAgentStartupPlan } from '@/lib/launch-work-item-direct-agent'
import { resolveSourceControlLaunchPlatform } from '@/lib/source-control-launch-platform'
import { preflightAgentTrust } from '@/lib/agent-trust-preflight'

export function buildDirectWorkItemStartup(args: {
  agent: TuiAgent | null
  agentArgs?: string | null
  draftContent: string
  promptDelivery: PromptDelivery
  settings: AppState['settings']
  launchPlatform?: NodeJS.Platform
  launchConnectionId: string | null
  worktreePath: string
  repoProjectRuntime?: Parameters<typeof resolveSourceControlLaunchPlatform>[0]['projectRuntime']
}): ReturnType<typeof buildDirectWorkItemAgentStartupPlan> {
  const launchPlatform =
    args.launchPlatform ??
    resolveSourceControlLaunchPlatform({
      connectionId: args.launchConnectionId,
      worktreePath: args.worktreePath,
      projectRuntime: args.repoProjectRuntime
    })
  return buildDirectWorkItemAgentStartupPlan({
    agent: args.agent,
    agentArgs: args.agentArgs,
    draftContent: args.draftContent,
    promptDelivery: args.promptDelivery,
    settings: args.settings,
    launchPlatform,
    // Why: SSH hosts run the plain `orca` shim, so the Linux-only `orca-ide` rename is not applied.
    isRemote: typeof args.launchConnectionId === 'string'
  })
}

type PromptDelivery = 'draft' | 'submit-after-ready'

export async function resolveDirectWorkItemAgent(args: {
  agentOverride?: TuiAgent
  launchConnectionId: string | null
  repoConnectionId: string | null
  detectedAgentsPromise: Promise<string[]> | null
  latestStore: AppState
}): Promise<{ agent: TuiAgent | null; unavailable: boolean }> {
  const detectedAgents =
    args.agentOverride !== undefined
      ? args.launchConnectionId
        ? await args.latestStore.ensureRemoteDetectedAgents(args.launchConnectionId)
        : await args.latestStore.ensureDetectedAgents()
      : args.launchConnectionId === args.repoConnectionId
        ? await args.detectedAgentsPromise!
        : args.launchConnectionId
          ? await args.latestStore.ensureRemoteDetectedAgents(args.launchConnectionId)
          : await args.latestStore.ensureDetectedAgents()
  if (args.agentOverride !== undefined) {
    return {
      agent: args.agentOverride,
      unavailable:
        !detectedAgents.includes(args.agentOverride) ||
        !isTuiAgentEnabled(args.agentOverride, args.latestStore.settings?.disabledTuiAgents)
    }
  }
  return {
    agent: pickTuiAgent(
      args.latestStore.settings?.defaultTuiAgent,
      new Set(detectedAgents.filter((agent): agent is TuiAgent => agent in TUI_AGENT_CONFIG)),
      args.latestStore.settings?.disabledTuiAgents
    ),
    unavailable: false
  }
}

/** Why: kept apart from the refusal fallback's preflight because it runs before launch. */
export async function markDirectWorkItemAgentTrusted(args: {
  agent: TuiAgent | null
  workspacePath: string
  connectionId: string | null
}): Promise<void> {
  await preflightAgentTrust({
    agent: args.agent,
    workspacePath: args.workspacePath,
    connectionId: args.connectionId
  })
}

/** Native chat removal leaves the terminal route, so no structured launch settles here. */
export async function settleDirectWorkItemStructuredLaunch(args: {
  worktreeId: string
  primaryTabId: string | null
}): Promise<{
  completed: boolean
  structuredLaunch: boolean
  visibilityUnknown: boolean
  failed: boolean
  primaryTabId: string | null
}> {
  return {
    completed: false,
    structuredLaunch: false,
    visibilityUnknown: false,
    failed: false,
    primaryTabId: args.primaryTabId
  }
}
