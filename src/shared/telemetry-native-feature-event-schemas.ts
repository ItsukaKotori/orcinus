import { z } from 'zod'
import { agentKindSchema, optInViaSchema } from './telemetry-property-schemas'

// Native chat (terminal⇄chat toggle) adoption; view-mode enum mirrors `Tab.viewMode` in shared/types.ts.
export const nativeChatViewModeSchema = z.enum(['terminal', 'chat'])
export const nativeChatToggledSchema = z
  .object({
    from_mode: nativeChatViewModeSchema,
    to_mode: nativeChatViewModeSchema,
    agent_kind: agentKindSchema
  })
  .strict()
// `runtime`: local vs SSH/remote agent PTY; `'unknown'` when unresolved at send time.
export const nativeChatRuntimeSchema = z.enum(['local', 'remote', 'unknown'])
export type NativeChatRuntime = z.infer<typeof nativeChatRuntimeSchema>
export const nativeChatMessageSentSchema = z
  .object({
    agent_kind: agentKindSchema,
    runtime: nativeChatRuntimeSchema
  })
  .strict()
export const nativeChatPickerOpenedSchema = z
  .object({ agent_kind: agentKindSchema, prefix: z.enum(['slash', 'dollar']) })
  .strict()
export const nativeChatPickerItemAcceptedSchema = z
  .object({ agent_kind: agentKindSchema, item_kind: z.enum(['command', 'skill']) })
  .strict()
export const nativeChatSendClassifiedSchema = z
  .object({ agent_kind: agentKindSchema, outcome: z.enum(['chat', 'command', 'unknown-token']) })
  .strict()
export const nativeChatSkillDiscoverySchema = z
  .object({
    agent_kind: agentKindSchema,
    outcome: z.enum(['ready', 'error', 'timeout', 'unavailable']),
    execution_host_kind: z.enum(['local', 'runtime', 'ssh'])
  })
  .strict()

export const telemetryOptedInSchema = z.object({ via: optInViaSchema }).strict()
export const telemetryOptedOutSchema = z.object({ via: optInViaSchema }).strict()
