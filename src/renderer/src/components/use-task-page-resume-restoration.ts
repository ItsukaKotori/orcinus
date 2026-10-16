import type { TaskPageDetailRoutingModel } from './use-task-page-detail-routing'
import { useEffect } from 'react'
import { resolveVisibleTaskProvider } from '../../../shared/task-providers'
import { normalizeGitHubTaskPreset } from '@/components/task-page-github-task-kind'
import { getTaskPresetQuery } from '../../../shared/task-preset-query'
export function useTaskPageResumeRestoration(model: TaskPageDetailRoutingModel) {
  const {
    settings,
    persistedUIReady,
    taskResumeState,
    pageData,
    resolvedInitialSelection,
    setRepoSelection,
    visibleTaskProviders,
    setTaskSource,
    setGithubMode,
    setTaskSearchInput,
    setAppliedTaskSearch,
    setActiveTaskPreset,
    taskResumeAppliedRef,
    setTaskResumeApplied
  } = model
  useEffect(() => {
    if (taskResumeAppliedRef.current || !persistedUIReady || !settings) {
      return
    }
    setTaskSource(
      resolveVisibleTaskProvider(
        pageData.taskSource ?? settings.defaultTaskSource,
        visibleTaskProviders
      )
    )
    setRepoSelection(resolvedInitialSelection)
    const nextGithubMode = taskResumeState?.githubMode ?? 'items'
    setGithubMode(nextGithubMode)
    const preset = taskResumeState?.githubItemsPreset
    if (preset === null) {
      const query = taskResumeState?.githubItemsQuery ?? ''
      setTaskSearchInput(query)
      setAppliedTaskSearch(query)
      setActiveTaskPreset(null)
    } else {
      const presetId = normalizeGitHubTaskPreset(preset ?? settings.defaultTaskViewPreset)
      const query = getTaskPresetQuery(presetId)
      setTaskSearchInput(query)
      setAppliedTaskSearch(query)
      setActiveTaskPreset(presetId)
    }

    // Why: settings/UI hydrate async; apply the restored Tasks context exactly once so later source/filter clicks stay local.
    taskResumeAppliedRef.current = true
    setTaskResumeApplied(true)
  }, [
    persistedUIReady,
    settings,
    pageData.taskSource,
    resolvedInitialSelection,
    taskResumeState,
    visibleTaskProviders,
    setTaskSearchInput,
    setActiveTaskPreset,
    taskResumeAppliedRef,
    setAppliedTaskSearch,
    setTaskSource,
    setRepoSelection,
    setGithubMode,
    setTaskResumeApplied
  ])
  return model
}
export type TaskPageResumeRestorationModel = ReturnType<typeof useTaskPageResumeRestoration>
