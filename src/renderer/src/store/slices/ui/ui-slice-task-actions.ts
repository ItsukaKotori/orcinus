import type { UISlice, UISliceGet, UISliceSet } from './ui-slice-contract'
import { findPrevLiveNonTaskStackHistoryIndex } from '../worktree-nav-history'
import {
  normalizeVisibleTaskProviders,
  restoreAvailableDefaultTaskProvider,
  resolveVisibleTaskProvider
} from '../../../../../shared/task-providers'
import { PER_REPO_FETCH_LIMIT } from '../../../../../shared/work-items'
import { isGitRepoKind } from '../../../../../shared/repo-kind'
import { presetToQuery } from './ui-slice-hydration-sanitizers'

export function createUiTaskActions(set: UISliceSet, get: UISliceGet): Partial<UISlice> {
  return {
    activeView: 'terminal',
    previousViewBeforeTasks: 'terminal',
    previousViewBeforeSettings: 'terminal',
    previousViewBeforeAutomations: 'terminal',
    previousViewBeforeSpace: 'terminal',
    previousViewBeforeArtifacts: 'terminal',
    setActiveView: (view) => set({ activeView: view }),
    taskPageData: {},
    taskResumeState: undefined,
    taskListPosition: null,
    githubTaskDrawerWorkItem: null,
    newWorkspaceDraft: null,
    openTaskPage: (data = {}, options = {}) => {
      if (options.recordTasksInteraction !== false) {
        get().recordFeatureInteraction?.('tasks')
      }
      if (data.openGitHubWorkItem) {
        get().recordFeatureInteraction?.('github-tasks')
      }
      // Why: record a Tasks visit in shared back/forward history; all task-source variants collapse to one deduped 'tasks' entry.
      const detailEntry = data.openGitHubWorkItem
        ? ({
            kind: 'task-detail',
            source: 'github',
            workItem: data.openGitHubWorkItem,
            sourceContext: data.openGitHubSourceContext,
            initialTab: data.openGitHubInitialTab
          } as const)
        : null
      const currentEntry = get().worktreeNavHistory[get().worktreeNavHistoryIndex]
      const currentIsTaskStack =
        currentEntry === 'tasks' ||
        (typeof currentEntry === 'object' && currentEntry.kind === 'task-detail')
      if (!detailEntry || !currentIsTaskStack) {
        get().recordViewVisit('tasks')
      }
      if (detailEntry) {
        get().recordViewVisit(detailEntry)
      }
      set((state) => ({
        activeView: 'tasks',
        previousViewBeforeTasks:
          state.activeView === 'tasks' ? state.previousViewBeforeTasks : state.activeView,
        taskPageData: data
      }))
      // Why: prefetch the work-item list during first render so the page's effect hits a warm/in-flight SWR cache (~300–800ms win).
      const state = get()
      const preferredVisibleTaskProviders = normalizeVisibleTaskProviders(
        state.settings?.visibleTaskProviders
      )
      const visibleTaskProviders = restoreAvailableDefaultTaskProvider(
        preferredVisibleTaskProviders,
        {
          gitlabInstalled: false,
          linearConnected: false
        },
        state.settings?.defaultTaskSource
      )
      const resolvedSource = resolveVisibleTaskProvider(
        data.taskSource ?? state.settings?.defaultTaskSource,
        visibleTaskProviders
      )
      const resolvedMode = state.taskResumeState?.githubMode ?? 'items'
      if (resolvedSource === 'github' && resolvedMode === 'items') {
        const eligibleRepos = state.repos.filter((repo) => isGitRepoKind(repo) && repo.path)
        const selectedRepos = (() => {
          const preferred = data.preselectedRepoId
          if (preferred) {
            const repo = eligibleRepos.find((r) => r.id === preferred)
            return repo ? [repo] : []
          }
          const persisted = state.settings?.defaultRepoSelection
          if (Array.isArray(persisted)) {
            const selected = eligibleRepos.filter((repo) => persisted.includes(repo.id))
            if (selected.length > 0) {
              return selected
            }
          }
          return eligibleRepos
        })()

        const resume = state.taskResumeState
        const defaultPreset = state.settings?.defaultTaskViewPreset ?? 'all'
        // Why: must match the query TaskPage's resume effect mounts with, else the warm cache key misses and prefetch is wasted.
        const query =
          resume?.githubItemsPreset === null
            ? (resume.githubItemsQuery ?? '').trim()
            : presetToQuery(resume?.githubItemsPreset ?? defaultPreset)
        for (const repo of selectedRepos) {
          state.prefetchWorkItems(repo.id, repo.path, PER_REPO_FETCH_LIMIT, query, {
            sourceContext:
              data.openGitHubSourceContext?.provider === 'github' &&
              data.openGitHubSourceContext.repoId === repo.id
                ? data.openGitHubSourceContext
                : null
          })
        }
      }
    },
    setTaskResumeState: (updates) =>
      set((s) => {
        const next = { ...s.taskResumeState, ...updates }
        window.api.ui.set({ taskResumeState: next }).catch(console.error)
        return { taskResumeState: next }
      }),
    setTaskListPosition: (taskListPosition) => set({ taskListPosition }),
    setGithubTaskDrawerWorkItem: (item) => set({ githubTaskDrawerWorkItem: item }),
    closeTaskPage: () =>
      set((state) => {
        // Why: if parked on a 'tasks' entry, rewind the history index so Back/Forward aren't no-ops; keep 0 if it's the only entry.
        const currentEntry = state.worktreeNavHistory[state.worktreeNavHistoryIndex]
        let nextHistoryIndex = state.worktreeNavHistoryIndex
        if (
          currentEntry === 'tasks' ||
          (typeof currentEntry === 'object' && currentEntry.kind === 'task-detail')
        ) {
          const prev = findPrevLiveNonTaskStackHistoryIndex(state)
          if (prev !== null) {
            nextHistoryIndex = prev
          } else if (typeof currentEntry === 'object' && state.worktreeNavHistory[0] === 'tasks') {
            nextHistoryIndex = 0
          }
        }
        return {
          activeView: state.previousViewBeforeTasks,
          taskPageData: {},
          githubTaskDrawerWorkItem: null,
          worktreeNavHistoryIndex: nextHistoryIndex
        }
      })
  }
}
