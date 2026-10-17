import type { AiVaultDeleteSessionArgs } from '../../shared/ai-vault-session-deletion'
import type { AiVaultPrepareSessionResumeArgs } from '../../shared/ai-vault-resume-preparation'
import type { AiVaultSessionTitlesArgs } from '../../shared/ai-vault-session-title'
import type { AiVaultListArgs } from '../../shared/ai-vault-types'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

/** The five `ai_vault_*` commands all take one camelCase `args` struct. */
type AiVaultListCommandArgs = {
  limit?: number
  force?: boolean
  scopePaths?: readonly string[]
  unlimited?: boolean
  requestToken?: string
}

/**
 * `executionHostScope` routes the Electron contract between hosts; the local
 * Tauri command has no such field, so it is stripped rather than shipped as an
 * unknown key. Absent optionals are omitted, never materialized as null.
 */
function toListCommandArgs(args?: AiVaultListArgs): AiVaultListCommandArgs | undefined {
  if (args === undefined) {
    return undefined
  }
  const commandArgs: AiVaultListCommandArgs = {}
  if (args.limit !== undefined) commandArgs.limit = args.limit
  if (args.force !== undefined) commandArgs.force = args.force
  if (args.scopePaths !== undefined) commandArgs.scopePaths = args.scopePaths
  if (args.unlimited !== undefined) commandArgs.unlimited = args.unlimited
  if (args.requestToken !== undefined) commandArgs.requestToken = args.requestToken
  return commandArgs
}

/**
 * Real `aiVault` adapter (spec §3.3): the five host-backed methods route to the
 * `ai_vault_*` commands, and `onWindowFocused` bridges the Tauri window focus
 * event. `resolveSessionTitles` passes through untouched because the command
 * declares (and ignores) its `executionHostScope`.
 */
export function createAiVaultRealApi(): PreloadApi['aiVault'] {
  return withMethodFallback<PreloadApi['aiVault']>('aiVault', {
    listSessions: (args) =>
      invokeCommand('ai_vault_list_sessions', { args: toListCommandArgs(args) }),
    resolveSessionTitles: (args: AiVaultSessionTitlesArgs) =>
      invokeCommand('ai_vault_resolve_session_titles', { args }),
    cancelListSessions: (args) => invokeCommand('ai_vault_cancel_list_sessions', { args }),
    prepareSessionResume: (args: AiVaultPrepareSessionResumeArgs) =>
      invokeCommand('ai_vault_prepare_session_resume', { args }),
    deleteSession: (args: AiVaultDeleteSessionArgs) =>
      invokeCommand('ai_vault_delete_session', { args }),
    onWindowFocused: (callback) => subscribeToEvent('tauri://focus', () => callback())
  })
}
