import { capOpenCodeHookText } from './listener-limits'
import type { HookListenerState } from './listener-state'
import type { ToolSnapshot } from './listener-event'

// ─── Per-pane field caches + extractors ─────────────────────────────

export type ExtractedPromptText = {
  text: string
  source:
    | 'prompt'
    | 'user_prompt'
    | 'userPrompt'
    | 'initial_prompt'
    | 'initialPrompt'
    | 'user_message'
    | 'message'
    | 'role_user_text'
    | null
}

// Joins text of an Anthropic-style content-block array; returns '' when nothing textual so callers fall through to the next prompt source.
export function contentBlockArrayText(value: unknown[]): string {
  const parts: string[] = []
  for (const item of value) {
    if (typeof item === 'string') {
      parts.push(item)
      continue
    }
    if (item && typeof item === 'object') {
      const text = (item as Record<string, unknown>).text
      if (typeof text === 'string') {
        parts.push(text)
      }
    }
  }
  return parts.join(' ').replace(/\s+/g, ' ').trim()
}

export function extractPromptText(hookPayload: Record<string, unknown>): ExtractedPromptText {
  const candidateKeys = [
    'prompt',
    'user_prompt',
    'userPrompt',
    'initial_prompt',
    'initialPrompt',
    'user_message',
    'message'
  ]
  for (const key of candidateKeys) {
    const value = hookPayload[key]
    if (typeof value === 'string' && value.trim().length > 0) {
      // Why: trim so prompts match readStringField output — whitespace would otherwise leak into UI and caches.
      return { text: value.trim(), source: key as Exclude<ExtractedPromptText['source'], null> }
    }
    // Why: Kimi sends `prompt` as a content-block array, not a string; extract it for real prompt keys but skip `message` (ambiguous status field).
    if (key !== 'message' && Array.isArray(value)) {
      const text = contentBlockArrayText(value)
      if (text.length > 0) {
        return { text, source: key as Exclude<ExtractedPromptText['source'], null> }
      }
    }
  }
  // Why: OpenCode sends MessagePart { role, text } with no UserPromptSubmit; when role === 'user' the text is the prompt.
  if (hookPayload.role === 'user' && typeof hookPayload.text === 'string') {
    const trimmed = capOpenCodeHookText(hookPayload.text.trim())
    if (trimmed.length > 0) {
      return { text: trimmed, source: 'role_user_text' }
    }
  }
  return { text: '', source: null }
}

export function stripGrokUserQueryWrapper(promptText: string): string {
  const opener = '<user_query>'
  if (!promptText.startsWith(opener)) {
    return promptText
  }
  const closer = '</user_query>'
  const wrappedText = promptText.slice(opener.length)
  const text = wrappedText.endsWith(closer) ? wrappedText.slice(0, -closer.length) : wrappedText
  // Why: Grok wraps the submitted prompt in a `<user_query>` envelope; the status cache should hold the plain user text.
  return text.trim()
}

// Why: the post-compact continuation prompt has no matching Stop and would resurrect working.
export function shouldIgnoreCompactContinuationUserPromptSubmit(
  eventName: unknown,
  promptText: string
): boolean {
  return eventName === 'UserPromptSubmit' && isCompactContinuationUserTurnText(promptText)
}

export function resolvePrompt(
  state: HookListenerState,
  paneKey: string,
  promptText: string,
  options?: { resetOnNewTurn?: boolean }
): string {
  // Why: harness-injected turns fire UserPromptSubmit but aren't the user's ask — keep cached prompt; match only known tags so real <tags> still reset the turn.
  if (isKnownHarnessInjectedUserTurnText(promptText)) {
    return state.lastPromptByPaneKey.get(paneKey) ?? ''
  }
  if (options?.resetOnNewTurn) {
    state.lastPromptByPaneKey.delete(paneKey)
  }
  if (promptText) {
    state.lastPromptByPaneKey.set(paneKey, promptText)
    return promptText
  }
  return state.lastPromptByPaneKey.get(paneKey) ?? ''
}

export function resolveToolState(
  state: HookListenerState,
  paneKey: string,
  update: ToolSnapshot,
  options: { resetOnNewTurn: boolean }
): ToolSnapshot {
  if (options.resetOnNewTurn) {
    state.lastToolByPaneKey.delete(paneKey)
  }
  const previous = state.lastToolByPaneKey.get(paneKey) ?? {}
  // Why: undefined means either "no update" or "input not previewable"; extractor metadata decides whether to inherit stale input.
  const clearsUnpreviewableInput =
    update.hasToolInputField === true && update.toolInput === undefined
  const clearsUnidentifiedTool =
    update.hasToolUpdate === true &&
    update.toolName === undefined &&
    update.hasToolInputField === true
  const toolName = clearsUnidentifiedTool ? undefined : (update.toolName ?? previous.toolName)
  const toolInput =
    clearsUnpreviewableInput ||
    (update.toolName !== undefined &&
      update.toolName !== previous.toolName &&
      update.toolInput === undefined)
      ? undefined
      : (update.toolInput ?? previous.toolInput)
  const merged: ToolSnapshot = {
    toolName,
    toolInput,
    // Why: don't inherit previous.interactivePrompt — valid only for its one AskUserQuestion event; carrying it forward leaves a stale live card.
    interactivePrompt: update.interactivePrompt,
    lastAssistantMessage: update.clearLastAssistantMessage
      ? undefined
      : (update.lastAssistantMessage ?? previous.lastAssistantMessage),
    // Why: the provenance flag has to move with the value it describes — inherit it
    // only when the message itself is inherited, or a later prose turn keeps the
    // previous tool result's flag and stays suppressed in native chat.
    lastAssistantMessageIsToolOutput: update.clearLastAssistantMessage
      ? undefined
      : update.lastAssistantMessage === undefined
        ? previous.lastAssistantMessageIsToolOutput
        : update.lastAssistantMessageIsToolOutput
  }
  state.lastToolByPaneKey.set(paneKey, merged)
  return merged
}

// ─── Harness-injected user-turn classification ─────────────────────
// Why: agent harnesses (Claude Code and its forks) inject machinery into the
// conversation as user-role turns — background task notifications, system
// reminders, inter-agent messages, slash-command envelopes, local-command
// output, interruption and compaction notices. These fire user-prompt hooks
// and land in transcripts, but they are not something the user typed, so
// prompt-derived UI must not surface them.
//
// We match only tags we have observed from harnesses, never a broad kebab
// shape: a real prompt starting with a custom `<my-element>` or a Grok
// `<user_query>` envelope is a genuine user turn, and misclassifying it would
// hide the turn (drop it from transcripts, demote its session title, or leave
// the agent visibly done after an interrupt).
const LEADING_TAG_NAME = /^<([a-z][a-z0-9-]*)(?:[\s>]|$)/

// Consumers must only treat tags we have observed from harnesses as machinery;
// arbitrary kebab tags can be genuine user code.
const KNOWN_HARNESS_TAG_NAMES = new Set([
  'agent-message',
  'bash-input',
  'bash-stderr',
  'bash-stdout',
  'command-args',
  'command-message',
  'command-name',
  'cross-session-message',
  'fork-boilerplate',
  'local-command-caveat',
  'local-command-stderr',
  'local-command-stdout',
  'mcp-polling-update',
  'mcp-resource-update',
  'system-reminder',
  'task-notification',
  'teammate-message',
  'user-memory-input',
  'user-prompt-submit-hook'
])

// Injected turns identified by a leading string rather than a known tag name:
// the harness only emits <channel> in its attributed `<channel source=…>` form
// (a bare <channel> is a real RSS/XML paste), plus prose deliveries and notices.
const COMPACT_CONTINUATION_PREFIX = 'this session is being continued from a previous conversation'
const HARNESS_INJECTED_TURN_PREFIXES = [
  '<channel source=',
  '[request interrupted',
  'a message arrived from ',
  'another claude session sent a message',
  'no response requested.',
  'caveat: the messages below were generated by the user while running local commands',
  COMPACT_CONTINUATION_PREFIX
]

// Why: classification only inspects leading tags/prefixes. Cap the toLowerCase
// copy so vault-scan / prompt-seed paths stay O(1) on multi-KB pastes.
const HARNESS_CLASSIFY_HEAD_LIMIT = 256
const HARNESS_CLASSIFY_LEADING_WS_LIMIT = 64

/** True only for observed harness shapes. Match on trimmed, lowercased text.
 *  Unknown kebab tags stay user turns — only tags we have observed count. */
export function isKnownHarnessInjectedUserTurnText(text: string): boolean {
  const normalized = normalizedHarnessTurnHead(text)
  if (!normalized) {
    return false
  }
  const tagName = LEADING_TAG_NAME.exec(normalized)?.[1]
  if (tagName && KNOWN_HARNESS_TAG_NAMES.has(tagName)) {
    return true
  }
  return HARNESS_INJECTED_TURN_PREFIXES.some((prefix) => normalized.startsWith(prefix))
}

/** True only for the observed post-compaction continuation prompt. */
export function isCompactContinuationUserTurnText(text: string): boolean {
  return normalizedHarnessTurnHead(text).startsWith(COMPACT_CONTINUATION_PREFIX)
}

function normalizedHarnessTurnHead(text: string): string {
  let start = 0
  const wsScanEnd = Math.min(text.length, HARNESS_CLASSIFY_LEADING_WS_LIMIT)
  while (start < wsScanEnd && isAsciiWhitespace(text.charCodeAt(start))) {
    start += 1
  }
  if (start >= text.length) {
    return ''
  }
  const headEnd = Math.min(text.length, start + HARNESS_CLASSIFY_HEAD_LIMIT)
  return text.slice(start, headEnd).toLowerCase()
}

function isAsciiWhitespace(code: number): boolean {
  return code === 32 || code === 9 || code === 10 || code === 13 || code === 12
}
