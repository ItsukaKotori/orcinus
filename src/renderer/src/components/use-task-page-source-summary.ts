import type { TaskPageSourceAvailabilityPreludeModel } from './use-task-page-source-availability'
import { useMemo } from 'react'
import type { TaskProvider } from '../../../shared/task-providers'
import type {
  TaskSourceAvailabilityNotice,
  TaskSourceHostAvailability
} from './task-source-context-summary'
import type { TaskSourceContext } from '../../../shared/task-source-context'
import {
  getTaskSourceHostAvailabilityForHost,
  getTaskPageRepoSourceContext
} from './task-page-source-context'
import { getRepoBackedProviderAvailability } from '@/components/task-source-provider-availability'
import {
  getTaskSourceAvailabilityNotice,
  getTaskSourceContextSummary
} from './task-source-context-summary'
import { getRepoBackedTaskEmptyState } from '@/components/task-page-empty-state'
export function useTaskPageSourceSummary(model: TaskPageSourceAvailabilityPreludeModel) {
  const {
    preflightStatus,
    preflightStatusChecked,
    preflightStatusCurrent,
    selectedRepos,
    sourceOptions,
    taskSource,
    runtimePreflightStatusByHostId,
    taskSourceRepoContexts,
    hostRegistryById,
    hostLabelById,
    taskSourceHostAvailability
  } = model
  const taskSourceAvailabilityNoticeByProvider = useMemo<
    Partial<Record<TaskProvider, TaskSourceAvailabilityNotice>>
  >(() => {
    const availabilityForContexts = (
      provider: Extract<TaskProvider, 'github'>,
      contexts: readonly TaskSourceContext[]
    ): TaskSourceHostAvailability[] => [
      ...contexts.flatMap((context) => {
        const host = hostRegistryById.get(context.hostId)
        const availability = getTaskSourceHostAvailabilityForHost(host, context.hostId)
        return availability ? [availability] : []
      }),
      ...getRepoBackedProviderAvailability({
        provider,
        contexts,
        preflightStatus,
        preflightReady: preflightStatusCurrent && preflightStatusChecked,
        runtimePreflightStatusByHostId
      })
    ]
    const labelFor = (provider: TaskProvider): string =>
      sourceOptions.find((source) => source.id === provider)?.label ?? provider
    return {
      github:
        getTaskSourceAvailabilityNotice({
          providerLabel: labelFor('github'),
          sourceCount: selectedRepos.length,
          hostLabelById,
          hostAvailability: availabilityForContexts(
            'github',
            selectedRepos
              .map((repo) => getTaskPageRepoSourceContext(repo, 'github'))
              .filter((context): context is TaskSourceContext => context !== null)
          )
        }) ?? undefined
    }
  }, [
    hostRegistryById,
    hostLabelById,
    preflightStatus,
    preflightStatusChecked,
    preflightStatusCurrent,
    runtimePreflightStatusByHostId,
    selectedRepos,
    sourceOptions
  ])
  const taskSourceContextSummary = useMemo(() => {
    const providerLabel =
      sourceOptions.find((source) => source.id === taskSource)?.label ?? taskSource
    return getTaskSourceContextSummary({
      provider: taskSource,
      providerLabel,
      repoContexts: taskSourceRepoContexts,
      hostAvailability: taskSourceHostAvailability,
      hostLabelById,
      selectedRepoCount: selectedRepos.length
    })
  }, [
    selectedRepos.length,
    sourceOptions,
    taskSource,
    hostLabelById,
    taskSourceHostAvailability,
    taskSourceRepoContexts
  ])
  const taskSourceAvailabilityNotice = useMemo(() => {
    const providerLabel =
      sourceOptions.find((source) => source.id === taskSource)?.label ?? taskSource
    return getTaskSourceAvailabilityNotice({
      providerLabel,
      sourceCount: Math.max(1, taskSourceRepoContexts.length),
      hostAvailability: taskSourceHostAvailability,
      hostLabelById
    })
  }, [
    hostLabelById,
    sourceOptions,
    taskSource,
    taskSourceHostAvailability,
    taskSourceRepoContexts.length
  ])
  const githubEmptyState = useMemo(
    () =>
      getRepoBackedTaskEmptyState({
        provider: 'github',
        selectedRepoCount: selectedRepos.length
      }),
    [selectedRepos.length]
  )
  const nextModel = model as typeof model & {
    taskSourceAvailabilityNoticeByProvider: typeof taskSourceAvailabilityNoticeByProvider
    taskSourceContextSummary: typeof taskSourceContextSummary
    taskSourceAvailabilityNotice: typeof taskSourceAvailabilityNotice
    githubEmptyState: typeof githubEmptyState
  }
  nextModel.taskSourceAvailabilityNoticeByProvider = taskSourceAvailabilityNoticeByProvider
  nextModel.taskSourceContextSummary = taskSourceContextSummary
  nextModel.taskSourceAvailabilityNotice = taskSourceAvailabilityNotice
  nextModel.githubEmptyState = githubEmptyState
  return nextModel
}
export type TaskPageSourceSummaryModel = ReturnType<typeof useTaskPageSourceSummary>
