import type {
  RuntimeTerminalAgentStatus,
  RuntimeTerminalAgentStatusState,
  RuntimeTerminalListResult,
  RuntimeTerminalSend,
  RuntimeTerminalSummary,
  RuntimeTerminalWait,
  RuntimeTerminalWaitCondition
} from '../../../shared/runtime-types'
import type { AgentStatusState } from '../../../shared/agent-status-types'
import { AGENT_STATUS_STALE_AFTER_MS } from '../../../shared/agent-status-types'
import type { AppState } from '@/store/types'
import { getIndexedWorktreeById } from '@/store/worktree-repo-index'
import {
  classifyTitleActivity,
  isExplicitAgentStatusFresh,
  resolveTitleActivityLabel
} from '@/lib/pane-agent-evidence'
import { resolveRuntimePaneTitleForLeaf } from '@/lib/runtime-pane-title-leaf-id'
import { RuntimeRpcCallError } from './runtime-rpc-result'

const LOCAL_TERMINAL_RPC_METHODS = [
  'terminal.list',
  'terminal.agentStatus',
  'terminal.isRunningAgent',
  'terminal.wait',
  'terminal.send'
] as const

const TERMINAL_LIST_DEFAULT_LIMIT = 200

const WAIT_POLL_MS = 250
const WAIT_OUTPUT_QUIET_MS = 1500
const WAIT_TIMEOUT_DEFAULT_MS = 15_000
const WAIT_TIMEOUT_MAX_MS = 60_000

type StoreModule = typeof import('@/store')
let storeModulePromise: Promise<StoreModule> | null = null
function loadStoreModule(): Promise<StoreModule> {
  storeModulePromise ??= import('@/store')
  return storeModulePromise
}

type PtyDataModule = typeof import('@/components/terminal-pane/pty-data-sidecar-subscriptions')
let ptyDataModulePromise: Promise<PtyDataModule> | null = null
function loadPtyDataModule(): Promise<PtyDataModule> {
  ptyDataModulePromise ??= import('@/components/terminal-pane/pty-data-sidecar-subscriptions')
  return ptyDataModulePromise
}

export type LocalTerminalLocation = {
  ptyId: string
  tabId: string
  leafId: string
  worktreeId: string
}

export function isLocalTerminalRpcMethod(method: string): boolean {
  return (LOCAL_TERMINAL_RPC_METHODS as readonly string[]).includes(method)
}

export function localTerminalFailure(code: string, message: string): RuntimeRpcCallError {
  return new RuntimeRpcCallError({ id: 'local', ok: false, error: { code, message } })
}

export async function callLocalTerminalRpc<TResult>(
  method: string,
  params: unknown
): Promise<TResult> {
  switch (method) {
    case 'terminal.list':
      return (await listLocalTerminals(params)) as TResult
    case 'terminal.agentStatus':
      return (await getLocalAgentStatus(params)) as TResult
    case 'terminal.isRunningAgent': {
      const { agentStatus } = await getLocalAgentStatus(params)
      return { isRunningAgent: agentStatus.isRunningAgent } as TResult
    }
    case 'terminal.wait':
      return (await waitLocalTerminal(params)) as TResult
    case 'terminal.send':
      return (await sendLocalTerminal(params)) as TResult
    default:
      throw localTerminalFailure(
        'method_not_found',
        `Unsupported local terminal method: ${method}`
      )
  }
}

export function readTerminalHandle(params: unknown): string {
  const handle = (params as { terminal?: unknown } | null | undefined)?.terminal
  if (typeof handle !== 'string' || handle.length === 0) {
    throw localTerminalFailure('terminal_handle_stale', 'A terminal handle is required.')
  }
  return handle
}

export function findWorktreeIdForTab(state: AppState, tabId: string): string | null {
  for (const [worktreeId, tabs] of Object.entries(state.tabsByWorktree ?? {})) {
    if (tabs?.some((tab) => tab.id === tabId)) {
      return worktreeId
    }
  }
  return null
}

export function findLeafIdForPty(
  layout: AppState['terminalLayoutsByTabId'][string] | undefined,
  ptyId: string
): string {
  const byLeaf = layout?.ptyIdsByLeafId
  if (byLeaf) {
    for (const [leafId, candidate] of Object.entries(byLeaf)) {
      if (candidate === ptyId) {
        return leafId
      }
    }
  }
  return layout?.activeLeafId ?? ''
}

export function collectLocalTerminalLocations(state: AppState): LocalTerminalLocation[] {
  const locations: LocalTerminalLocation[] = []
  for (const [tabId, ptyIds] of Object.entries(state.ptyIdsByTabId ?? {})) {
    const worktreeId = findWorktreeIdForTab(state, tabId)
    if (!worktreeId) {
      continue
    }
    const layout = state.terminalLayoutsByTabId?.[tabId]
    for (const ptyId of ptyIds ?? []) {
      locations.push({ ptyId, tabId, leafId: findLeafIdForPty(layout, ptyId), worktreeId })
    }
  }
  return locations
}

export function findLocalTerminalLocation(
  state: AppState,
  ptyId: string
): LocalTerminalLocation | null {
  return collectLocalTerminalLocations(state).find((location) => location.ptyId === ptyId) ?? null
}

export async function isPtyLive(ptyId: string): Promise<boolean> {
  const sessions = await window.api.pty.listSessions({ connectionId: null })
  return sessions.some((session) => session.id === ptyId)
}

export function readPaneTitle(state: AppState, location: LocalTerminalLocation): string | null {
  const resolved = resolveRuntimePaneTitleForLeaf(
    state.terminalLayoutsByTabId?.[location.tabId],
    state.runtimePaneTitlesByTabId?.[location.tabId],
    location.leafId
  )
  if (resolved) {
    return resolved
  }
  const tabs = state.tabsByWorktree?.[location.worktreeId]
  const tab = tabs?.find((entry) => entry.id === location.tabId)
  return tab?.title ?? null
}

export function mapAgentStatusState(state: AgentStatusState): RuntimeTerminalAgentStatusState {
  switch (state) {
    case 'working':
      return 'working'
    case 'blocked':
      return 'permission'
    case 'waiting':
    case 'done':
      return 'idle'
  }
}

export function readLocalAgentStatus(
  state: AppState,
  location: LocalTerminalLocation
): RuntimeTerminalAgentStatusState {
  const entry = state.agentStatusByPaneKey?.[`${location.tabId}:${location.leafId}`]
  if (entry && isExplicitAgentStatusFresh(entry, Date.now(), AGENT_STATUS_STALE_AFTER_MS)) {
    return mapAgentStatusState(entry.state)
  }
  return null
}

export function hasAgentTitleEvidence(state: AppState, location: LocalTerminalLocation): boolean {
  const title = readPaneTitle(state, location)
  return title !== null && classifyTitleActivity(title) !== null && resolveTitleActivityLabel(title) !== null
}

export function hasIdleTitleEvidence(state: AppState, location: LocalTerminalLocation): boolean {
  const title = readPaneTitle(state, location)
  return title !== null && classifyTitleActivity(title) === 'idle'
}

function readSendRefusal(
  state: AppState,
  location: LocalTerminalLocation
): 'no-agent' | 'permission' | null {
  const status = readLocalAgentStatus(state, location)
  if (status === 'permission') {
    return 'permission'
  }
  if (status !== null) {
    return null
  }
  return hasAgentTitleEvidence(state, location) ? null : 'no-agent'
}

async function sendLocalTerminal(params: unknown): Promise<{ send: RuntimeTerminalSend }> {
  const terminal = readTerminalHandle(params)
  const args = (params ?? {}) as {
    text?: unknown
    enter?: unknown
    requireAgentStatus?: unknown
  }
  const { useAppStore } = await loadStoreModule()
  const state = useAppStore.getState()
  const location = findLocalTerminalLocation(state, terminal)
  if (!location) {
    throw localTerminalFailure('terminal_handle_stale', `Unknown terminal: ${terminal}`)
  }
  if (!(await isPtyLive(terminal))) {
    throw localTerminalFailure('terminal_exited', `Terminal is not running: ${terminal}`)
  }
  if (args.requireAgentStatus === 'sendable') {
    const refusal = readSendRefusal(state, location)
    if (refusal !== null) {
      return { send: { handle: terminal, accepted: false, bytesWritten: 0, refusedReason: refusal } }
    }
  }
  const text = typeof args.text === 'string' ? args.text : ''
  let bytesWritten = 0
  if (text.length > 0) {
    const accepted = await window.api.pty.writeAccepted(terminal, text)
    if (!accepted) {
      return { send: { handle: terminal, accepted: false, bytesWritten: 0 } }
    }
    bytesWritten += text.length
  }
  if (args.enter === true) {
    const accepted = await window.api.pty.writeAccepted(terminal, '\r')
    if (!accepted) {
      return { send: { handle: terminal, accepted: false, bytesWritten } }
    }
    bytesWritten += 1
  }
  return { send: { handle: terminal, accepted: true, bytesWritten } }
}

function readWaitTimeout(value: unknown): number {
  const timeout = typeof value === 'number' && Number.isFinite(value) ? value : WAIT_TIMEOUT_DEFAULT_MS
  return Math.min(Math.max(timeout, WAIT_POLL_MS), WAIT_TIMEOUT_MAX_MS)
}

function makeWaitResult(
  handle: string,
  condition: RuntimeTerminalWaitCondition,
  fields: Pick<RuntimeTerminalWait, 'satisfied' | 'status'> &
    Partial<Pick<RuntimeTerminalWait, 'blockedReason'>>
): { wait: RuntimeTerminalWait } {
  return {
    wait: { handle, condition, exitCode: null, ...fields }
  }
}

async function waitLocalTerminal(params: unknown): Promise<{ wait: RuntimeTerminalWait }> {
  const terminal = readTerminalHandle(params)
  const args = (params ?? {}) as { for?: unknown; timeoutMs?: unknown }
  const condition: RuntimeTerminalWaitCondition = args.for === 'exit' ? 'exit' : 'tui-idle'
  const timeoutMs = readWaitTimeout(args.timeoutMs)
  const deadline = Date.now() + timeoutMs
  const { useAppStore } = await loadStoreModule()
  const state = useAppStore.getState()
  const location = findLocalTerminalLocation(state, terminal)
  if (!location) {
    throw localTerminalFailure('terminal_handle_stale', `Unknown terminal: ${terminal}`)
  }
  let lastOutputAt = Date.now()
  const unsubscribe =
    condition === 'tui-idle'
      ? (await loadPtyDataModule()).subscribeToPtyData(terminal, () => (lastOutputAt = Date.now()))
      : () => {}
  try {
    for (;;) {
      if (!(await isPtyLive(terminal))) {
        return makeWaitResult(terminal, condition, {
          satisfied: condition === 'exit',
          status: 'exited'
        })
      }
      if (condition === 'tui-idle') {
        const liveState = useAppStore.getState()
        const status = readLocalAgentStatus(liveState, location)
        if (status === 'permission') {
          return makeWaitResult(terminal, condition, {
            satisfied: false,
            status: 'running',
            blockedReason: 'agent-approval-prompt'
          })
        }
        if (status === 'idle') {
          return makeWaitResult(terminal, condition, { satisfied: true, status: 'running' })
        }
        if (
          status === null &&
          (hasIdleTitleEvidence(liveState, location) ||
            Date.now() - lastOutputAt >= WAIT_OUTPUT_QUIET_MS)
        ) {
          return makeWaitResult(terminal, condition, { satisfied: true, status: 'running' })
        }
      }
      if (Date.now() >= deadline) {
        return makeWaitResult(terminal, condition, { satisfied: false, status: 'running' })
      }
      await new Promise<void>((resolve) => window.setTimeout(resolve, WAIT_POLL_MS))
    }
  } finally {
    unsubscribe()
  }
}

async function getLocalAgentStatus(params: unknown): Promise<{ agentStatus: RuntimeTerminalAgentStatus }> {
  const terminal = readTerminalHandle(params)
  const { useAppStore } = await loadStoreModule()
  const state = useAppStore.getState()
  const location = findLocalTerminalLocation(state, terminal)
  if (!location) {
    throw localTerminalFailure('terminal_handle_stale', `Unknown terminal: ${terminal}`)
  }
  const hookStatus = readLocalAgentStatus(state, location)
  if (hookStatus !== null) {
    return { agentStatus: { handle: terminal, isRunningAgent: true, status: hookStatus } }
  }
  const titleEvidence = hasAgentTitleEvidence(state, location)
  return {
    agentStatus: { handle: terminal, isRunningAgent: titleEvidence, status: titleEvidence ? 'idle' : null }
  }
}

function readWorktreeSelectorFilter(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }
  const trimmed = value.trim()
  if (!trimmed) {
    return null
  }
  return trimmed.startsWith('id:') ? trimmed.slice(3) : trimmed
}

function readListLimit(value: unknown): number {
  return typeof value === 'number' && Number.isInteger(value) && value > 0
    ? value
    : TERMINAL_LIST_DEFAULT_LIMIT
}

async function listLocalTerminals(params: unknown): Promise<RuntimeTerminalListResult> {
  const args = (params ?? {}) as { worktree?: unknown; limit?: unknown }
  const { useAppStore } = await loadStoreModule()
  const state = useAppStore.getState()
  const worktreeFilter = readWorktreeSelectorFilter(args.worktree)
  const limit = readListLimit(args.limit)
  const sessions = await window.api.pty.listSessions({ connectionId: null })
  const liveIds = new Set(sessions.map((session) => session.id))
  const terminals: RuntimeTerminalSummary[] = []
  for (const location of collectLocalTerminalLocations(state)) {
    if (worktreeFilter !== null && location.worktreeId !== worktreeFilter) {
      continue
    }
    if (!liveIds.has(location.ptyId)) {
      continue
    }
    const worktree = getIndexedWorktreeById(state.worktreesByRepo ?? {}, location.worktreeId)
    terminals.push({
      handle: location.ptyId,
      ptyId: location.ptyId,
      worktreeId: location.worktreeId,
      worktreePath: worktree?.path ?? '',
      branch: worktree?.branch ?? '',
      tabId: location.tabId,
      leafId: location.leafId,
      title: readPaneTitle(state, location),
      connected: true,
      writable: true,
      lastOutputAt: null,
      preview: ''
    })
  }
  return {
    terminals: terminals.slice(0, limit),
    totalCount: terminals.length,
    truncated: terminals.length > limit
  }
}
