import { useEffect } from 'react'
import { getSmartWorkspaceNameModes } from './smart-workspace-localized-options'
import {
  SEARCH_DEBOUNCE_MS,
  type NormalizedSmartWorkspaceNameFieldProps
} from './smart-workspace-name-field-model'
import { useSmartWorkspaceFieldFocusControls } from './use-smart-workspace-field-focus-controls'
import type { useSmartWorkspaceNameFieldState } from './use-smart-workspace-name-field-state'

type FieldState = ReturnType<typeof useSmartWorkspaceNameFieldState>

export function useSmartWorkspaceFieldAvailability({
  props,
  state,
  preflightStatusChecked,
  preflightStatusContextKey,
  expectedPreflightContextKey,
  refreshPreflightStatus
}: {
  props: NormalizedSmartWorkspaceNameFieldProps
  state: FieldState
  preflightStatusChecked: boolean
  preflightStatusContextKey: string | null
  expectedPreflightContextKey: string
  refreshPreflightStatus: () => Promise<void>
}) {
  const { disabled, textOnly, branchesEnabled, onActiveSourceModeChange, value } = props
  const {
    mode,
    setMode,
    setOpen,
    setBranches,
    setBranchesLoading,
    setBranchResultsSource,
    setCommandValue,
    setDebouncedQuery
  } = state

  useEffect(() => {
    onActiveSourceModeChange?.(mode)
  }, [mode, onActiveSourceModeChange])
  const preflightStatusCurrent = preflightStatusContextKey === expectedPreflightContextKey
  const availableModes = getSmartWorkspaceNameModes().filter((item) => {
    if (textOnly) {
      return item.id === 'text'
    }
    if (item.id === 'branches') {
      return branchesEnabled
    }
    return true
  })

  useEffect(() => {
    if (availableModes.some((item) => item.id === mode)) {
      return
    }
    setMode(availableModes[0]?.id ?? 'text')
  }, [availableModes, mode, setMode])

  const focusControls = useSmartWorkspaceFieldFocusControls({ props, state })

  useEffect(() => {
    if (disabled || textOnly) {
      return
    }
    if (!preflightStatusChecked || !preflightStatusCurrent) {
      void refreshPreflightStatus()
    }
  }, [
    disabled,
    preflightStatusChecked,
    preflightStatusCurrent,
    refreshPreflightStatus,
    textOnly
  ])

  useEffect(() => {
    if (textOnly) {
      if (mode !== 'text') {
        setMode('text')
      }
      setOpen(false)
    }
  }, [mode, setMode, setOpen, textOnly])

  useEffect(() => {
    if (!disabled) {
      return
    }
    setOpen(false)
    setBranches([])
    setBranchResultsSource(null)
    setBranchesLoading(false)
    setCommandValue('')
  }, [
    disabled,
    setBranches,
    setBranchesLoading,
    setBranchResultsSource,
    setCommandValue,
    setOpen
  ])

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedQuery(value), SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [setDebouncedQuery, value])

  return {
    availableModes,
    ...focusControls
  }
}
