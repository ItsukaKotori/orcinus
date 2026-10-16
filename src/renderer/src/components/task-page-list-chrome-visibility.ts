import type { TaskProvider } from '../../../shared/task-providers'

export type TaskPageListChromeVisibilityState = {
  taskSource: TaskProvider
  hasGitHubDetail: boolean
}

export function shouldHideTaskPageListChrome({
  taskSource,
  hasGitHubDetail
}: TaskPageListChromeVisibilityState): boolean {
  // Why: provider-specific selection can intentionally survive source switches;
  // stale detail state from another provider must not hide the active list chrome.
  return taskSource === 'github' ? hasGitHubDetail : false
}
