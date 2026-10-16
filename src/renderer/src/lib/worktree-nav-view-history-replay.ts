import { useAppStore } from '@/store'
import type { WorktreeNavHistoryViewEntry } from '@/store/slices/worktree-nav-history'

// Why: page entries replay via setActiveView (not open*Page) so back/forward doesn't mutate previousViewBefore* or duplicate history (see navigateToIndex).
export function applyWorktreeNavViewEntry(entry: WorktreeNavHistoryViewEntry): void {
  if (entry === 'automations' || entry === 'artifacts') {
    useAppStore.getState().setActiveView(entry)
    return
  }
  if (entry === 'tasks') {
    useAppStore.setState((state) => ({
      activeView: 'tasks',
      githubTaskDrawerWorkItem: null,
      taskPageData: {
        ...state.taskPageData,
        openGitHubWorkItem: undefined,
        openGitHubSourceContext: undefined,
        openGitHubInitialTab: undefined
      }
    }))
    return
  }
  useAppStore.setState((state) => ({
    activeView: 'tasks',
    taskPageData: {
      ...state.taskPageData,
      taskSource: 'github',
      preselectedRepoId: entry.workItem.repoId,
      openGitHubWorkItem: entry.workItem,
      openGitHubSourceContext: entry.sourceContext,
      openGitHubInitialTab: entry.initialTab
    }
  }))
}
