import type { GitStatusEntry, GitUpstreamStatus } from '../../shared/git-status-types'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { createGhExecClient, defaultGhExecutor } from '@/lib/github/gh-exec-client'
import { createHostedReviewClient } from '@/lib/github/hosted-review'
import { createHostedReviewCreation } from '@/lib/github/hosted-review-create'
import { createRunGit, defaultGitReadExecutor } from '@/lib/github/git-read-client'
import { createPRForBranchLookup } from '@/lib/github/pr-for-branch'
import { createRepoIdentityResolver } from '@/lib/github/repo-identity'
import { invokeCommand } from './invoke'

// `fs_read_file` answers image payloads as base64 content; a PR template must
// be text, so anything image-shaped (or missing content) degrades to "no
// template" and the factory moves on to the next conventional path.
type TemplateFileContent = {
  content: string
  isBinary: boolean
  isImage?: boolean | null
}

function extractTextOrNull(
  file: TemplateFileContent
): { content: string; isBinary: boolean } | null {
  if (file.isImage === true || typeof file.content !== 'string') {
    return null
  }
  return { content: file.content, isBinary: file.isBinary }
}

/**
 * Real `hostedReview` adapter (spec §3.2): 2D.1 ports the read-only `forBranch`
 * poll through the GitHub PR lookup; 2D.2 wires eligibility and creation
 * through `createHostedReviewCreation` — git probes via `git_read`, status and
 * upstream via their bridge commands, templates via `fs_read_file`. Stacked
 * creation stays on the rejecting method fallback.
 */
export function createHostedReviewRealApi(): PreloadApi['hostedReview'] {
  const client = createGhExecClient(defaultGhExecutor())
  const readRemoteUrls = (worktreePath: string) =>
    invokeCommand<{ name: string; url: string }[]>('git_remote_urls', {
      args: { worktreePath }
    })
  const identity = createRepoIdentityResolver({ client, readRemoteUrls })
  const lookup = createPRForBranchLookup({ client, identity })
  const reviewLookup = createHostedReviewClient({ identity, lookup })
  const creation = createHostedReviewCreation({
    client,
    identity,
    reviewLookup,
    makeRunGit: (worktreePath) => createRunGit(defaultGitReadExecutor(worktreePath)),
    readStatus: (worktreePath) =>
      invokeCommand<{ entries: readonly GitStatusEntry[] }>('git_status', {
        args: { worktreePath }
      }),
    readUpstream: (worktreePath) =>
      invokeCommand<GitUpstreamStatus>('git_upstream_status', {
        args: { worktreePath }
      }),
    readTemplate: async (worktreePath, relativePath) => {
      try {
        const file = await invokeCommand<TemplateFileContent>('fs_read_file', {
          args: { filePath: `${worktreePath}/${relativePath}` }
        })
        return extractTextOrNull(file)
      } catch {
        // A missing/unreadable template is not a create failure; the factory
        // tries the remaining conventional paths and falls back to an empty body.
        return null
      }
    }
  })

  return withMethodFallback<PreloadApi['hostedReview']>('hostedReview', {
    forBranch: (args) => reviewLookup.forBranch(args),
    getCreationEligibility: (args) => creation.getCreationEligibility(args),
    create: (args) => creation.create(args)
  })
}
