import type { TaskPageGitHubIssueDraftModel } from './use-task-page-github-issue-draft'
import { useCallback } from 'react'
import { useAppStore } from '@/store'
export function useTaskPageDetailRouting(model: TaskPageGitHubIssueDraftModel) {
  const { setDialogWorkItem } = model
  const closeTaskDetailPage = useCallback(() => {
    const state = useAppStore.getState()
    const currentEntry = state.worktreeNavHistory[state.worktreeNavHistoryIndex]
    if (
      typeof currentEntry === 'object' &&
      currentEntry.kind === 'task-detail' &&
      state.worktreeNavHistoryIndex > 0
    ) {
      state.goBackWorktree()
      return
    }
    setDialogWorkItem(null)
    useAppStore.setState((s) => ({
      taskPageData: {
        ...s.taskPageData,
        openGitHubWorkItem: undefined,
        openGitHubSourceContext: undefined,
        openGitHubInitialTab: undefined
      }
    }))
  }, [setDialogWorkItem])
  const nextModel = model as typeof model & {
    closeTaskDetailPage: typeof closeTaskDetailPage
  }
  nextModel.closeTaskDetailPage = closeTaskDetailPage
  return nextModel
}
export type TaskPageDetailRoutingModel = ReturnType<typeof useTaskPageDetailRouting>
