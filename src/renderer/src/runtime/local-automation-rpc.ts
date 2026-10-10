/**
 * Local automation RPC adapter.
 *
 * The renderer's automation clients speak the host-scoped wire contract even
 * against the in-process desktop host, so this adapter translates back:
 * `repo`/`workspace` machine selectors become desktop `projectId`/`workspaceId`,
 * and the fenced multi-host fields (`selector`/`expectedOwner`/`destination`)
 * are dropped — the desktop host has no second host to fence against.
 *
 * `automations_list` answers a bare `Automation[]`, so the scoped list contract
 * the page validates (`{automations, items}`) is synthesized here: the desktop
 * authority is the in-process host, so every stored row is Self.
 */

import type {
  AutomationListItem,
  AutomationListResult
} from '../../../shared/automation-list-scope'
import type { Automation } from '../../../shared/automations-types'
import { invokeCommand } from '../../../bridge/real/invoke'
import { RuntimeRpcCallError } from './runtime-rpc-result'

const LOCAL_AUTOMATION_RPC_METHODS = [
  'automation.list',
  'automation.create',
  'automation.update',
  'automation.delete',
  'automation.runNow',
  'automation.runs'
] as const

export function isLocalAutomationRpcMethod(method: string): boolean {
  return (LOCAL_AUTOMATION_RPC_METHODS as readonly string[]).includes(method)
}

export function localAutomationFailure(code: string, message: string): RuntimeRpcCallError {
  return new RuntimeRpcCallError({ id: 'local', ok: false, error: { code, message } })
}

function readParams(params: unknown): Record<string, unknown> {
  return typeof params === 'object' && params !== null
    ? { ...(params as Record<string, unknown>) }
    : {}
}

function omitKeys(
  record: Record<string, unknown>,
  keys: readonly string[]
): Record<string, unknown> {
  for (const key of keys) {
    delete record[key]
  }
  return record
}

/** `id:<value>` is the machine selector every host speaks; desktop fields carry the bare id. */
function readSelectorId(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }
  return value.startsWith('id:') ? value.slice('id:'.length) : value
}

/**
 * Renames `repo`/`workspace` back to `projectId`/`workspaceId`. An explicit but
 * empty workspace selector — a new-per-run create or a clearing update — becomes
 * `null` so the definition really loses its workspace.
 */
function toDesktopInput(input: Record<string, unknown>): Record<string, unknown> {
  const mapped = readParams(input)
  if (mapped.repo !== undefined) {
    mapped.projectId = readSelectorId(mapped.repo)
  }
  if (mapped.workspace !== undefined) {
    mapped.workspaceId = readSelectorId(mapped.workspace)
  } else if ('workspace' in mapped) {
    mapped.workspaceId = null
  }
  delete mapped.repo
  delete mapped.workspace
  return mapped
}

function toRunsArgs(params: Record<string, unknown>): Record<string, unknown> {
  const args: Record<string, unknown> = {}
  if (typeof params.automationId === 'string') {
    args.automationId = params.automationId
  }
  if (typeof params.limit === 'number') {
    args.limit = params.limit
  }
  if (typeof params.cursor === 'string') {
    args.cursor = params.cursor
  }
  return args
}

async function listLocalAutomations(): Promise<AutomationListResult> {
  const automations = await invokeCommand<Automation[]>('automations_list')
  const items: AutomationListItem[] = automations.map((automation) => ({
    automationId: automation.id,
    selector: { kind: 'self' }
  }))
  return { automations, items }
}

export async function callLocalAutomationRpc<TResult>(
  method: string,
  params: unknown
): Promise<TResult> {
  switch (method) {
    case 'automation.list':
      return (await listLocalAutomations()) as TResult
    case 'automation.create': {
      const input = omitKeys(readParams(params), ['destination'])
      const result = await invokeCommand<{ automation: Automation }>('automations_create', {
        input: toDesktopInput(input)
      })
      return result as TResult
    }
    case 'automation.update': {
      const { id, updates } = omitKeys(readParams(params), ['expectedOwner', 'destination'])
      const result = await invokeCommand<{ automation: Automation | null }>('automations_update', {
        id,
        updates: toDesktopInput(readParams(updates))
      })
      return result as TResult
    }
    case 'automation.delete': {
      const { id } = omitKeys(readParams(params), ['expectedOwner'])
      return (await invokeCommand('automations_delete', { id })) as TResult
    }
    case 'automation.runNow': {
      const { id } = omitKeys(readParams(params), ['expectedOwner'])
      return (await invokeCommand('automations_run_now', { id })) as TResult
    }
    case 'automation.runs':
      return (await invokeCommand(
        'automations_runs',
        toRunsArgs(readParams(params))
      )) as TResult
    default:
      throw localAutomationFailure(
        'method_not_found',
        `Unsupported local automation method: ${method}`
      )
  }
}
