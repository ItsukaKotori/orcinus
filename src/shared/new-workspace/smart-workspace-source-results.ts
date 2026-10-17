import type { BaseRefSearchResult } from '../repo-types'
import { isSmartWorkspaceSourceQueryWithinLimit } from './smart-workspace-source-query'

export {
  SMART_WORKSPACE_SOURCE_QUERY_MAX_BYTES,
  isSmartWorkspaceSourceQueryWithinLimit
} from './smart-workspace-source-query'

export type SmartNameMode = 'smart' | 'branches' | 'text'

export type SmartWorkspaceSourceRow =
  | { kind: 'use-name'; value: string; name: string }
  | { kind: 'create-branch'; value: string; name: string }
  | { kind: 'branch'; value: string; refName: string; localBranchName: string }

const EMPTY_HINT_BY_MODE: Record<SmartNameMode, string> = {
  smart: 'Start typing to create a name or find a source.',
  branches: 'No matching branches.',
  text: ''
}

export function getSmartWorkspaceEmptyHint(mode: SmartNameMode): string {
  return EMPTY_HINT_BY_MODE[mode]
}

export function getBranchSearchRequest({
  branchesEnabled,
  disabled,
  textOnly,
  mode,
  selectedRepoId,
  query,
  limit
}: {
  branchesEnabled?: boolean
  disabled: boolean
  textOnly: boolean
  mode: SmartNameMode
  selectedRepoId: string | null
  query: string
  limit: number
}): { repoId: string; query: string; limit: number } | null {
  if (
    branchesEnabled === false ||
    disabled ||
    textOnly ||
    !isSmartWorkspaceSourceQueryWithinLimit(query) ||
    !selectedRepoId
  ) {
    return null
  }
  const trimmedQuery = query.trim()
  const shouldSearchBranches = mode === 'branches' || (mode === 'smart' && trimmedQuery.length > 0)
  if (!shouldSearchBranches) {
    return null
  }
  return { repoId: selectedRepoId, query: trimmedQuery, limit }
}

export function getVisibleBranchResults({
  branches,
  mode,
  resultRepoId,
  resultQuery,
  selectedRepoId,
  value
}: {
  branches: BaseRefSearchResult[]
  mode: SmartNameMode
  resultRepoId: string | null
  resultQuery: string | null
  selectedRepoId: string | null
  value: string
}): BaseRefSearchResult[] {
  if (!isSmartWorkspaceSourceQueryWithinLimit(value)) {
    return []
  }
  if (mode !== 'branches' && mode !== 'smart') {
    return []
  }
  if (!selectedRepoId || resultRepoId !== selectedRepoId || resultQuery === null) {
    return []
  }
  const currentQuery = value.trim()
  // Why: hold the last settled list while the user extends/trims the query so the
  // dropdown does not blank between debounced keystrokes. Drop the hold when the
  // query diverges (e.g. "feat" → "bug") so unrelated rows do not linger.
  if (currentQuery === '') {
    return resultQuery === '' ? branches : []
  }
  if (!shouldHoldSourceResultsForQuery({ resultQuery, value: currentQuery })) {
    return []
  }
  return branches
}

/** Max |live − settled| length while still treating a prefix as "still typing". */
const SOURCE_RESULT_HOLD_MAX_DELTA = 4

/**
 * Why: prefix-only hold lets a settled "f" stick under "fix-unrelated-…" for the
 * whole next debounce. Cap the length delta so hold covers fast typing, not long
 * continuations of a short settled query.
 */
export function shouldHoldSourceResultsForQuery({
  resultQuery,
  value
}: {
  resultQuery: string
  value: string
}): boolean {
  const currentQueryKey = value.trim().toLowerCase()
  const resultQueryKey = resultQuery.trim().toLowerCase()
  if (resultQueryKey === currentQueryKey) {
    return true
  }
  if (!currentQueryKey.startsWith(resultQueryKey) && !resultQueryKey.startsWith(currentQueryKey)) {
    return false
  }
  return Math.abs(currentQueryKey.length - resultQueryKey.length) <= SOURCE_RESULT_HOLD_MAX_DELTA
}

export function buildSmartWorkspaceSourceRows({
  branches,
  mode,
  resultLimit,
  value
}: {
  branches: BaseRefSearchResult[]
  mode: SmartNameMode
  resultLimit: number
  value: string
}): SmartWorkspaceSourceRow[] {
  if (!isSmartWorkspaceSourceQueryWithinLimit(value)) {
    return []
  }
  const trimmed = value.trim()
  const nextRows: SmartWorkspaceSourceRow[] = []
  if (trimmed && mode === 'smart') {
    // Why: stable cmdk value — embedding the query remounted the row every keystroke.
    nextRows.push({ kind: 'use-name', value: 'use-name', name: trimmed })
  }
  if (mode === 'text') {
    return nextRows
  }
  const shouldShowBranches = mode === 'branches' || (mode === 'smart' && trimmed.length > 0)
  if (shouldShowBranches) {
    const branchExactMatch = branches.some(
      (branch) => branch.refName === trimmed || branch.localBranchName === trimmed
    )
    if (trimmed && mode === 'branches' && !branchExactMatch) {
      nextRows.push({ kind: 'create-branch', value: 'create-branch', name: trimmed })
    }
    nextRows.push(
      ...branches.map((branch) => ({
        kind: 'branch' as const,
        value: `branch-${branch.refName}`,
        refName: branch.refName,
        localBranchName: branch.localBranchName
      }))
    )
  }
  return nextRows.slice(0, resultLimit + 1)
}
