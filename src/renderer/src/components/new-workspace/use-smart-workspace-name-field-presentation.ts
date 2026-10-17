import { useEffect, useMemo } from 'react'
import { CaseSensitive, LoaderCircle, Search } from 'lucide-react'
import {
  getActiveWorkspaceEmojiShortcode,
  searchWorkspaceEmojiShortcodes,
  type WorkspaceEmojiSuggestion
} from '@/lib/workspace-emoji-shortcodes'
import { resolveSmartWorkspaceCommandValue } from './smart-workspace-command-value'
import {
  buildSmartWorkspaceSourceRows,
  getVisibleBranchResults,
  isSmartWorkspaceSourceQueryWithinLimit
} from './smart-workspace-source-results'
import { RESULT_LIMIT, type RowEntry } from './smart-workspace-name-field-model'
import type { useSmartWorkspaceNameFieldFoundation } from './use-smart-workspace-name-field-foundation'

type Foundation = ReturnType<typeof useSmartWorkspaceNameFieldFoundation>

function isTypedTextSourceRow(row: RowEntry): boolean {
  return row.kind === 'use-name' || row.kind === 'create-branch'
}

export function useSmartWorkspaceNameFieldPresentation(foundation: Foundation) {
  const {
    branches,
    mode,
    branchResultsSource,
    selectedRepo,
    value,
    debouncedQuery,
    commandValue,
    setCommandValue,
    emojiCursor,
    disabled,
    selectedSource,
    emojiCommandValue,
    branchesLoading
  } = foundation
  const rows = useMemo<RowEntry[]>(
    () =>
      buildSmartWorkspaceSourceRows({
        branches: getVisibleBranchResults({
          branches,
          mode,
          resultRepoId: branchResultsSource?.repoId ?? null,
          resultQuery: branchResultsSource?.query ?? null,
          selectedRepoId: selectedRepo?.id ?? null,
          value
        }),
        mode,
        resultLimit: RESULT_LIMIT,
        value
      }),
    [branches, branchResultsSource, mode, selectedRepo?.id, value]
  )
  const { typedTextActionRow, searchResultRows } = useMemo(() => {
    const typedTextRow = rows.find(isTypedTextSourceRow) ?? null
    return {
      typedTextActionRow: typedTextRow,
      searchResultRows: typedTextRow ? rows.filter((row) => row !== typedTextRow) : rows
    }
  }, [rows])

  // Why: live input leads debounced search; freeze highlight until the query catches up.
  const valueWithinSourceLimit = isSmartWorkspaceSourceQueryWithinLimit(value)
  const debouncedQueryWithinSourceLimit = isSmartWorkspaceSourceQueryWithinLimit(debouncedQuery)
  const trimmedValue = valueWithinSourceLimit ? value.trim() : ''
  const trimmedDebouncedQuery = debouncedQueryWithinSourceLimit ? debouncedQuery.trim() : ''
  const isQueryStale = trimmedValue.length > 0 && trimmedDebouncedQuery !== trimmedValue
  const resolvedCommandValue = resolveSmartWorkspaceCommandValue({
    currentValue: commandValue,
    rows,
    isQueryStale
  })
  // Why: ignored stale cmdk changes must not reappear after the query settles.
  useEffect(() => {
    if (commandValue === resolvedCommandValue) {
      return
    }
    setCommandValue(resolvedCommandValue)
  }, [commandValue, resolvedCommandValue, setCommandValue])
  const activeEmojiShortcode = useMemo(
    () => getActiveWorkspaceEmojiShortcode(value, emojiCursor),
    [emojiCursor, value]
  )
  const emojiSuggestions = useMemo(
    () =>
      activeEmojiShortcode
        ? searchWorkspaceEmojiShortcodes(activeEmojiShortcode.query)
        : ([] as WorkspaceEmojiSuggestion[]),
    [activeEmojiShortcode]
  )
  const emojiMenuOpen =
    !disabled &&
    selectedSource === null &&
    activeEmojiShortcode !== null &&
    emojiSuggestions.length > 0
  const resolvedEmojiCommandValue = emojiSuggestions.some(
    (suggestion) => `emoji:${suggestion.shortcode}` === emojiCommandValue
  )
    ? emojiCommandValue
    : emojiSuggestions[0]
      ? `emoji:${emojiSuggestions[0].shortcode}`
      : ''
  const selectedEmojiSuggestion =
    emojiSuggestions.find(
      (suggestion) => `emoji:${suggestion.shortcode}` === resolvedEmojiCommandValue
    ) ?? null
  const loading = branchesLoading
  // Why: only spin on first load, not refreshes with retained rows.
  const showSearchSpinner = loading && searchResultRows.length === 0
  const ActiveInputIcon =
    mode === 'text' ? CaseSensitive : showSearchSpinner ? LoaderCircle : Search

  return {
    rows,
    typedTextActionRow,
    searchResultRows,
    isQueryStale,
    resolvedCommandValue,
    activeEmojiShortcode,
    emojiSuggestions,
    emojiMenuOpen,
    resolvedEmojiCommandValue,
    selectedEmojiSuggestion,
    loading,
    showSearchSpinner,
    ActiveInputIcon
  }
}
