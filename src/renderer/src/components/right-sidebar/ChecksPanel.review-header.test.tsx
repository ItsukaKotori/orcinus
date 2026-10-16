import type { ReactNode } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChecksPanelReviewHeader } from './ChecksPanel'

vi.mock('@/components/ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children }: { children: ReactNode; asChild?: boolean }) => (
    <>{children}</>
  ),
  DropdownMenuContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuItem: ({
    children,
    disabled
  }: {
    children: ReactNode
    disabled?: boolean
    onSelect?: () => void
  }) => <div data-disabled={disabled ? 'true' : undefined}>{children}</div>
}))

vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: ReactNode }) => <>{children}</>
}))

beforeEach(() => {
  vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function renderHeader({ canUnlinkReview = true }: { canUnlinkReview?: boolean } = {}): string {
  return renderToStaticMarkup(
    <ChecksPanelReviewHeader
      review={{
        provider: 'github',
        number: 2964,
        title: 'fix: pr-bug-scan validated finding',
        state: 'open',
        url: 'https://github.com/stablyai/orca/pull/2964',
        status: 'pending',
        updatedAt: '2026-05-31T22:58:01Z',
        mergeable: 'UNKNOWN'
      }}
      isRefreshing={false}
      canUnlinkReview={canUnlinkReview}
      onRefresh={vi.fn()}
      onOpenReview={vi.fn()}
      onUnlinkReview={vi.fn()}
      onLinkAnotherReview={vi.fn()}
    />
  )
}

describe('ChecksPanelReviewHeader', () => {
  it('opens the PR from the number and puts link management behind the menu', () => {
    const markup = renderHeader()

    expect(markup).toContain('Open on GitHub')
    expect(markup).toContain('#2964')
    expect(markup).toContain('underline decoration-border underline-offset-2')
    expect(markup).toContain('More PR actions')
    expect(markup).toContain('Unlink PR from workspace')
    expect(markup).toContain(
      'Orca will hide PR #2964 details for this workspace. The PR and branch on GitHub won’t be changed.'
    )
    expect(markup).toContain('Link another PR')
    expect(markup).toContain('lucide-ellipsis')
    expect(markup).not.toContain('lucide-external-link')
  })

  it('enables unlinking for a displayed auto-detected PR', () => {
    const markup = renderHeader({ canUnlinkReview: true })

    expect(markup).not.toContain('data-disabled="true"')
    expect(markup).toContain('Unlink PR from workspace')
  })
})
