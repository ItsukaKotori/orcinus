import { titleHasAgentName } from '../../../shared/agent-detection'
import type {
  AgentProviderSessionMetadata,
  SleepingAgentSessionRecord
} from '../../../shared/agent-session-resume'
import { makePaneKey } from '../../../shared/stable-pane-id'
import type { TerminalPaneLayoutNode } from '../../../shared/terminal-tab-types'
import type { AppState } from '../store'
import { findAgentPaneWorktreeId } from '../store/slices/agent-status-pane-key-tab-binding'

const CAPTURE_AGENTS = ['claude', 'codex'] as const
type CaptureAgent = (typeof CAPTURE_AGENTS)[number]

/** Renderer boot time is the widest window 2A tracks (spec §3.3). */
export const RENDERER_BOOT_MS = Date.now()

export type TranscriptCaptureDeps = {
  state: AppState
  origin: 'quit' | 'live'
  resolveCwd: (ptyId: string) => Promise<string | null>
  resolveProviderSession: (args: {
    cwd: string
    agentKind: string
    windowFromMs: number
    windowToMs: number
  }) => Promise<AgentProviderSessionMetadata | null>
  now: () => number
  mergeRecords: (records: SleepingAgentSessionRecord[]) => void
}

type CaptureTarget = {
  paneKey: string
  tabId: string
  worktreeId: string
  agent: CaptureAgent
  ptyId: string
}

function collectLeafIds(node: TerminalPaneLayoutNode | null, into: string[]): void {
  if (!node) {
    return
  }
  if (node.type === 'leaf') {
    into.push(node.leafId)
    return
  }
  collectLeafIds(node.first, into)
  collectLeafIds(node.second, into)
}

function collectCaptureTargets(state: AppState): CaptureTarget[] {
  const targets: CaptureTarget[] = []
  for (const [worktreeKey, tabs] of Object.entries(state.tabsByWorktree)) {
    for (const tab of tabs) {
      const layout = state.terminalLayoutsByTabId[tab.id]
      if (!layout) {
        continue
      }
      const leafIds: string[] = []
      collectLeafIds(layout.root, leafIds)
      for (const leafId of leafIds) {
        let paneKey: string
        try {
          paneKey = makePaneKey(tab.id, leafId)
        } catch {
          continue
        }
        if (state.sleepingAgentSessionsByPaneKey[paneKey]?.providerSession) {
          continue
        }
        const title = layout.titlesByLeafId?.[leafId] ?? tab.title ?? ''
        const agent = CAPTURE_AGENTS.find((candidate) => titleHasAgentName(title, candidate))
        if (!agent) {
          continue
        }
        const ptyId = layout.ptyIdsByLeafId?.[leafId]
        if (!ptyId) {
          continue
        }
        targets.push({
          paneKey,
          tabId: tab.id,
          worktreeId: findAgentPaneWorktreeId(state, paneKey) ?? worktreeKey,
          agent,
          ptyId
        })
      }
    }
  }
  return targets
}

/**
 * OSC-title identity + transcript-directory scan → sleeping resume records
 * (spec §5.4). Best-effort by construction: every failure mode converges on
 * "no record" = shell-only restore, never an error surface.
 */
export async function captureTranscriptAgentSessions(deps: TranscriptCaptureDeps): Promise<void> {
  const targets = collectCaptureTargets(deps.state)
  if (targets.length === 0) {
    return
  }
  const now = deps.now()
  const records: SleepingAgentSessionRecord[] = []
  for (const target of targets) {
    try {
      const cwd = await deps.resolveCwd(target.ptyId)
      if (!cwd) {
        continue
      }
      const providerSession = await deps.resolveProviderSession({
        cwd,
        agentKind: target.agent,
        windowFromMs: RENDERER_BOOT_MS,
        windowToMs: now
      })
      if (!providerSession) {
        continue
      }
      records.push({
        paneKey: target.paneKey,
        tabId: target.tabId,
        worktreeId: target.worktreeId,
        agent: target.agent,
        providerSession,
        prompt: '',
        state: 'waiting',
        capturedAt: now,
        updatedAt: now,
        origin: deps.origin
      })
    } catch {
      // Transcript capture is best-effort; absence means shell-only restore.
    }
  }
  if (records.length > 0) {
    deps.mergeRecords(records)
  }
}

/** Default IO against the bridge (spec §3.3/§3.4 contracts). */
export const defaultTranscriptCaptureIo = {
  resolveCwd: async (ptyId: string): Promise<string | null> => {
    try {
      return await window.api.pty.getCwd(ptyId)
    } catch {
      return null
    }
  },
  resolveProviderSession: (
    args: Parameters<TranscriptCaptureDeps['resolveProviderSession']>[0]
  ): Promise<AgentProviderSessionMetadata | null> =>
    window.api.preflight.resolveAgentProviderSession(args)
}
