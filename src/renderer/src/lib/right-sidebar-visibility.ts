import type { AppState } from '@/store/types'
import { getIndexedRepoMap, getIndexedWorktreeMap } from '@/store/worktree-repo-index'
import { isFolderRepo } from '../../../shared/repo-kind'

type ActiveView = AppState['activeView']

const RIGHT_SIDEBAR_SUPPRESSED_VIEWS = new Set<ActiveView>([
  'settings',
  'tasks',
  'activity',
  'automations',
  'space',
  'artifacts',
  'plugin-center'
])

const PLUGIN_HOSTED_VIEW_PREFIX = 'plugin:'

function isPluginHostedView(activeView: ActiveView): boolean {
  return activeView.startsWith(PLUGIN_HOSTED_VIEW_PREFIX)
}

export function canShowRightSidebarForView(activeView: ActiveView): boolean {
  return !isPluginHostedView(activeView) && !RIGHT_SIDEBAR_SUPPRESSED_VIEWS.has(activeView)
}

export function rightSidebarShowsPullRequestData(
  state: Pick<
    AppState,
    | 'activeView'
    | 'activeWorktreeId'
    | 'repos'
    | 'rightSidebarOpen'
    | 'rightSidebarTab'
    | 'worktreesByRepo'
  >
): boolean {
  if (
    !canShowRightSidebarForView(state.activeView) ||
    !state.rightSidebarOpen ||
    (state.rightSidebarTab !== 'checks' && state.rightSidebarTab !== 'source-control')
  ) {
    return false
  }

  const activeWorktree = state.activeWorktreeId
    ? getIndexedWorktreeMap(state.worktreesByRepo).get(state.activeWorktreeId)
    : undefined
  const activeRepo = activeWorktree
    ? getIndexedRepoMap(state.repos).get(activeWorktree.repoId)
    : null
  if (!activeRepo || isFolderRepo(activeRepo)) {
    return false
  }

  return true
}
