import type { TaskPageRuntimeHostsModel } from './use-task-page-runtime-hosts'
import { useCallback, useMemo } from 'react'
import type { Repo } from '../../../shared/repo-types'
import {
  getTaskPageRepoSourceContext,
  getTaskSourceHostAvailabilityForHost
} from './task-page-source-context'
import type { TaskSourceHostAvailability } from './task-source-context-summary'
import { getRepoBackedProviderAvailability } from '@/components/task-source-provider-availability'
import { getSettingsFocusedExecutionHostId } from '../../../shared/execution-host'
import { type TaskSourceContext } from '../../../shared/task-source-context'
import { useTaskPageSourceSummary } from './use-task-page-source-summary'
export type TaskPageSourceAvailabilityPreludeModel = ReturnType<
  typeof useTaskPageSourceAvailabilityPrelude
>
export function useTaskPageSourceAvailabilityPrelude(model: TaskPageRuntimeHostsModel) {
  const {
    settings,
    preflightStatus,
    preflightStatusChecked,
    preflightStatusCurrent,
    selectedRepos,
    taskSource,
    runtimePreflightStatusByHostId,
    taskSourceRepoContexts,
    hostRegistryById
  } = model
  const getTaskPickerRepoHostLabel = useCallback(
    (repo: Repo): string | null => {
      const context = getTaskPageRepoSourceContext(repo, 'github')
      const hostId = context?.hostId ?? repo.executionHostId ?? 'local'
      return hostRegistryById.get(hostId)?.label ?? null
    },
    [hostRegistryById]
  )
  const taskSourceHostAvailability = useMemo<TaskSourceHostAvailability[]>(() => {
    if (taskSource !== 'github') {
      return []
    }
    return [
      ...taskSourceRepoContexts.flatMap((context) => {
        const host = hostRegistryById.get(context.hostId)
        const availability = getTaskSourceHostAvailabilityForHost(host, context.hostId)
        return availability ? [availability] : []
      }),
      ...getRepoBackedProviderAvailability({
        provider: taskSource,
        contexts: taskSourceRepoContexts,
        preflightStatus,
        preflightReady: preflightStatusCurrent && preflightStatusChecked,
        runtimePreflightStatusByHostId
      })
    ]
  }, [
    hostRegistryById,
    preflightStatus,
    preflightStatusChecked,
    preflightStatusCurrent,
    runtimePreflightStatusByHostId,
    taskSource,
    taskSourceRepoContexts
  ])
  const accountBackedTaskSourceHostId = useMemo(
    () => getSettingsFocusedExecutionHostId(settings),
    [settings]
  )
  const fallbackTaskSourceProjectId = useMemo(() => {
    const firstRepoContext = selectedRepos
      .map((repo) => getTaskPageRepoSourceContext(repo, 'github'))
      .find((context): context is TaskSourceContext => context !== null)
    return firstRepoContext?.projectId ?? 'account-backed-task-source'
  }, [selectedRepos])
  const nextModel = model as typeof model & {
    getTaskPickerRepoHostLabel: typeof getTaskPickerRepoHostLabel
    taskSourceHostAvailability: typeof taskSourceHostAvailability
    accountBackedTaskSourceHostId: typeof accountBackedTaskSourceHostId
    fallbackTaskSourceProjectId: typeof fallbackTaskSourceProjectId
  }
  nextModel.getTaskPickerRepoHostLabel = getTaskPickerRepoHostLabel
  nextModel.taskSourceHostAvailability = taskSourceHostAvailability
  nextModel.accountBackedTaskSourceHostId = accountBackedTaskSourceHostId
  nextModel.fallbackTaskSourceProjectId = fallbackTaskSourceProjectId
  return nextModel
}
export function useTaskPageSourceAvailability(model: TaskPageRuntimeHostsModel) {
  const preludeModel = useTaskPageSourceAvailabilityPrelude(model)
  return useTaskPageSourceSummary(preludeModel)
}
export type TaskPageSourceAvailabilityModel = ReturnType<typeof useTaskPageSourceAvailability>
