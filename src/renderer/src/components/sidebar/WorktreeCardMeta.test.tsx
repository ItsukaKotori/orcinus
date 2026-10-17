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
        comment={null}
        onEditComment={vi.fn()}
      >
        <span>Workspace card</span>
      </WorktreeCardDetailsHover>
    )

    expect(markup).toContain('break-words')
    expect(markup).not.toContain('truncate font-mono')
    expect(markup).not.toContain('truncate text-[13px]')
  })

})
