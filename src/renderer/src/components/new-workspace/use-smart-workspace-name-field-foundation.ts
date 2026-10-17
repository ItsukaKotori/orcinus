import { useMemo } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useAppStore } from '@/store'
import { getLocalPreflightContext, localPreflightContextKey } from '@/lib/local-preflight-context'
import { getRepoOwnerRoutedSettings } from '@/lib/repo-runtime-owner'
import type { NormalizedSmartWorkspaceNameFieldProps } from './smart-workspace-name-field-model'
import { useSmartWorkspaceFieldAvailability } from './use-smart-workspace-field-availability'
import { useSmartWorkspaceNameFieldState } from './use-smart-workspace-name-field-state'

export function useSmartWorkspaceNameFieldFoundation(
  props: NormalizedSmartWorkspaceNameFieldProps
) {
  const { repos, repoId, textOnly, value } = props
  const {
    addRepo,
    preflightStatusChecked,
    preflightStatusContextKey,
    expectedPreflightContextKey,
    refreshPreflightStatus,
    settings
  } = useAppStore(
    useShallow((s) => ({
      addRepo: s.addRepo,
      preflightStatusChecked: s.preflightStatusChecked,
      preflightStatusContextKey: s.preflightStatusContextKey,
      expectedPreflightContextKey: localPreflightContextKey(getLocalPreflightContext(s)),
      refreshPreflightStatus: s.refreshPreflightStatus,
      settings: s.settings
    }))
  )
  const selectedRepo = useMemo(
    () => repos.find((repo) => repo.id === repoId) ?? null,
    [repoId, repos]
  )
  const selectedRepoOwnerSettings = useMemo(
    () => getRepoOwnerRoutedSettings(settings, selectedRepo),
    [selectedRepo, settings]
  )
  const state = useSmartWorkspaceNameFieldState(textOnly, value)
  const availability = useSmartWorkspaceFieldAvailability({
    props,
    state,
    preflightStatusChecked,
    preflightStatusContextKey,
    expectedPreflightContextKey,
    refreshPreflightStatus
  })

  return {
    ...props,
    ...state,
    ...availability,
    addRepo,
    selectedRepo,
    selectedRepoOwnerSettings
  }
}
