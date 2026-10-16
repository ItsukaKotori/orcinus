import type { PRCheckDetail, PRCheckRunDetails } from '../../../../shared/github/check-types'
import type { GitHubRepositoryIdentity } from '../../../../shared/github/pull-request-types'

export type OpenCheckRunDetailsState = {
  contextKey: string
  check: PRCheckDetail
  requestId?: number
  details: PRCheckRunDetails | null
  loading: boolean
  error: string | null
  githubRepository?: GitHubRepositoryIdentity | null
}

export type CheckRunDetailsTabPatch = Pick<
  OpenCheckRunDetailsState,
  'requestId' | 'details' | 'loading' | 'error' | 'githubRepository'
>

let nextCheckRunDetailsRequestId = 0

export function createCheckRunDetailsRequestId(): number {
  nextCheckRunDetailsRequestId += 1
  return nextCheckRunDetailsRequestId
}

export function isSameGitHubRepository(
  a: GitHubRepositoryIdentity | null,
  b: GitHubRepositoryIdentity | null
): boolean {
  return a === b || (a?.owner === b?.owner && a?.repo === b?.repo && a?.host === b?.host)
}

export function getCheckRunTabIdentity(check: PRCheckDetail): string {
  if (check.checkRunId) {
    return `check-run:${check.checkRunId}`
  }
  if (check.workflowRunId) {
    return `workflow-run:${check.workflowRunId}`
  }
  if (check.url) {
    return `url:${check.url}`
  }
  return `name:${check.name}`
}

export function buildCheckRunDetailsTabId(worktreeId: string, check: PRCheckDetail): string {
  // Why: one tab per hosted check identity keeps the center pane stable across
  // PR head refreshes; contextKey lives on the tab state instead of the tab id.
  return `${worktreeId}::check-details::${getCheckRunTabIdentity(check)}`
}

export function getCheckRunDetailsTabLabel(check: PRCheckDetail): string {
  return check.name
}
