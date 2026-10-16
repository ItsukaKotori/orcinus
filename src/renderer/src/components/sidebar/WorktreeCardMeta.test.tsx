import { renderToStaticMarkup } from 'react-dom/server'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'
import { WorktreeCardDetailsHover } from './WorktreeCardMeta'

vi.mock('@/components/ui/hover-card', () => ({
  HoverCard: ({ children }: { children: ReactNode }) => <>{children}</>,
  HoverCardContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  HoverCardTrigger: ({ children }: { children: ReactNode }) => <>{children}</>
}))

vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: ReactNode }) => <>{children}</>
}))

vi.mock('@/components/ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children }: { children: ReactNode; asChild?: boolean }) => (
    <>{children}</>
  ),
  DropdownMenuContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuItem: ({ children }: { children: ReactNode; onSelect?: () => void }) => (
    <div>{children}</div>
  )
}))

describe('WorktreeCardDetailsHover', () => {
  it('wraps workspace and branch identity so long names stay readable in the hover panel', () => {
    const markup = renderToStaticMarkup(
      <WorktreeCardDetailsHover
        branchName="bug-hold-to-talk-speech-to-text-option-no-longer-works"
        workspaceTitle="[Bug]: Hold-to-talk speech-to-text option no longer works"
        issue={null}
        review={null}
        comment={null}
        onEditIssue={vi.fn()}
        onEditComment={vi.fn()}
      >
        <span>Workspace card</span>
      </WorktreeCardDetailsHover>
    )

    expect(markup).toContain('break-words')
    expect(markup).not.toContain('truncate font-mono')
    expect(markup).not.toContain('truncate text-[13px]')
  })

  it('puts workspace title before branch identity and metadata details', () => {
    const markup = renderToStaticMarkup(
      <WorktreeCardDetailsHover
        branchName="feature/local-branch"
        workspaceTitle="Fix stale GH PR"
        issue={null}
        review={{
          provider: 'github',
          number: 456,
          title: 'Fix stale GH PR',
          state: 'open',
          url: 'https://github.com/acme/orca/pull/456',
          status: 'success',
          updatedAt: '2026-05-17T00:00:00.000Z',
          mergeable: 'MERGEABLE'
        }}
        comment={null}
        onEditIssue={vi.fn()}
        onEditComment={vi.fn()}
      >
        <span>Fix stale GH PR</span>
      </WorktreeCardDetailsHover>
    )

    expect(markup).toContain('feature/local-branch')
    expect(markup.indexOf('Fix stale GH PR')).toBeLessThan(markup.indexOf('feature/local-branch'))
    expect(markup.indexOf('feature/local-branch')).toBeLessThan(markup.indexOf('PR #456'))
  })

  it('keeps the hover title unruled and inline editable while section bodies stay inset', () => {
    const markup = renderToStaticMarkup(
      <WorktreeCardDetailsHover
        branchName="feature/local-branch"
        workspaceTitle="Fix stale GH PR"
        issue={{
          number: 5518,
          title: 'Agent monitor lists ephemeral headless subprocesses',
          state: 'open',
          url: 'https://github.com/acme/orca/issues/5518',
          labels: []
        }}
        review={null}
        comment={null}
        onRenameWorkspaceTitle={vi.fn()}
        onEditIssue={vi.fn()}
        onEditComment={vi.fn()}
      >
        <span>Fix stale GH PR</span>
      </WorktreeCardDetailsHover>
    )
    const identityHeaderTag =
      markup.match(/<div[^>]*data-worktree-hover-identity-header=""[^>]*>/)?.[0] ?? ''

    expect(identityHeaderTag).not.toContain('border-l')
    expect(identityHeaderTag).not.toContain('pl-2')
    expect(markup).toContain('data-worktree-title-inline-rename=""')
    expect(markup).toContain('cursor-text text-[13px] font-semibold')
    expect(markup).toContain('Fix stale GH PR')
    expect(markup).toContain('border-l border-border/70 pl-3')
  })

  it('puts unlink behind the first PR actions menu and keeps GitHub last', () => {
    const markup = renderToStaticMarkup(
      <WorktreeCardDetailsHover
        issue={null}
        review={{
          provider: 'github',
          number: 456,
          title: 'Fix stale GH PR',
          state: 'open',
          url: 'https://github.com/acme/orca/pull/456',
          status: 'success',
          updatedAt: '2026-05-17T00:00:00.000Z',
          mergeable: 'MERGEABLE'
        }}
        comment={null}
        onEditIssue={vi.fn()}
        onEditComment={vi.fn()}
        onOpenReviewInOrca={vi.fn()}
        onOpenReviewInBrowser={vi.fn()}
        onUnlinkReview={vi.fn()}
      >
        <span>Linked PR</span>
      </WorktreeCardDetailsHover>
    )

    const moreActionsIndex = markup.indexOf('aria-label="More PR actions"')
    const openInOrcaIndex = markup.indexOf('aria-label="Open in Orca"')
    const viewOnGitHubIndex = markup.indexOf('aria-label="View on GitHub"')

    expect(moreActionsIndex).toBeGreaterThan(-1)
    expect(markup).toContain('More PR actions')
    expect(markup).toContain('Copy link')
    expect(markup).toContain('Unlink PR from workspace')
    expect(markup).toContain(
      'Orca will hide PR #456 details for this workspace. The PR and branch on GitHub won’t be changed.'
    )
    expect(markup).toContain('Open in browser')
    expect(moreActionsIndex).toBeLessThan(openInOrcaIndex)
    expect(openInOrcaIndex).toBeLessThan(viewOnGitHubIndex)
    expect(markup.indexOf('Open in browser')).toBeLessThan(markup.indexOf('Copy link'))
    expect(markup.indexOf('Copy link')).toBeLessThan(markup.indexOf('Unlink PR from workspace'))
    expect(markup).not.toContain('aria-label="Unlink PR from workspace"')
  })

  it('puts issue copy menu before edit and open actions and keeps GitHub last', () => {
    const markup = renderToStaticMarkup(
      <WorktreeCardDetailsHover
        issue={{
          number: 5518,
          title: 'Agent monitor lists ephemeral headless subprocesses',
          state: 'closed',
          url: 'https://github.com/acme/orca/issues/5518',
          labels: []
        }}
        review={null}
        comment={null}
        onEditIssue={vi.fn()}
        onEditComment={vi.fn()}
        onOpenGitHubIssueInOrca={vi.fn()}
        onOpenIssueInBrowser={vi.fn()}
      >
        <span>Linked issue</span>
      </WorktreeCardDetailsHover>
    )

    const moreActionsIndex = markup.indexOf('aria-label="More issue actions"')
    const copyLinkIndex = markup.indexOf('Copy link')
    const editIssueIndex = markup.indexOf('aria-label="Edit issue"')
    const openInOrcaIndex = markup.indexOf('aria-label="Open in Orca"')
    const viewOnGitHubIndex = markup.indexOf('aria-label="View on GitHub"')

    expect(moreActionsIndex).toBeGreaterThan(-1)
    expect(copyLinkIndex).toBeGreaterThan(-1)
    expect(markup).toContain('Open in browser')
    expect(markup.indexOf('Open in browser')).toBeLessThan(copyLinkIndex)
    expect(editIssueIndex).toBeGreaterThan(-1)
    expect(moreActionsIndex).toBeLessThan(editIssueIndex)
    expect(copyLinkIndex).toBeLessThan(editIssueIndex)
    expect(editIssueIndex).toBeLessThan(openInOrcaIndex)
    expect(openInOrcaIndex).toBeLessThan(viewOnGitHubIndex)
  })

  it('hides the embedded-browser action when a linked review has no URL', () => {
    const markup = renderToStaticMarkup(
      <WorktreeCardDetailsHover
        issue={null}
        review={{
          provider: 'github',
          number: 456,
          title: 'Loading PR...',
          state: 'open'
        }}
        comment={null}
        onOpenReviewInBrowser={vi.fn()}
      >
        <span>Linked PR</span>
      </WorktreeCardDetailsHover>
    )

    expect(markup).not.toContain('Open in browser')
  })

  it('keeps the embedded-browser action provider-neutral for unsupported review URLs', () => {
    const markup = renderToStaticMarkup(
      <WorktreeCardDetailsHover
        issue={null}
        review={{
          provider: 'unsupported',
          number: 12,
          title: 'Review from an unsupported provider',
          state: 'open',
          url: 'https://code.example.test/reviews/12',
          status: 'neutral',
          updatedAt: '2026-05-17T00:00:00.000Z',
          mergeable: 'UNKNOWN'
        }}
        comment={null}
        onOpenReviewInBrowser={vi.fn()}
      >
        <span>Linked review</span>
      </WorktreeCardDetailsHover>
    )

    expect(markup).toContain('Open in browser')
  })

})
