import type { TaskPageGlobalEffectsModel } from './use-task-page-global-effects'
import { shouldHideTaskPageListChrome } from '@/components/task-page-list-chrome-visibility'
export function useTaskPageComposerActions(model: TaskPageGlobalEffectsModel) {
  const { taskSource, dialogWorkItem } = model
  const taskPageListChromeHidden = shouldHideTaskPageListChrome({
    taskSource,
    hasGitHubDetail: Boolean(dialogWorkItem)
  })
  const nextModel = model as typeof model & {
    taskPageListChromeHidden: typeof taskPageListChromeHidden
  }
  nextModel.taskPageListChromeHidden = taskPageListChromeHidden
  return nextModel
}
export type TaskPageComposerActionsModel = ReturnType<typeof useTaskPageComposerActions>
