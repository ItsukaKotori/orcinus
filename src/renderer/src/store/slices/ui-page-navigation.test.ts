import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getDefaultSettings } from '../../../../shared/constants'
import type { GitHubWorkItem } from '../../../../shared/github/work-item-types'
import type { Worktree } from '../../../../shared/worktree/types'
import type { FeatureInteractionState } from '../../../../shared/feature-interactions'
import type { TaskSourceContext } from '../../../../shared/task-source-context'
import type { AppState } from '../types'
import { createUIStore, makePersistedUI } from './ui-slice-test-harness'

const mocks = vi.hoisted(() => ({
  sendNotesToActiveAgentSession: vi.fn(),
  track: vi.fn(),
  toastMessage: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn()
}))

vi.mock('@/lib/active-agent-note-send', () => ({
  activeAgentNotesSendFailureMessage: (
    status: string,
    options: { explicitTarget?: boolean } = {}
  ) => (options.explicitTarget ? `selected:${status}` : status),
  sendNotesToActiveAgentSession: mocks.sendNotesToActiveAgentSession
}))

vi.mock('@/lib/telemetry', () => ({
  track: mocks.track
}))

vi.mock('sonner', () => ({
  toast: {
    message: mocks.toastMessage,
    success: mocks.toastSuccess,
    error: mocks.toastError
  }
}))

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

beforeEach(() => {
  mocks.sendNotesToActiveAgentSession.mockReset()
  mocks.sendNotesToActiveAgentSession.mockResolvedValue({ status: 'sent' })
  mocks.track.mockReset()
  mocks.toastMessage.mockReset()
  mocks.toastSuccess.mockReset()
  mocks.toastError.mockReset()
})

function makeWorktree(id: string): Worktree {
  return { id } as unknown as Worktree
}

function makeGitHubWorkItem(overrides: Partial<GitHubWorkItem> = {}): GitHubWorkItem {
  return {
    id: 'pr-95',
    type: 'pr',
    number: 95,
    title: 'feat: add file upload command',
    state: 'open',
    url: 'https://github.com/acme/repo/pull/95',
    labels: [],
    updatedAt: '2026-05-20T00:00:00.000Z',
    author: 'octocat',
    repoId: 'repo-1',
    ...overrides
  }
}

describe('createUISlice settings navigation', () => {
  it('accepts a host-qualified settings target', () => {
    const store = createUIStore()
    store.getState().openSettingsTarget({ pane: 'general', repoId: null, hostId: 'ssh:host-1' })
    expect(store.getState().settingsNavigationTarget).toEqual({
      pane: 'general',
      repoId: null,
      hostId: 'ssh:host-1'
    })
  })

  it('rejects malformed settings targets before storing them', () => {
    const store = createUIStore()
    const openSettingsTarget = store.getState().openSettingsTarget as unknown as (
      target: unknown
    ) => void

    expect(() =>
      openSettingsTarget({ pane: 'repo', repoId: 'repo-1', hostId: 'invalid' })
    ).toThrowError('openSettingsTarget received an invalid navigation target')
    expect(store.getState().settingsNavigationTarget).toBeNull()
  })

  it('prefetches direct GitHub task opens with their source context', () => {
    const store = createUIStore()
    const prefetchWorkItems = vi.fn()
    const workItem = makeGitHubWorkItem()
    const sourceContext: TaskSourceContext = {
      kind: 'task-source',
      provider: 'github',
      projectId: 'project-1',
      hostId: 'ssh:devbox',
      projectHostSetupId: 'setup-1',
      repoId: 'repo-1',
      providerIdentity: { provider: 'github', owner: 'acme', repo: 'repo' }
    }

    store.setState({
      repos: [
        {
          id: 'repo-1',
          path: '/repo',
          displayName: 'Repo',
          badgeColor: 'blue',
          addedAt: 1,
          kind: 'git'
        }
      ],
      settings: {
        visibleTaskProviders: ['github'],
        defaultTaskSource: 'github',
        defaultTaskViewPreset: 'all'
      } as unknown as AppState['settings'],
      prefetchWorkItems
    } as unknown as Partial<AppState>)

    store.getState().openTaskPage({
      taskSource: 'github',
      preselectedRepoId: 'repo-1',
      openGitHubWorkItem: workItem,
      openGitHubSourceContext: sourceContext
    })

    expect(prefetchWorkItems).toHaveBeenCalledWith(
      'repo-1',
      '/repo',
      expect.any(Number),
      'is:issue is:open',
      { sourceContext }
    )
  })

  it('returns to the tasks page after visiting settings from an in-progress draft', () => {
    const store = createUIStore()

    store.getState().openTaskPage({ preselectedRepoId: 'repo-1' })
    store.getState().openSettingsPage()

    expect(store.getState().activeView).toBe('settings')
    expect(store.getState().previousViewBeforeSettings).toBe('tasks')

    store.getState().closeSettingsPage()

    expect(store.getState().activeView).toBe('tasks')
  })

  it('keeps the original return target when settings is reopened while already visible', () => {
    const store = createUIStore()

    store.getState().openTaskPage()
    store.getState().openSettingsPage()
    store.getState().openSettingsPage()

    expect(store.getState().previousViewBeforeSettings).toBe('tasks')

    store.getState().closeSettingsPage()

    expect(store.getState().activeView).toBe('tasks')
  })

  it('clears transient settings search when opening settings', () => {
    const store = createUIStore()

    store.setState({ settingsSearchInputQuery: 'terminal', settingsSearchQuery: 'terminal' })
    store.getState().openSettingsPage()

    expect(store.getState().activeView).toBe('settings')
    expect(store.getState().settingsSearchInputQuery).toBe('')
    expect(store.getState().settingsSearchQuery).toBe('')
  })
})

describe('createUISlice page navigation history', () => {
  it('records and rewinds Tasks visits on close', () => {
    const store = createUIStore()
    store.setState({ worktreesByRepo: { 'repo-1': [makeWorktree('a')] } })

    store.getState().recordWorktreeVisit('a')
    store.getState().openTaskPage()
    expect(store.getState().worktreeNavHistory).toEqual(['a', 'tasks'])
    expect(store.getState().worktreeNavHistoryIndex).toBe(1)

    store.getState().closeTaskPage()
    expect(store.getState().activeView).toBe('terminal')
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)
  })

  it('rewinds Tasks detail visits on close', () => {
    const store = createUIStore()
    const workItem = makeGitHubWorkItem()
    store.setState({ worktreesByRepo: { 'repo-1': [makeWorktree('a')] } })

    store.getState().recordWorktreeVisit('a')
    store.getState().openTaskPage({ taskSource: 'github', openGitHubWorkItem: workItem })
    expect(store.getState().worktreeNavHistory).toEqual([
      'a',
      'tasks',
      {
        kind: 'task-detail',
        source: 'github',
        workItem,
        sourceContext: undefined,
        initialTab: undefined
      }
    ])
    expect(store.getState().worktreeNavHistoryIndex).toBe(2)

    store.getState().closeTaskPage()
    expect(store.getState().activeView).toBe('terminal')
    expect(store.getState().taskPageData).toEqual({})
    expect(store.getState().githubTaskDrawerWorkItem).toBeNull()
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)
  })

  it('records provider-depth interactions for direct Tasks detail opens', () => {
    const store = createUIStore()
    const recordFeatureInteraction = vi.fn()
    store.setState({ recordFeatureInteraction } as Partial<AppState>)
    const workItem = makeGitHubWorkItem()

    store.getState().openTaskPage({ taskSource: 'github', openGitHubWorkItem: workItem })

    expect(recordFeatureInteraction).toHaveBeenCalledWith('tasks')
    expect(recordFeatureInteraction).toHaveBeenCalledWith('github-tasks')
  })

  it('preserves GitHub task detail source context in navigation history', () => {
    const store = createUIStore()
    const workItem = makeGitHubWorkItem({ repoId: 'repo-remote' })
    const sourceContext: TaskSourceContext = {
      kind: 'task-source',
      provider: 'github',
      projectId: 'project-1',
      hostId: 'ssh:devbox',
      projectHostSetupId: 'setup-1',
      repoId: 'repo-remote',
      providerIdentity: { provider: 'github', owner: 'stablyai', repo: 'orca' }
    }

    store.getState().openTaskPage({
      taskSource: 'github',
      openGitHubWorkItem: workItem,
      openGitHubSourceContext: sourceContext
    })

    expect(store.getState().worktreeNavHistory.at(-1)).toEqual({
      kind: 'task-detail',
      source: 'github',
      workItem,
      sourceContext,
      initialTab: undefined
    })
  })

  it('can suppress the Tasks surface interaction for in-page provider navigation', () => {
    const store = createUIStore()
    const recordFeatureInteraction = vi.fn()
    store.setState({ recordFeatureInteraction } as Partial<AppState>)
    const workItem = makeGitHubWorkItem()

    store
      .getState()
      .openTaskPage(
        { taskSource: 'github', openGitHubWorkItem: workItem },
        { recordTasksInteraction: false }
      )

    expect(recordFeatureInteraction).not.toHaveBeenCalledWith('tasks')
    expect(recordFeatureInteraction).toHaveBeenCalledWith('github-tasks')
  })

  it('skips the whole Tasks detail stack on close', () => {
    const store = createUIStore()
    const workItem = makeGitHubWorkItem()
    store.setState({ worktreesByRepo: { 'repo-1': [makeWorktree('a')] } })

    store.getState().recordWorktreeVisit('a')
    store.getState().openTaskPage({ taskSource: 'github', openGitHubWorkItem: workItem })
    store.getState().openTaskPage({ taskSource: 'github' })
    expect(store.getState().worktreeNavHistory).toEqual([
      'a',
      'tasks',
      {
        kind: 'task-detail',
        source: 'github',
        workItem,
        sourceContext: undefined,
        initialTab: undefined
      },
      'tasks'
    ])

    store.getState().closeTaskPage()
    expect(store.getState().activeView).toBe('terminal')
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)
  })

  it('records and rewinds Automations visits on close', () => {
    const store = createUIStore()
    store.setState({ worktreesByRepo: { 'repo-1': [makeWorktree('a')] } })

    store.getState().recordWorktreeVisit('a')
    store.getState().openAutomationsPage()
    expect(store.getState().worktreeNavHistory).toEqual(['a', 'automations'])
    expect(store.getState().worktreeNavHistoryIndex).toBe(1)

    store.getState().closeAutomationsPage()
    expect(store.getState().activeView).toBe('terminal')
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)
  })

  it('dedupes repeated Automations opens against the current history entry', () => {
    const store = createUIStore()
    store.setState({ worktreesByRepo: { 'repo-1': [makeWorktree('a')] } })

    store.getState().recordWorktreeVisit('a')
    store.getState().openAutomationsPage()
    store.getState().openAutomationsPage()

    expect(store.getState().activeView).toBe('automations')
    expect(store.getState().worktreeNavHistory).toEqual(['a', 'automations'])
    expect(store.getState().worktreeNavHistoryIndex).toBe(1)
  })

  it('keeps the Automations history index when Automations is the only entry', () => {
    const store = createUIStore()

    store.getState().openAutomationsPage()
    expect(store.getState().worktreeNavHistory).toEqual(['automations'])
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)

    store.getState().closeAutomationsPage()
    expect(store.getState().activeView).toBe('terminal')
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)
  })

  it('skips deleted prior worktrees when closing Automations', () => {
    const store = createUIStore()
    store.setState({
      activeView: 'automations',
      previousViewBeforeAutomations: 'terminal',
      worktreesByRepo: { 'repo-1': [makeWorktree('c')] },
      worktreeNavHistory: ['c', 'a', 'automations'],
      worktreeNavHistoryIndex: 2
    })

    store.getState().closeAutomationsPage()
    expect(store.getState().activeView).toBe('terminal')
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)
  })
})

describe('createUISlice space navigation', () => {
  it('records Space page opens as workspace cleanup interactions', () => {
    const setMock = vi.fn(() => Promise.resolve())
    vi.stubGlobal('window', {
      api: {
        ui: {
          set: setMock
        }
      }
    })
    const now = 1_700_000_000_000
    vi.useFakeTimers()
    vi.setSystemTime(now)

    try {
      const store = createUIStore()
      store.getState().hydratePersistedUI(makePersistedUI())
      setMock.mockClear()

      store.getState().openSpacePage()

      const expected: FeatureInteractionState = {
        'workspace-cleanup': { firstInteractedAt: now, interactionCount: 1 }
      }
      expect(store.getState().featureInteractions).toEqual(expected)
      expect(setMock).toHaveBeenCalledWith({ featureInteractions: expected })
    } finally {
      vi.useRealTimers()
    }
  })

  it('returns to the tasks page after opening Space from an in-progress draft', () => {
    const store = createUIStore()

    store.getState().openTaskPage({ preselectedRepoId: 'repo-1' })
    store.getState().openSpacePage()

    expect(store.getState().activeView).toBe('space')
    expect(store.getState().previousViewBeforeSpace).toBe('tasks')

    store.getState().closeSpacePage()

    expect(store.getState().activeView).toBe('tasks')
  })

  it('keeps the original return target when Space is reopened while already visible', () => {
    const store = createUIStore()

    store.getState().openTaskPage()
    store.getState().openSpacePage()
    store.getState().openSpacePage()

    expect(store.getState().previousViewBeforeSpace).toBe('tasks')

    store.getState().closeSpacePage()

    expect(store.getState().activeView).toBe('tasks')
  })

  it('returns to the originating view after closing Artifacts', () => {
    const store = createUIStore()

    store.getState().openTaskPage()
    store.getState().openArtifactsPage()

    expect(store.getState().activeView).toBe('artifacts')
    expect(store.getState().previousViewBeforeArtifacts).toBe('tasks')

    store.getState().closeArtifactsPage()

    expect(store.getState().activeView).toBe('tasks')
  })

  it('records and rewinds Artifacts visits on close', () => {
    const store = createUIStore()
    store.setState({ worktreesByRepo: { 'repo-1': [makeWorktree('a')] } })

    store.getState().recordWorktreeVisit('a')
    store.getState().openArtifactsPage()
    expect(store.getState().worktreeNavHistory).toEqual(['a', 'artifacts'])
    expect(store.getState().worktreeNavHistoryIndex).toBe(1)

    store.getState().closeArtifactsPage()
    expect(store.getState().activeView).toBe('terminal')
    expect(store.getState().worktreeNavHistoryIndex).toBe(0)
  })

  it('opens and restores Artifacts when its sidebar shortcut is hidden', () => {
    const store = createUIStore()
    store.setState({ settings: { ...getDefaultSettings('/tmp'), showArtifactsButton: false } })

    store.getState().openArtifactsPage()
    expect(store.getState().activeView).toBe('artifacts')

    store.getState().hydratePersistedUI(makePersistedUI({ activeView: 'artifacts' }), 'startup')
    expect(store.getState().activeView).toBe('artifacts')
  })
})
