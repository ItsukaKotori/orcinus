import type {
  RuntimeTerminalListResult,
  RuntimeTerminalSummary
} from '../../../shared/runtime-types'
import type { AppState } from '@/store/types'
import { useAppStore } from '@/store'
import { getIndexedWorktreeById } from '@/store/worktree-repo-index'
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
