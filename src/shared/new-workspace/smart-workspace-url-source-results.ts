import { parseGitHubIssueOrPRLink, type GitHubIssueOrPRLink } from '../github/links'
import { githubRepoIdentityKey } from '../github/repository-identity-key'
import type { GitHubWorkItem } from '../github/work-item-types'

type SmartWorkspaceUrlSourceMode = 'smart' | 'github' | 'branches' | 'text'

export type SmartWorkspaceUrlSourceRow =
  | { kind: 'use-name'; value: 'use-name'; name: string }
  | { kind: 'github'; value: string; item: GitHubWorkItem }

function toGitHubSourceRow(item: GitHubWorkItem): SmartWorkspaceUrlSourceRow {
  return { kind: 'github', value: `github-${item.repoId}-${item.type}-${item.number}`, item }
}

function withSmartNameFallback(
  mode: SmartWorkspaceUrlSourceMode,
  name: string,
  rows: SmartWorkspaceUrlSourceRow[]
): SmartWorkspaceUrlSourceRow[] {
  return name && mode === 'smart' ? [{ kind: 'use-name', value: 'use-name', name }, ...rows] : rows
}

function isGitHubLinkIntentMatch(intent: GitHubIssueOrPRLink, item: GitHubWorkItem): boolean {
  const itemLink = parseGitHubIssueOrPRLink(item.url)
  return (
    itemLink !== null &&
    itemLink.type === intent.type &&
    itemLink.number === intent.number &&
    githubRepoIdentityKey(itemLink.slug) === githubRepoIdentityKey(intent.slug)
  )
}

export function buildSmartWorkspaceUrlSourceRows({
  githubItems,
  githubUrlIntent,
  mode,
  resultLimit,
  value
}: {
  githubItems: GitHubWorkItem[]
  githubUrlIntent?: GitHubIssueOrPRLink | null
  mode: SmartWorkspaceUrlSourceMode
  resultLimit: number
  value: string
}): SmartWorkspaceUrlSourceRow[] | null {
  const trimmed = value.trim()
  if (githubUrlIntent && (mode === 'smart' || mode === 'github')) {
    const rows = githubItems
      .filter((item) => isGitHubLinkIntentMatch(githubUrlIntent, item))
      .map(toGitHubSourceRow)
      .slice(0, resultLimit)
    return withSmartNameFallback(mode, trimmed, rows)
  }
  return null
}
