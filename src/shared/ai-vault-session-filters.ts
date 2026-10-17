// Why: this is the pure filter/group/query core for Agent Session History.
// It lives in /shared (not renderer) so the mobile package can reuse it —
// Metro only watches mobile/ + repo-root src/shared, never src/renderer.
// INVARIANT: /shared is a leaf — this module must NOT import from src/renderer.
import {
  createNormalizedPathInsideOrEqualMatcher,
  normalizeRuntimePathForComparison,
  normalizeRuntimePathSeparators
} from './cross-platform-path'
import { isClipboardTextByteLengthOverLimit } from './clipboard-text'
import { parseWslUncPath } from './wsl-paths'
import type {
  AiVaultAgent,
  AiVaultGroup,
  AiVaultScope,
  AiVaultSession,
  AiVaultSort
} from './ai-vault-types'
import {
  aiVaultAgentLabel,
  isAiVaultSessionRecoverableEmpty,
  isAiVaultSessionResumableContent
} from './ai-vault-types'
import type { ExecutionHostId } from './execution-host'
import { sessionPreviewSearchText } from './ai-vault-session-display'

// Why: the plain project descriptor is relocated here (no runtime dep) so the
// filter-state type can reference it without dragging the renderer-located
// ai-vault-session-projects runtime logic into /shared.
export type AiVaultSessionProject = {
  kind: 'repo' | 'folder' | 'unknown'
  key: string
  label: string
  projectId?: string
  repoId?: string
  hostKey?: ExecutionHostId
}

export type AiVaultSessionFilterState = {
  query: string
  agents: readonly AiVaultAgent[]
  scope: AiVaultScope
  sort: AiVaultSort
  activeWorktreePaths: readonly string[]
  activeProjectKey?: string | null
  sessionProjectById?: ReadonlyMap<string, AiVaultSessionProject>
  projectLabelByKey?: ReadonlyMap<string, string>
  hideEmptySessions: boolean
}

export type AiVaultSessionGroup = {
  key: string
  label: string
  sessions: AiVaultSession[]
}

type ParsedQuery = {
  terms: string[]
  repoTerms: string[]
  pathTerms: string[]
}

export const AI_VAULT_SESSION_FILTER_QUERY_MAX_BYTES = 2 * 1024

export function isAiVaultSessionFilterQueryTooLarge(
  query: string,
  maxBytes = AI_VAULT_SESSION_FILTER_QUERY_MAX_BYTES
): boolean {
  return isClipboardTextByteLengthOverLimit(query, maxBytes)
}

export function filterAiVaultSessions(
  sessions: readonly AiVaultSession[],
  filters: AiVaultSessionFilterState
): AiVaultSession[] {
  if (isAiVaultSessionFilterQueryTooLarge(filters.query)) {
    return []
  }

  const agentSet = new Set(filters.agents)
  const parsedQuery = parseVaultQuery(filters.query)
  const workspaceMatchers =
    filters.scope === 'workspace'
      ? filters.activeWorktreePaths.map(createAiVaultWorkspaceMatcher)
      : []

  const filtered = sessions.filter((session) => {
    if (!agentSet.has(session.agent)) {
      return false
    }
    // Hide plain empty sessions, but keep sessions with resumable content
    // (some parsers only learn turns from previews, e.g. Grok) and zero-turn
    // sessions that still carry recoverable content (queued prompts /
    // subagent transcripts) so a lost conversation is surfaced distinctly.
    if (
      filters.hideEmptySessions &&
      !isAiVaultSessionResumableContent(session) &&
      !isAiVaultSessionRecoverableEmpty(session)
    ) {
      return false
    }
    if (filters.scope === 'workspace') {
      const cwd = session.cwd
      const normalizedCwd = cwd ? normalizeRuntimePathForComparison(cwd) : null
      if (normalizedCwd === null || !workspaceMatchers.some((matches) => matches(normalizedCwd))) {
        return false
      }
    }
    if (filters.scope === 'project') {
      if (!filters.activeProjectKey) {
        return false
      }
      if (filters.sessionProjectById?.get(session.id)?.key !== filters.activeProjectKey) {
        return false
      }
    }
    return matchesQuery(session, parsedQuery, filters)
  })
  if (filtered.length < 2) {
    return filtered
  }
  return filtered
    .map((session) => ({ session, time: sessionSortTime(session, filters.sort) }))
    .sort((left, right) => right.time - left.time)
    .map(({ session }) => session)
}

export function groupAiVaultSessions(
  sessions: readonly AiVaultSession[],
  group: AiVaultGroup,
  options: {
    sessionProjectById?: ReadonlyMap<string, AiVaultSessionProject>
    projectLabelByKey?: ReadonlyMap<string, string>
  } = {}
): AiVaultSessionGroup[] {
  const groups = new Map<string, AiVaultSessionGroup>()

  for (const session of sessions) {
    const { key, label } = getGroupIdentity(session, group, options)
    const existing = groups.get(key)
    if (existing) {
      existing.sessions.push(session)
    } else {
      groups.set(key, { key, label, sessions: [session] })
    }
  }

  return [...groups.values()]
}

export function folderLabel(pathValue: string | null): string {
  if (!pathValue) {
    return 'Unknown location'
  }
  // NFC so one folder renders the same header whichever spelling (macOS NFD vs
  // agent-recorded NFC) reaches the group first.
  const parts = normalizeRuntimePathSeparators(pathValue.normalize('NFC'))
    .split('/')
    .filter(Boolean)
  if (parts.length >= 2) {
    return parts.slice(-2).join('/')
  }
  return parts[0] ?? pathValue
}

/**
 * Why comparison-normalized: cwd is copied verbatim out of agent transcripts, so
 * one folder arrives with and without a trailing slash and in both NFD/NFC — each
 * spelling otherwise became its own group under an identical `folderLabel`. Also
 * avoids blanket lowercasing, which merged distinct case-sensitive POSIX folders.
 * The `folder:` prefix matches the folder project key so project grouping and its
 * fallback agree.
 */
export function folderGroupKey(pathValue: string | null): string {
  return pathValue ? `folder:${normalizeRuntimePathForComparison(pathValue)}` : 'unknown'
}

export function agentLabel(agent: AiVaultAgent): string {
  return aiVaultAgentLabel(agent)
}

/**
 * One reading of `repo:` / `path:` for this list: the tokens are split by
 * `splitVaultQueryOperators` and folded, because everything this file compares
 * is folded. The split itself keeps the operand case; the fold happens here.
 */
export function parseVaultQuery(query: string): ParsedQuery {
  const split = splitVaultQueryOperators(query)
  const fold = (values: readonly string[]): string[] => values.map((value) => value.toLowerCase())
  return {
    terms: fold(split.terms),
    repoTerms: fold(split.repoTerms),
    pathTerms: fold(split.pathTerms)
  }
}

type SplitVaultQuery = {
  /** The free text as tokens with quotes stripped; what a substring matcher wants. */
  terms: string[]
  /** Operator values as typed apart from surrounding space. */
  repoTerms: string[]
  pathTerms: string[]
}

/** Anchored at a token start only, so `myrepo:x` and `https://h/path:x` stay literal. */
const VAULT_QUERY_OPERATOR = /(repo|path):/iy

function splitVaultQueryOperators(query: string): SplitVaultQuery {
  const terms: string[] = []
  const repoTerms: string[] = []
  const pathTerms: string[] = []
  let index = 0
  while (index < query.length) {
    if (isVaultQueryBoundary(query[index])) {
      index += 1
      continue
    }
    VAULT_QUERY_OPERATOR.lastIndex = index
    const operator = VAULT_QUERY_OPERATOR.exec(query)
    if (operator) {
      const at = index + operator[0].length
      const quoted = readVaultQueryQuoted(query, at)
      const value = quoted?.value ?? readVaultQueryBare(query, at)
      index = quoted ? quoted.end : at + value.length
      // Trimmed for the same reason an empty value is dropped: `repo:"  "` is
      // not a narrowing anyone typed on purpose, and an untrimmed one matches
      // no label at all, which silently empties the list.
      const operand = value.trim()
      if (operand) {
        ;(operator[1]!.toLowerCase() === 'repo' ? repoTerms : pathTerms).push(operand)
      }
      continue
    }
    const quoted = readVaultQueryQuoted(query, index)
    const value = quoted?.value ?? readVaultQueryBare(query, index)
    const end = quoted ? quoted.end : index + value.length
    // Only the substring matcher's copy is trimmed, so `"  "` reads as the
    // empty term `""` already does rather than as a term no session's text
    // contains; the quote characters themselves stay out of the token.
    terms.push(value.trim())
    index = end
  }
  return { terms, repoTerms, pathTerms }
}

function isVaultQueryBoundary(char: string | undefined): boolean {
  return char === undefined || /\s/.test(char)
}

/**
 * A quoted span, or null when this is not one.
 *
 * What keeps the apostrophes in `it's a repo:orca thing's` from opening a span
 * that swallows the operator is the caller: this only ever runs at a token
 * start, and the quote in `it's` is not at one. The closing quote is then just
 * the next one, wherever it falls, so `"a b"c` reads as the panel has always
 * read it — the span, then the rest as its own token.
 */
function readVaultQueryQuoted(query: string, at: number): { value: string; end: number } | null {
  const quote = query[at]
  if (quote !== '"' && quote !== "'") {
    return null
  }
  const close = query.indexOf(quote, at + 1)
  return close === -1 ? null : { value: query.slice(at + 1, close), end: close + 1 }
}

function readVaultQueryBare(query: string, at: number): string {
  let end = at
  while (end < query.length && !isVaultQueryBoundary(query[end])) {
    end += 1
  }
  return query.slice(at, end)
}

/** What `repo:` and `path:` are compared against for one session. */
export type AiVaultQueryOperatorTarget = {
  cwd: string | null
  filePath: string
  /**
   * What `repo:` matches. The panel passes a resolved project label when it has
   * one; everything else falls back to the last two path segments.
   */
  repoLabel?: string
}

/**
 * Whether one session satisfies every `repo:` and `path:` term.
 *
 * The single definition of what those operators mean. The search index applies
 * this over its retrieved rows rather than expressing it in SQL, because SQL
 * cannot: LIKE folds ASCII and nothing else, and `path:` searches the transcript
 * path as well as the working directory. Both keys are conjunctive, matching
 * the qualifier semantics the panel has always had.
 */
export function matchesAiVaultQueryOperators(
  target: AiVaultQueryOperatorTarget,
  operators: { repoTerms: readonly string[]; pathTerms: readonly string[] }
): boolean {
  if (operators.repoTerms.length > 0) {
    const repoLabel = (target.repoLabel ?? folderLabel(target.cwd)).toLowerCase()
    if (operators.repoTerms.some((term) => !repoLabel.includes(term.toLowerCase()))) {
      return false
    }
  }
  if (operators.pathTerms.length > 0) {
    const pathSearch = `${target.cwd ?? ''} ${target.filePath}`.toLowerCase()
    if (operators.pathTerms.some((term) => !pathSearch.includes(term.toLowerCase()))) {
      return false
    }
  }
  return true
}

function matchesQuery(
  session: AiVaultSession,
  parsed: ParsedQuery,
  filters: Pick<AiVaultSessionFilterState, 'sessionProjectById' | 'projectLabelByKey'>
): boolean {
  if (parsed.terms.length > 0) {
    const searchable = [
      session.title,
      session.sessionId,
      session.agent,
      session.branch,
      session.model,
      session.cwd,
      session.filePath,
      sessionPreviewSearchText(session)
    ]
      .filter(Boolean)
      .join(' ')
      .toLowerCase()
    if (parsed.terms.some((term) => !searchable.includes(term))) {
      return false
    }
  }
  const sessionProject = filters.sessionProjectById?.get(session.id)
  return matchesAiVaultQueryOperators(
    {
      cwd: session.cwd,
      filePath: session.filePath,
      repoLabel:
        sessionProject?.kind === 'repo'
          ? (filters.projectLabelByKey?.get(sessionProject.key) ?? sessionProject.label)
          : undefined
    },
    parsed
  )
}

function sessionSortTime(session: AiVaultSession, sort: AiVaultSort): number {
  const value = sort === 'created' ? session.createdAt : session.updatedAt
  return Date.parse(value ?? session.modifiedAt)
}

function getGroupIdentity(
  session: AiVaultSession,
  group: AiVaultGroup,
  options: {
    sessionProjectById?: ReadonlyMap<string, AiVaultSessionProject>
    projectLabelByKey?: ReadonlyMap<string, string>
  }
): Pick<AiVaultSessionGroup, 'key' | 'label'> {
  if (group === 'agent') {
    return { key: session.agent, label: agentLabel(session.agent) }
  }
  if (group === 'project') {
    const sessionProject = options.sessionProjectById?.get(session.id)
    if (sessionProject) {
      return {
        key: sessionProject.key,
        label:
          options.projectLabelByKey?.get(sessionProject.key) ||
          sessionProject.label ||
          folderLabel(session.cwd)
      }
    }
  }
  return { key: folderGroupKey(session.cwd), label: folderLabel(session.cwd) }
}

function createAiVaultWorkspaceMatcher(workspacePath: string): (normalizedCwd: string) => boolean {
  const matches = createNormalizedPathInsideOrEqualMatcher(workspacePath)
  const workspaceWslPath = parseWslUncPath(workspacePath)
  if (!workspaceWslPath) {
    return matches
  }
  // WSL transcripts record Linux cwd even when the workspace uses a UNC path.
  const matchesLinux = createNormalizedPathInsideOrEqualMatcher(workspaceWslPath.linuxPath)
  return (cwd) => matches(cwd) || matchesLinux(cwd)
}
