import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/store'
import { useRepoMap, useAllWorktrees } from '@/store/selectors'
import { localPreflightContextKey, getLocalPreflightContext } from '@/lib/local-preflight-context'
import { getProviderRuntimeContextKey } from '@/lib/provider-runtime-context'
import { useRef } from 'react'
import { getScreenSubmitShortcutLabel } from '@/lib/screen-submit-shortcut'
export function useTaskPageStoreBindings() {
  useTranslation()
  const settings = useAppStore((s) => s.settings)
  const persistedUIReady = useAppStore((s) => s.persistedUIReady)
  const taskResumeState = useAppStore((s) => s.taskResumeState)
  const setTaskResumeState = useAppStore((s) => s.setTaskResumeState)
  const pageData = useAppStore((s) => s.taskPageData)
  const openTaskPage = useAppStore((s) => s.openTaskPage)
  const closeTaskPage = useAppStore((s) => s.closeTaskPage)
  const activeModal = useAppStore((s) => s.activeModal)
  const repos = useAppStore((s) => s.repos)
  const sshConnectionStates = useAppStore((s) => s.sshConnectionStates)
  const sshTargetLabels = useAppStore((s) => s.sshTargetLabels)
  const runtimeEnvironments = useAppStore((s) => s.runtimeEnvironments)
  const runtimeStatusByEnvironmentId = useAppStore((s) => s.runtimeStatusByEnvironmentId)
  const repoMap = useRepoMap()
  const allWorktrees = useAllWorktrees()
  const openModal = useAppStore((s) => s.openModal)
  const updateSettings = useAppStore((s) => s.updateSettings)
  const fetchWorkItemsAcrossRepos = useAppStore((s) => s.fetchWorkItemsAcrossRepos)
  const fetchPRChecks = useAppStore((s) => s.fetchPRChecks)
  const getCachedWorkItems = useAppStore((s) => s.getCachedWorkItems)
  const setIssueSourcePreference = useAppStore((s) => s.setIssueSourcePreference)
  // Why: bumped after cache eviction to re-run the fetch effect — eviction alone won't, since its deps don't include workItemsCache.
  const workItemsInvalidationNonce = useAppStore((s) => s.workItemsInvalidationNonce)
  const preflightStatus = useAppStore((s) => s.preflightStatus)
  const preflightStatusChecked = useAppStore((s) => s.preflightStatusChecked)
  const preflightStatusContextKey = useAppStore((s) => s.preflightStatusContextKey)
  const folderWorkspaces = useAppStore((s) => s.folderWorkspaces)
  const refreshPreflightStatus = useAppStore((s) => s.refreshPreflightStatus)
  const expectedPreflightContextKey = useAppStore((s) =>
    localPreflightContextKey(getLocalPreflightContext(s))
  )
  const providerRuntimeContextKey = getProviderRuntimeContextKey(settings)
  const providerRuntimeContextKeyRef = useRef(providerRuntimeContextKey)
  // Submit handlers must fence against the current provider context immediately.
  // react-doctor-disable-next-line react-doctor/no-ref-current-in-render
  providerRuntimeContextKeyRef.current = providerRuntimeContextKey
  const preflightStatusCurrent = preflightStatusContextKey === expectedPreflightContextKey
  const submitShortcutLabel = getScreenSubmitShortcutLabel()
  return {
    settings,
    persistedUIReady,
    taskResumeState,
    setTaskResumeState,
    pageData,
    openTaskPage,
    closeTaskPage,
    activeModal,
    repos,
    sshConnectionStates,
    sshTargetLabels,
    runtimeEnvironments,
    runtimeStatusByEnvironmentId,
    repoMap,
    allWorktrees,
    openModal,
    updateSettings,
    fetchWorkItemsAcrossRepos,
    fetchPRChecks,
    getCachedWorkItems,
    setIssueSourcePreference,
    workItemsInvalidationNonce,
    preflightStatus,
    preflightStatusChecked,
    preflightStatusContextKey,
    folderWorkspaces,
    refreshPreflightStatus,
    expectedPreflightContextKey,
    providerRuntimeContextKey,
    providerRuntimeContextKeyRef,
    preflightStatusCurrent,
    submitShortcutLabel
  }
}
export type TaskPageStoreBindingsModel = ReturnType<typeof useTaskPageStoreBindings>
