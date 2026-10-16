import React from 'react'
import { useTaskPageStoreBindings } from '../use-task-page-store-bindings'
import { useTaskPageRepoSelection } from '../use-task-page-repo-selection'
import { useTaskPageRuntimeHosts } from '../use-task-page-runtime-hosts'
import { useTaskPageSourceAvailability } from '../use-task-page-source-availability'
import { useTaskPageProviderState } from '../use-task-page-provider-state'
import { useTaskPageGitHubListState } from '../use-task-page-github-list-state'
import { useTaskPageGitHubDetail } from '../use-task-page-github-detail'
import { useTaskPageGitHubCacheReconciliation } from '../use-task-page-github-cache-reconciliation'
import { useTaskPageGitHubIssueDraft } from '../use-task-page-github-issue-draft'
import { useTaskPageDetailRouting } from '../use-task-page-detail-routing'
import { useTaskPageResumeRestoration } from '../use-task-page-resume-restoration'
import { useTaskPageGitHubMutationState } from '../use-task-page-github-mutation-state'
import { useTaskPageGitHubListProjection } from '../use-task-page-github-list-projection'
import { useTaskPageGitHubSearchPagination } from '../use-task-page-github-search-pagination'
import { useTaskPageGitHubLandingRefresh } from '../use-task-page-github-landing-refresh'
import { useTaskPageGitHubQuietRefresh } from '../use-task-page-github-quiet-refresh'
import { useTaskPageSearchActions } from '../use-task-page-search-actions'
import { useTaskPageWorkspaceActions } from '../use-task-page-workspace-actions'
import { useTaskPageGitHubIssueCreation } from '../use-task-page-github-issue-creation'
import { useTaskPageGlobalEffects } from '../use-task-page-global-effects'
import { useTaskPageComposerActions } from '../use-task-page-composer-actions'
import { TaskPageSurface } from './Surface'

export default function TaskPage(): React.JSX.Element {
  const stage1 = useTaskPageStoreBindings()
  const stage2 = useTaskPageRepoSelection(stage1)
  const stage3 = useTaskPageRuntimeHosts(stage2)
  const stage4 = useTaskPageSourceAvailability(stage3)
  const stage5 = useTaskPageProviderState(stage4)
  const stage6 = useTaskPageGitHubListState(stage5)
  const stage7 = useTaskPageGitHubDetail(stage6)
  const stage8 = useTaskPageGitHubCacheReconciliation(stage7)
  const stage9 = useTaskPageGitHubIssueDraft(stage8)
  const stage10 = useTaskPageDetailRouting(stage9)
  const stage11 = useTaskPageResumeRestoration(stage10)
  const stage12 = useTaskPageGitHubMutationState(stage11)
  const stage13 = useTaskPageGitHubListProjection(stage12)
  const stage14 = useTaskPageGitHubSearchPagination(stage13)
  const stage15 = useTaskPageGitHubLandingRefresh(stage14)
  const stage16 = useTaskPageGitHubQuietRefresh(stage15)
  const stage17 = useTaskPageSearchActions(stage16)
  const stage18 = useTaskPageWorkspaceActions(stage17)
  const stage19 = useTaskPageGitHubIssueCreation(stage18)
  const stage20 = useTaskPageGlobalEffects(stage19)
  const stage21 = useTaskPageComposerActions(stage20)
  return <TaskPageSurface model={stage21} />
}
