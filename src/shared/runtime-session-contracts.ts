import type { AgentStatusOrchestrationContext } from './agent-status-types'
import type { RemoteServerUpdateSupport } from './remote-server-update'
import type { RemoteRuntimeSharedConnectionDiagnostics } from './remote-runtime-shared-control-types'
import type { RuntimeHostConnectionState } from './runtime-host-connection-state'
import type { RuntimeCapability } from './protocol-version'
import type { RuntimeDegradation } from './runtime-capability-degradation'
import type { TabGroupLayoutNode } from './tab-types'
import type { TerminalPaneLayoutNode } from './terminal-tab-types'
import type {
  RuntimeMobileSessionClientTab,
  RuntimeMobileSessionSnapshotTab,
  RuntimeMobileSessionTerminalClientTab
} from './runtime-mobile-session-tab-contracts'

export type * from './runtime-mobile-session-tab-contracts'

export type RuntimeGraphStatus = 'ready' | 'reloading' | 'unavailable'

export type RuntimeDesktopWindowStatus = 'available' | 'openable' | 'initializing' | 'blocked'

export const HEADLESS_RUNTIME_WINDOW_ID = 0

export type DeviceScope = 'mobile' | 'runtime'

export type RuntimeTerminalDriverState =
  | { kind: 'idle' }
  | { kind: 'desktop' }
  | { kind: 'mobile'; clientId: string }

export type RuntimeStatus = {
  runtimeId: string
  /** Authenticated requester identity. Missing for in-process callers and older hosts. */
  pairedDeviceId?: string
  rendererGraphEpoch: number
  graphStatus: RuntimeGraphStatus
  authoritativeWindowId: number | null
  desktopWindowStatus?: RuntimeDesktopWindowStatus
  liveTabCount: number
  liveLeafCount: number
  runtimeProtocolVersion?: number
  minCompatibleRuntimeClientVersion?: number
  capabilities?: RuntimeCapability[]
  /** Optional policy for clients that negotiated worktree.create-idempotency.v1. */
  worktreeCreateIdempotency?: {
    dedupeTtlMs: number
  }
  /** True only when this Windows host can prove process creation times for PID ownership. */
  windowsProcessStartTimeAvailable?: boolean
  /**
   * Optional for mixed-version peers. Absence means the host predates structured
   * degradation reporting, not that the host proved every optional feature available.
   */
  degradations?: RuntimeDegradation[]
  appVersion?: string
  remoteUpdateSupport?: RemoteServerUpdateSupport
  remoteControl?: RemoteRuntimeSharedConnectionDiagnostics | null
  hostPlatform?: NodeJS.Platform
  terminalWindowsShell?: string | null
  deviceScope?: DeviceScope
  floatingWorkspaceEnabled?: boolean
  // COMPAT(runtimeStatusMobileAliases): added 2026-05-15 for older mobile builds.
  protocolVersion?: number
  minCompatibleMobileVersion?: number
}

export type CliRuntimeState =
  | 'not_running'
  | 'starting'
  | 'ready'
  | 'graph_not_ready'
  | 'stale_bootstrap'

export type CliStatusResult = {
  target?: { kind: 'local' } | { kind: 'environment'; environment: string }
  app: {
    running: boolean
    pid: number | null
    desktopWindowStatus?: RuntimeDesktopWindowStatus
  }
  runtime: {
    state: CliRuntimeState
    reachable: boolean
    /** Canonical runtime transport verdict, when the caller has runtime evidence. */
    connectionState?: RuntimeHostConnectionState
    runtimeId: string | null
    appVersion?: string
    remoteUpdateSupport?: RemoteServerUpdateSupport
    capabilities?: RuntimeCapability[]
    degradations?: RuntimeDegradation[]
  }
  graph: {
    state: RuntimeGraphStatus | 'not_running' | 'starting'
  }
}

export type RuntimeSyncedTab = {
  tabId: string
  worktreeId: string
  title: string | null
  activeLeafId: string | null
  layout: TerminalPaneLayoutNode | null
}

export type RuntimeSyncedLeaf = {
  tabId: string
  worktreeId: string
  leafId: string
  paneRuntimeId: number
  ptyId: string | null
  paneTitle?: string | null
  title?: string | null
  /** True when this leaf is retained by a parked PTY watcher, not mounted in the renderer. */
  parked?: boolean
}

export type RuntimeSyncWindowGraph = {
  tabs: RuntimeSyncedTab[]
  leaves: RuntimeSyncedLeaf[]
  mobileSessionTabs?: RuntimeMobileSessionTabsSnapshot[]
  unchangedMobileSessionWorktrees?: string[]
}

export type RuntimeRendererSyncWindowGraph = RuntimeSyncWindowGraph & {
  rendererGeneration: string
}

export type RuntimeSyncWindowGraphResult = RuntimeStatus & {
  agentOrchestrationByPaneKey?: Record<string, AgentStatusOrchestrationContext>
  mobileSessionResyncWorktrees?: string[]
}

export type RuntimeMobileSessionTabGroup = {
  id: string
  activeTabId: string | null
  tabOrder: string[]
  recentTabIds?: string[]
}

type RuntimeMobileSessionTabMoveBase = {
  tabId: string
  targetGroupId: string
}

export type RuntimeMobileSessionTabMove =
  | (RuntimeMobileSessionTabMoveBase & { kind: 'reorder'; tabOrder: string[] })
  | (RuntimeMobileSessionTabMoveBase & { kind: 'move-to-group'; index?: number })
  | (RuntimeMobileSessionTabMoveBase & {
      kind: 'split'
      splitDirection: 'left' | 'right' | 'up' | 'down'
    })

export type RuntimeMobileSessionTabMoveResult = { moved: true }

export type RuntimeMobileSessionTabCloseResult = {
  closed: true
  refused?: true
  refusalReason?:
    | 'missing-intent'
    | 'stale-publication'
    | 'stale-terminal'
    | 'live-host-pty'
    | 'unknown-liveness'
    | 'retirement-owner'
  snapshotRepublished?: true
}

export type RuntimeSessionTabCloseReason = 'user' | 'pty-exit' | 'cleanup'

/**
 * The publication epoch a runtime answers with for a worktree it has published nothing for yet —
 * the state every worktree is in for a moment after the host process restarts.
 *
 * Paired with `snapshotVersion: 0` it marks a synthesized placeholder, not a host answer: the
 * runtime is saying "ask me later", not "those tabs are gone". Clients must not read absence from
 * such a frame as evidence a tab was closed.
 */
export const UNPUBLISHED_WORKTREE_PUBLICATION_EPOCH = 'none'

export type RuntimeMobileSessionTabsSnapshot = {
  worktree: string
  /** Immutable catalog identity used to fence snapshots across path reuse. */
  worktreeInstanceId?: string
  publicationEpoch: string
  snapshotVersion: number
  activeGroupId: string | null
  activeTabId: string | null
  activeTabType: 'terminal' | 'markdown' | 'file' | 'agent-session' | null
  tabGroups?: RuntimeMobileSessionTabGroup[]
  tabGroupLayout?: TabGroupLayoutNode | null
  retiredTerminalSurfaces?: RuntimeMobileSessionRetiredTerminalSurface[]
  tabs: RuntimeMobileSessionSnapshotTab[]
}

export type RuntimeMobileSessionRetiredTerminalSurface = {
  parentTabId: string
  leafId: string
  ptyId: string
  terminal: string
  incarnationId?: string
}

export type RuntimeMobileSessionTabsResult = {
  worktree: string
  publicationEpoch: string
  snapshotVersion: number
  navigationIntent?: 'follow'
  activeGroupId: string | null
  activeTabId: string | null
  activeTabType: 'terminal' | 'markdown' | 'file' | 'agent-session' | null
  tabGroups?: RuntimeMobileSessionTabGroup[]
  tabGroupLayout?: TabGroupLayoutNode | null
  retiredTerminalSurfaces?: RuntimeMobileSessionRetiredTerminalSurface[]
  tabs: RuntimeMobileSessionClientTab[]
  /**
   * Set while a freshly started runtime has not yet taken back the client-hosted pages its paired
   * hosts are still holding. Such a snapshot is authoritative about terminals, which it rehydrated
   * from disk, but silently empty of browser rows it has simply not heard about yet — so a client
   * must not read the absence of its own client-hosted rows here as "the host closed them".
   *
   * Always bounded: the runtime clears it once a host attaches, and drops it on a deadline so a
   * host that never returns cannot hold rows open forever.
   */
  clientHostedPagesUnreconciled?: true
}

export type RuntimeMobileSessionCreateTerminalResult = {
  tab: RuntimeMobileSessionTerminalClientTab
  publicationEpoch: string
  snapshotVersion: number
}

export type RuntimeMobileSessionTabsRemovedResult = RuntimeMobileSessionTabsResult & {
  removed: true
  activeGroupId: null
  activeTabId: null
  activeTabType: null
  tabs: []
}
