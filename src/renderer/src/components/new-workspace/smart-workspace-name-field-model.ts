import type React from 'react'
import type { useAppStore } from '@/store'
import type { BaseRefSearchResult } from '../../../../shared/repo-types'
import type { SmartNameMode, SmartWorkspaceSourceRow } from './smart-workspace-source-results'

export type RepoOption = ReturnType<typeof useAppStore.getState>['repos'][number]

export type SmartWorkspaceNameFieldProps = {
  repos: readonly RepoOption[]
  repoId: string
  onRepoChange: (repoId: string) => void
  value: string
  onValueChange: (value: string) => void
  onBranchSelect: (refName: string, localBranchName: string) => void
  selectedSource: SmartWorkspaceNameSelection | null
  onClearSelectedSource: () => void
  inputRef?: React.RefObject<HTMLInputElement | null>
  onPlainEnter?: () => void
  disabled?: boolean
  disabledPlaceholder?: string
  textOnly?: boolean
  branchesEnabled?: boolean
  onActiveSourceModeChange?: (mode: SmartNameMode) => void
}

export type NormalizedSmartWorkspaceNameFieldProps = Omit<
  SmartWorkspaceNameFieldProps,
  'disabled' | 'textOnly' | 'branchesEnabled'
> & {
  disabled: boolean
  textOnly: boolean
  branchesEnabled: boolean
}

export type SmartWorkspaceNameSelection = {
  kind: 'branch'
  label: string
  url?: string
}

export type RowEntry = SmartWorkspaceSourceRow

export type SmartWorkspaceNameFieldSearchState = {
  debouncedQuery: string
  branches: BaseRefSearchResult[]
  branchResultsSource: { repoId: string; query: string } | null
}

export const SEARCH_DEBOUNCE_MS = 200
export const RESULT_LIMIT = 12
