import { renderToStaticMarkup } from 'react-dom/server'
import React, { type ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DashboardAgentRow as DashboardAgentRowData } from '@/components/dashboard/useDashboardData'
import type { GlobalSettings } from '../../../../shared/global-settings-types'
import type { Repo } from '../../../../shared/repo-types'
import type { WorktreeCardProperty } from '../../../../shared/ui-chrome-types'
import type { Worktree } from '../../../../shared/worktree/types'

const fetchHostedReviewForBranch = vi.fn()
const fetchIssue = vi.fn()
const fetchLinearIssue = vi.fn()
const openModal = vi.fn()
const openTaskPage = vi.fn()
const updateWorktreeMeta = vi.fn()
const recordFeatureInteraction = vi.fn()
const replaceWorkspacePortScans = vi.fn()
const setWorkspacePortScanRefreshing = vi.fn()
const cacheTimerMocks = vi.hoisted(() => ({
  usePromptCacheCountdownStartedAt: vi.fn()
}))

let worktreeCardProperties: WorktreeCardProperty[] = ['status']
let hostedReviewCache: Record<string, unknown> = {}
let issueCache: Record<string, unknown> = {}
let projectGroups: unknown[] = []
let settings: Partial<GlobalSettings> | null = { compactWorktreeCards: true }
let agentActivityDisplayMode: 'compact' | 'full' | undefined
let mockInlineAgentRows: DashboardAgentRowData[] = []

vi.mock('@/store', () => ({
  useAppStore: (selector: (state: unknown) => unknown) =>
    selector({
      browserTabsByWorktree: {},
      agentActivityDisplayMode,
      createBrowserTab: vi.fn(),
      deleteStateByWorktreeId: {},
      fetchHostedReviewForBranch,
      fetchIssue,
      fetchLinearIssue,
      gitConflictOperationByWorktree: {},
      hostedReviewCache,
      issueCache,
      linearIssueCache: {},
      openModal,
      openTaskPage,
      projectGroups,
      ptyIdsByTabId: {},
      recordFeatureInteraction,
      remoteBranchConflictByWorktreeId: {},
      setRemoteBrowserPageHandle: vi.fn(),
      replaceWorkspacePortScans,
      setWorkspacePortScanRefreshing,
      settings,
      sshConnectionStates: new Map(),
      sshTargetLabels: new Map(),
      tabsByWorktree: {},
      updateWorktreeMeta,
      worktreeCardProperties
    })
}))

vi.mock('@/components/ui/hover-card', () => ({
  HoverCard: ({ children, openDelay }: { children: ReactNode; openDelay?: number }) => (
    <div data-hover-open-delay={openDelay}>{children}</div>
  ),
  HoverCardContent: ({ children }: { children: ReactNode }) => (
    <div data-hover-card-content="">{children}</div>
  ),
  HoverCardTrigger: ({ children }: { children: ReactNode }) =>
    React.isValidElement(children) ? (
      React.cloneElement(children as React.ReactElement<Record<string, unknown>>, {
        'data-hover-card-trigger': ''
      })
    ) : (
      <>{children}</>
    )
}))

vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: ReactNode }) => <>{children}</>
}))

vi.mock('@/lib/sidebar-worktree-activation', () => ({
  activateWorktreeFromSidebar: vi.fn()
}))

vi.mock('@/runtime/runtime-rpc-client', () => ({
  getActiveRuntimeTarget: () => ({ kind: 'local' })
}))

vi.mock('./use-worktree-activity-status', () => ({
  useWorktreeActivityStatus: () => 'active'
}))

vi.mock('./CacheTimer', () => ({
  default: () => null,
  usePromptCacheCountdownStartedAt: cacheTimerMocks.usePromptCacheCountdownStartedAt
}))

vi.mock('./useWorktreeAgentRows', () => ({
  useWorktreeAgentRows: vi.fn(() => mockInlineAgentRows)
}))

vi.mock('./WorktreeCardAgents', () => ({
  default: ({ className, agents }: { className?: string; agents?: DashboardAgentRowData[] }) => (
    <div className={className} data-agent-count={agents?.length ?? ''} data-worktree-agents="" />
  )
}))

vi.mock('./WorktreeContextMenu', () => ({
  default: ({ children }: { children: ReactNode }) => <>{children}</>,
  CLOSE_ALL_CONTEXT_MENUS_EVENT: 'orca:test-close-context-menus',
  WORKTREE_CONTEXT_MENU_SCOPE_ATTR: 'data-orca-context-menu-scope',
  WORKTREE_NATIVE_CONTEXT_MENU_ATTR: 'data-worktree-native-context-menu'
}))

function makeRepo(): Repo {
  return {
    id: 'repo-1',
    path: '/repo',
    displayName: 'orca',
    badgeColor: '#999999',
    addedAt: 1
  }
}

function makeWorktree(overrides: Partial<Worktree> = {}): Worktree {
  return {
    id: 'repo-1::/repo/worktrees/pr-456',
    repoId: 'repo-1',
    path: '/repo/worktrees/pr-456',
    displayName: 'Fix stale GH PR',
    branch: 'feature/local-branch',
    head: 'abc123',
    isBare: false,
    isMainWorktree: false,
    comment: '',
    linkedIssue: null,
    linkedPR: null,
    linkedLinearIssue: null,
    isArchived: false,
    isUnread: false,
    isPinned: false,
    sortOrder: 0,
    lastActivityAt: 1,
    ...overrides
  }
}

function expectIdentityBodyIsHoverTrigger(markup: string): void {
  const surfaceTag = markup.match(/<div[^>]*data-worktree-card-surface="true"[^>]*>/)?.[0]
  const triggerTag = markup.match(/<div[^>]*data-worktree-card-hover-trigger=""[^>]*>/)?.[0]

  expect(surfaceTag).toBeDefined()
  expect(surfaceTag).not.toContain('data-hover-card-trigger=""')
  expect(surfaceTag).not.toContain('group/worktree-card')
  expect(triggerTag).toBeDefined()
  expect(triggerTag).toContain('data-hover-card-trigger=""')
  expect(triggerTag).toContain('group/worktree-card')
}

describe('WorktreeCard compact hover details', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    worktreeCardProperties = ['status']
    hostedReviewCache = {}
    issueCache = {}
    projectGroups = []
    settings = { compactWorktreeCards: true }
    agentActivityDisplayMode = undefined
    mockInlineAgentRows = []
    cacheTimerMocks.usePromptCacheCountdownStartedAt.mockReturnValue(null)
  })

  it('uses one identity hover even when detailed metadata icons are visible when new card style is on', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'issue', 'comment']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({
          linkedIssue: 123,
          linkedPR: 456,
          comment: 'Reviewer handoff note'
        })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expect(markup).toContain('Workspace metadata')
    expect(markup).not.toContain('data-worktree-card-meta-row=""')
    expectIdentityBodyIsHoverTrigger(markup)
    expect(markup.match(/data-hover-open-delay="100"/g)).toHaveLength(1)
    expect(markup).toContain('Reviewer handoff note')
  })

  it('keeps long workspace and branch identity in hover details when the branch row is hidden', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'comment']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({
          branch: 'bug-hold-to-talk-speech-to-text-option-no-longer-works',
          displayName: '[Bug]: Hold-to-talk speech-to-text option no longer works',
          comment: 'Reviewer handoff note'
        })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expect(markup).not.toContain('data-worktree-card-meta-row=""')
    expectIdentityBodyIsHoverTrigger(markup)
    expect(markup).toContain('[Bug]: Hold-to-talk speech-to-text option no longer works')
    expect(markup).toContain('bug-hold-to-talk-speech-to-text-option-no-longer-works')
    expect(markup).toContain('Reviewer handoff note')
  })

  it('repeats a long workspace title inside the identity hover when branch is already visible', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'branch', 'comment']
    const longTitle =
      'Investigate why the worktree hover card title disappears behind single-line truncation'
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ displayName: longTitle, comment: 'Reviewer handoff note' })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expectIdentityBodyIsHoverTrigger(markup)
    expect(markup.match(new RegExp(longTitle, 'g'))).toHaveLength(2)
    expect(markup).toContain('feature/local-branch')
    expect(markup).toContain('Reviewer handoff note')
  })

  it('uses identity hover for identity-only new card worktrees with branch row visible', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'branch']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ displayName: 'Readable identity only' })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expect(markup).toContain('data-worktree-card-meta-row=""')
    expectIdentityBodyIsHoverTrigger(markup)
    expect(markup.match(/data-hover-open-delay="100"/g)).toHaveLength(1)
    expect(markup.match(/Readable identity only/g)).toHaveLength(2)
    expect(markup).toContain('feature/local-branch')
    expect(markup).not.toContain('Workspace metadata')
    expect(markup).not.toContain('Live Ports')
  })

  it('does not duplicate workspace identity when trimmed title equals branch', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'branch']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ displayName: '  feature/local-branch  ' })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expectIdentityBodyIsHoverTrigger(markup)
    expect(markup.match(/feature\/local-branch/g)).toHaveLength(3)
  })

  it('keeps detailed metadata hover scoped to metadata icons by default', async () => {
    settings = { compactWorktreeCards: false }
    worktreeCardProperties = ['status', 'issue', 'comment']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({
          linkedIssue: 123,
          linkedPR: 456,
          comment: 'Reviewer handoff note'
        })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expect(markup).toContain('data-worktree-card-meta-row=""')
    const surfaceTag = markup.match(/<div[^>]*data-worktree-card-surface="true"[^>]*>/)?.[0]
    expect(surfaceTag).toBeDefined()
    expect(surfaceTag).not.toContain('data-hover-card-trigger=""')
    expect(markup.match(/data-hover-open-delay="250"/g)).toHaveLength(1)
    expect(markup).toContain('Reviewer handoff note')
  })

  it('keeps child card markup inside the parent card by default', async () => {
    settings = { compactWorktreeCards: false }
    worktreeCardProperties = ['status', 'comment']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ comment: 'Parent note' })}
        repo={makeRepo()}
        isActive={false}
        lineageChildren={<div data-lineage-child-card="">Child card</div>}
      />
    )
    const childIndex = markup.indexOf('data-lineage-child-card=""')

    const surfaceTag = markup.match(/<div[^>]*data-worktree-card-surface="true"[^>]*>/)?.[0]
    expect(surfaceTag).toBeDefined()
    expect(surfaceTag).not.toContain('data-hover-card-trigger=""')
    expect(markup).not.toContain('data-worktree-lineage-children=""')
    expect(childIndex).toBeGreaterThanOrEqual(0)
  })

  it('suppresses inline agent rows in compact cards by default', async () => {
    settings = { compactWorktreeCards: true }
    worktreeCardProperties = ['status', 'inline-agents']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard worktree={makeWorktree()} repo={makeRepo()} isActive={false} />
    )

    expect(markup).not.toContain('data-worktree-agents')
  })

  it('does not create a compact metadata row solely for an aggregate cache timer', async () => {
    settings = { compactWorktreeCards: true }
    worktreeCardProperties = ['status']
    cacheTimerMocks.usePromptCacheCountdownStartedAt.mockImplementation(
      (_worktreeId: string, active = true) => (active ? 10_000 : null)
    )
    const worktree = makeWorktree()
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard worktree={worktree} repo={makeRepo()} isActive={false} />
    )

    expect(cacheTimerMocks.usePromptCacheCountdownStartedAt).toHaveBeenCalledWith(
      worktree.id,
      false
    )
    expect(markup).not.toContain('data-worktree-card-meta-row=""')
  })

  it('suppresses the aggregate cache timer when compact inline agents are visible', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'inline-agents']
    agentActivityDisplayMode = 'compact'
    mockInlineAgentRows = [{} as DashboardAgentRowData]
    const worktree = makeWorktree()
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard worktree={worktree} repo={makeRepo()} isActive={false} />
    )

    expect(markup).toContain('data-worktree-agents=""')
    expect(cacheTimerMocks.usePromptCacheCountdownStartedAt).toHaveBeenCalledWith(
      worktree.id,
      false
    )
  })

  it('keeps status and agent tooltip targets outside the worktree details hover trigger', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'inline-agents']
    agentActivityDisplayMode = 'compact'
    mockInlineAgentRows = [{} as DashboardAgentRowData]
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard worktree={makeWorktree()} repo={makeRepo()} isActive={false} />
    )
    const statusIndex = markup.indexOf('data-worktree-card-status-slot=""')
    const triggerIndex = markup.indexOf('data-worktree-card-hover-trigger=""')
    const hoverContentIndex = markup.indexOf('data-hover-card-content=""')
    const agentsIndex = markup.indexOf('data-worktree-agents=""')

    expectIdentityBodyIsHoverTrigger(markup)
    expect(statusIndex).toBeGreaterThanOrEqual(0)
    expect(statusIndex).toBeLessThan(triggerIndex)
    expect(hoverContentIndex).toBeGreaterThan(triggerIndex)
    expect(agentsIndex).toBeGreaterThan(hoverContentIndex)
  })

  it('preserves the aggregate cache timer when compact inline agents are enabled but absent', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'inline-agents']
    agentActivityDisplayMode = 'compact'
    mockInlineAgentRows = []
    cacheTimerMocks.usePromptCacheCountdownStartedAt.mockImplementation(
      (_worktreeId: string, active = true) => (active ? 10_000 : null)
    )
    const worktree = makeWorktree()
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard worktree={worktree} repo={makeRepo()} isActive={false} />
    )

    expect(markup).toContain('data-worktree-agents=""')
    expect(markup).toContain('data-agent-count="0"')
    expect(cacheTimerMocks.usePromptCacheCountdownStartedAt).toHaveBeenCalledWith(worktree.id, true)
  })

  it('keeps child card markup outside the parent hover trigger when new card style is on', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'comment']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ comment: 'Parent note' })}
        repo={makeRepo()}
        isActive={false}
        lineageChildren={<div data-lineage-child-card="">Child card</div>}
      />
    )
    const surfaceIndex = markup.indexOf('data-worktree-card-surface="true"')
    const triggerIndex = markup.indexOf('data-worktree-card-hover-trigger=""')
    const hoverContentIndex = markup.indexOf('data-hover-card-content=""')
    const childIndex = markup.indexOf('data-lineage-child-card=""')

    expectIdentityBodyIsHoverTrigger(markup)
    expect(markup).toContain('data-worktree-lineage-children=""')
    expect(markup).toContain('group/worktree-card')
    expect(markup).not.toContain('group relative flex cursor-pointer')
    expect(markup).not.toContain('group/worktree-card relative flex cursor-pointer')
    expect(surfaceIndex).toBeGreaterThanOrEqual(0)
    expect(triggerIndex).toBeGreaterThan(surfaceIndex)
    expect(hoverContentIndex).toBeGreaterThanOrEqual(0)
    expect(childIndex).toBeGreaterThan(hoverContentIndex)
  })

  it('uses a centered parent row and raised title size when no meta row is visible', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard worktree={makeWorktree()} repo={makeRepo()} isActive={false} hideRepoBadge />
    )

    expect(markup).not.toContain('data-worktree-card-meta-row=""')
    expect(markup).toContain('data-worktree-card-parent-content=""')
    expect(markup).toContain('items-center')
    expect(markup).toContain('w-5 items-center')
    expect(markup).toContain('text-[13px] leading-5')
  })

  it('does not show a folder path row in new-card mode when no project groups exist', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'branch']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({
          displayName: 'Docs folder',
          branch: '',
          path: ' /Users/x/projects/my-app '
        })}
        repo={{ ...makeRepo(), kind: 'folder' }}
        isActive={false}
        hideRepoBadge
      />
    )

    expect(markup).not.toContain('data-worktree-card-meta-row=""')
    expect(markup).toContain('Docs folder')
    expect(markup).not.toContain('/Users/x/projects/my-app')
  })

  it('shows a folder path row in new-card mode through the branch setting when project groups exist', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'branch']
    projectGroups = [{ id: 'group-1' }]
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({
          displayName: 'Docs folder',
          branch: '',
          path: ' /Users/x/projects/my-app '
        })}
        repo={{ ...makeRepo(), kind: 'folder' }}
        isActive={false}
        hideRepoBadge
      />
    )

    expect(markup).toContain('data-worktree-card-meta-row=""')
    expect(markup).toContain(' /Users/x/projects/my-app ')
    expect(markup).not.toContain('>Folder</span>')
  })

  it('keeps hidden folder path identity available from the new-card hover', async () => {
    settings = { compactWorktreeCards: false, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status']
    projectGroups = [{ id: 'group-1' }]
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({
          displayName: 'Docs folder',
          branch: '',
          path: '/Users/x/projects/my-app'
        })}
        repo={{ ...makeRepo(), kind: 'folder' }}
        isActive={false}
        hideRepoBadge
      />
    )

    expect(markup).not.toContain('data-worktree-card-meta-row=""')
    expect(markup).toContain('data-hover-open-delay="100"')
    expect(markup).toContain('/Users/x/projects/my-app')
    expect(markup).toContain('Docs folder')
  })

  it('shows the branch row for migrated Default cards with branch enabled', async () => {
    settings = { compactWorktreeCards: false }
    worktreeCardProperties = ['status', 'branch']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ displayName: 'Human title' })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expect(markup).toContain('data-worktree-card-meta-row=""')
    expect(markup).toContain('feature/local-branch')
  })

  it('keeps compact card branch hidden in the row but available from title hover by default', async () => {
    settings = { compactWorktreeCards: true }
    worktreeCardProperties = ['status', 'branch']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ displayName: 'Human title' })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expect(markup).not.toContain('data-worktree-card-meta-row=""')
    expect(markup).toContain('data-hover-open-delay="100"')
    expect(markup).toContain('feature/local-branch')
  })

  it('shows the branch row for compact cards when branch is enabled and new card style is on', async () => {
    settings = { compactWorktreeCards: true, experimentalNewWorktreeCardStyle: true }
    worktreeCardProperties = ['status', 'branch']
    const { default: WorktreeCard } = await import('./WorktreeCard')

    const markup = renderToStaticMarkup(
      <WorktreeCard
        worktree={makeWorktree({ displayName: 'Human title' })}
        repo={makeRepo()}
        isActive={false}
      />
    )

    expect(markup).toContain('data-worktree-card-meta-row=""')
    expect(markup).toContain('feature/local-branch')
  })
})
