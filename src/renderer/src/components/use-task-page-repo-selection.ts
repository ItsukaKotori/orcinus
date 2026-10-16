import type { TaskPageStoreBindingsModel } from './use-task-page-store-bindings'
import { useMemo, useState, useRef, useEffect, useCallback } from 'react'
import {
  getTaskEligibleRepos,
  normalizeTaskRepoSelection,
  getDefaultTaskRepoSelection,
  getTaskProjectPickerGroups
} from '@/components/task-page-default-repo-selection'
import { areStringSetsEqual } from '@/components/task-page-string-set-equality'
import { buildSelectedReposKey } from '@/components/task-page-work-item-pagination'
import {
  normalizeVisibleTaskProviders,
  restoreAvailableDefaultTaskProvider,
  resolveVisibleTaskProvider
} from '../../../shared/task-providers'
import { getSourceOptions, getGitHubModeButtons } from '@/components/task-page-localized-options'
import type { TaskProvider } from '../../../shared/task-providers'
import { toast } from 'sonner'
import { translate } from '@/i18n/i18n'
import { getTaskPageRepoSourceContext } from './task-page-source-context'
export function useTaskPageRepoSelection(model: TaskPageStoreBindingsModel) {
  const { settings, pageData, repos, updateSettings } = model
  const eligibleRepos = useMemo(() => getTaskEligibleRepos(repos), [repos])

  // Why: initial selection precedence — explicit preselection > persisted defaultRepoSelection > all eligible; preselection wins so "open tasks for this repo" lands single-repo.
  const resolvedInitialSelection = useMemo<ReadonlySet<string>>(() => {
    const preferred = pageData.preselectedRepoId
    if (preferred && eligibleRepos.some((repo) => repo.id === preferred)) {
      return new Set([preferred])
    }
    const persisted = settings?.defaultRepoSelection
    if (Array.isArray(persisted)) {
      return normalizeTaskRepoSelection(eligibleRepos, new Set(persisted))
    }
    return getDefaultTaskRepoSelection(eligibleRepos)
  }, [eligibleRepos, pageData.preselectedRepoId, settings?.defaultRepoSelection])
  const [repoSelection, setRepoSelection] = useState<ReadonlySet<string>>(resolvedInitialSelection)
  const taskPickerGroups = useMemo(
    () => getTaskProjectPickerGroups(eligibleRepos, repoSelection),
    [eligibleRepos, repoSelection]
  )
  const taskPickerRepos = useMemo(
    () => taskPickerGroups.map((group) => group.repo),
    [taskPickerGroups]
  )

  // Why: prune removed repos and preserve sticky-all (selection == all projects stays == all), without recreating the Set each time and churning the fetch effect.
  const prevTaskPickerCountRef = useRef(taskPickerRepos.length)
  useEffect(() => {
    const prevCount = prevTaskPickerCountRef.current
    prevTaskPickerCountRef.current = taskPickerRepos.length
    const eligibleIds = new Set(eligibleRepos.map((r) => r.id))
    const wasAll = repoSelection.size === prevCount && prevCount > 0
    const pruned = new Set<string>()
    for (const id of repoSelection) {
      if (eligibleIds.has(id)) {
        pruned.add(id)
      }
    }
    if (wasAll) {
      const allNow = new Set(taskPickerRepos.map((repo) => repo.id))
      if (!areStringSetsEqual(allNow, repoSelection)) {
        setRepoSelection(allNow)
      }
      return
    }
    if (pruned.size === 0 && eligibleIds.size === 0) {
      return
    }
    const normalized = normalizeTaskRepoSelection(eligibleRepos, pruned)
    if (!areStringSetsEqual(normalized, repoSelection)) {
      setRepoSelection(normalized)
    }
  }, [eligibleRepos, repoSelection, taskPickerRepos])
  const selectedRepos = useMemo(
    () => eligibleRepos.filter((r) => repoSelection.has(r.id)),
    [eligibleRepos, repoSelection]
  )

  // Why: see buildSelectedReposKey — array-identity deps re-fire on every
  // repos:changed even when the selection is unchanged.
  const selectedReposKey = useMemo(
    () => buildSelectedReposKey(selectedRepos, (r) => getTaskPageRepoSourceContext(r, 'github')),
    [selectedRepos]
  )

  // Why: many affordances need *a* repo; use the first selected as default, while cross-repo dialogs still let the user override per-action.
  const primaryRepo = selectedRepos[0] ?? null
  const preferredVisibleTaskProviders = useMemo(
    () => normalizeVisibleTaskProviders(settings?.visibleTaskProviders),
    [settings?.visibleTaskProviders]
  )
  const defaultTaskSource = settings?.defaultTaskSource ?? 'github'
  const visibleTaskProviders = useMemo(
    () =>
      restoreAvailableDefaultTaskProvider(
        preferredVisibleTaskProviders,
        {
          gitlabInstalled: false,
          linearConnected: false
        },
        defaultTaskSource
      ),
    [defaultTaskSource, preferredVisibleTaskProviders]
  )
  const sourceOptions = getSourceOptions()
  const githubModeButtons = getGitHubModeButtons()
  const visibleSourceOptions = useMemo(
    () => sourceOptions.filter((source) => visibleTaskProviders.includes(source.id)),
    [sourceOptions, visibleTaskProviders]
  )
  const hideTaskSource = useCallback(
    (provider: TaskProvider, label: string) => {
      const visibleWithoutProvider = preferredVisibleTaskProviders.filter(
        (visibleProvider) => visibleProvider !== provider
      )
      // Why: an empty provider list normalizes to "all providers", so keep one other source visible or hiding this one has no effect.
      const nextVisibleTaskProviders: TaskProvider[] =
        visibleWithoutProvider.length > 0 ? visibleWithoutProvider : ['github']
      const nextDefaultTaskSource = resolveVisibleTaskProvider(
        defaultTaskSource,
        nextVisibleTaskProviders
      )
      void updateSettings({
        visibleTaskProviders: nextVisibleTaskProviders,
        defaultTaskSource: nextDefaultTaskSource
      }).catch(() => {
        toast.error(
          translate('auto.components.TaskPage.e9139db03f', 'Failed to hide {{value0}}.', {
            value0: label
          })
        )
      })
    },
    [defaultTaskSource, preferredVisibleTaskProviders, updateSettings]
  )

  // Why: seed preset + query synchronously so the first fetch issues one request; a prior post-mount re-seed caused a throwaway empty-query fetch, doubling time-to-first-paint.
  const nextModel = model as typeof model & {
    eligibleRepos: typeof eligibleRepos
    resolvedInitialSelection: typeof resolvedInitialSelection
    repoSelection: typeof repoSelection
    setRepoSelection: typeof setRepoSelection
    taskPickerGroups: typeof taskPickerGroups
    taskPickerRepos: typeof taskPickerRepos
    prevTaskPickerCountRef: typeof prevTaskPickerCountRef
    selectedRepos: typeof selectedRepos
    selectedReposKey: typeof selectedReposKey
    primaryRepo: typeof primaryRepo
    preferredVisibleTaskProviders: typeof preferredVisibleTaskProviders
    defaultTaskSource: typeof defaultTaskSource
    visibleTaskProviders: typeof visibleTaskProviders
    sourceOptions: typeof sourceOptions
    githubModeButtons: typeof githubModeButtons
    visibleSourceOptions: typeof visibleSourceOptions
    hideTaskSource: typeof hideTaskSource
  }
  nextModel.eligibleRepos = eligibleRepos
  nextModel.resolvedInitialSelection = resolvedInitialSelection
  nextModel.repoSelection = repoSelection
  nextModel.setRepoSelection = setRepoSelection
  nextModel.taskPickerGroups = taskPickerGroups
  nextModel.taskPickerRepos = taskPickerRepos
  nextModel.prevTaskPickerCountRef = prevTaskPickerCountRef
  nextModel.selectedRepos = selectedRepos
  nextModel.selectedReposKey = selectedReposKey
  nextModel.primaryRepo = primaryRepo
  nextModel.preferredVisibleTaskProviders = preferredVisibleTaskProviders
  nextModel.defaultTaskSource = defaultTaskSource
  nextModel.visibleTaskProviders = visibleTaskProviders
  nextModel.sourceOptions = sourceOptions
  nextModel.githubModeButtons = githubModeButtons
  nextModel.visibleSourceOptions = visibleSourceOptions
  nextModel.hideTaskSource = hideTaskSource
  return nextModel
}
export type TaskPageRepoSelectionModel = ReturnType<typeof useTaskPageRepoSelection>
