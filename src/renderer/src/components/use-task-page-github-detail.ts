import type { TaskPageGitHubListStateModel } from './use-task-page-github-list-state'
import { useAppStore } from '@/store'
import { useState, useMemo, useLayoutEffect, useCallback, useEffect } from 'react'
import type { ItemDialogTab } from '@/components/GitHubItemDialog'
import { stripRepoQualifiers } from '../../../shared/task-query'
import { useShallow } from 'zustand/react/shallow'
import {
  selectTaskPageWorkItemsCacheEntries,
  findTaskPageDialogWorkItem
} from '@/components/task-page-cache-selectors'
import type { GitHubWorkItem } from '../../../shared/github/work-item-types'
import { getTaskPageRepoCacheInput, getTaskPageRepoSourceContext } from './task-page-source-context'
import { startGitHubListScrollRestore } from './task-page-github-list-scroll-restore'

function getTaskPageScrollTop(
  scrollRef: React.RefObject<HTMLElement | null>,
  fallback: number
): number {
  return scrollRef.current?.scrollTop ?? fallback
}

export function useTaskPageGitHubDetail(model: TaskPageGitHubListStateModel) {
  const {
    pageData,
    openTaskPage,
    repoMap,
    selectedRepos,
    setGithubMode,
    appliedTaskSearch,
    githubPerRepoPageLimit,
    githubResumeContextKey,
    pages,
    currentPage,
    currentPageRef,
    githubListScrollRef,
    githubListScrollTopRef,
    pendingGithubScrollRestoreRef,
    githubListRestoreWriteRef,
    taskListPositionRef
  } = model
  // Why: the dialog's "Use" button routes through the same direct-launch flow as the row-level "Use" CTA so behavior is consistent regardless of entry point.
  const githubTaskDrawerWorkItem = useAppStore((s) => s.githubTaskDrawerWorkItem)
  const setGithubTaskDrawerWorkItem = useAppStore((s) => s.setGithubTaskDrawerWorkItem)
  const [dialogInitialTab, setDialogInitialTab] = useState<ItemDialogTab>('conversation')
  const dialogWorkItemKey = githubTaskDrawerWorkItem
    ? {
        id: githubTaskDrawerWorkItem.id,
        repoId: githubTaskDrawerWorkItem.repoId
      }
    : null
  const appliedWorkItemsCacheQuery = useMemo(
    () => stripRepoQualifiers(appliedTaskSearch.trim()),
    [appliedTaskSearch]
  )
  const selectedWorkItemsCacheEntries = useAppStore(
    useShallow((s) =>
      selectTaskPageWorkItemsCacheEntries(
        s.workItemsCache,
        selectedRepos.map(getTaskPageRepoCacheInput),
        githubPerRepoPageLimit,
        appliedWorkItemsCacheQuery
      )
    )
  )

  // Why: derive the dialog item from the cache for optimistic patches, falling back to the click-time snapshot for new stubs; key by repoId so same-number issues across repos resolve to the clicked row.
  const cachedDialogWorkItem = useAppStore((s) =>
    findTaskPageDialogWorkItem(s.workItemsCache, dialogWorkItemKey)
  )
  const dialogWorkItem = dialogWorkItemKey
    ? (cachedDialogWorkItem ?? githubTaskDrawerWorkItem)
    : null
  useLayoutEffect(() => {
    const target = pendingGithubScrollRestoreRef.current
    if (target === null || !githubListScrollRef.current || !pages[currentPage]) {
      return
    }
    return startGitHubListScrollRestore({
      target,
      scrollElementRef: githubListScrollRef,
      pendingRestoreRef: pendingGithubScrollRestoreRef,
      restoreWriteRef: githubListRestoreWriteRef,
      onScrollTopApplied: (scrollTop) => {
        githubListScrollTopRef.current = scrollTop
        taskListPositionRef.current = {
          contextKey: githubResumeContextKey,
          page: currentPage,
          scrollTop
        }
      }
    })
  }, [
    currentPage,
    dialogWorkItem,
    githubListRestoreWriteRef,
    githubListScrollRef,
    githubListScrollTopRef,
    githubResumeContextKey,
    pages,
    pendingGithubScrollRestoreRef,
    taskListPositionRef
  ])
  const dialogRepoPath = dialogWorkItem ? (repoMap.get(dialogWorkItem.repoId)?.path ?? null) : null
  const dialogSourceContext = useMemo(() => {
    if (!dialogWorkItem) {
      return null
    }
    if (
      pageData.openGitHubSourceContext?.provider === 'github' &&
      pageData.openGitHubWorkItem?.id === dialogWorkItem.id &&
      pageData.openGitHubWorkItem.repoId === dialogWorkItem.repoId
    ) {
      return pageData.openGitHubSourceContext
    }
    return getTaskPageRepoSourceContext(repoMap.get(dialogWorkItem.repoId), 'github')
  }, [dialogWorkItem, pageData.openGitHubSourceContext, pageData.openGitHubWorkItem, repoMap])
  const setDialogWorkItem = useCallback(
    (item: GitHubWorkItem | null, initialTab: ItemDialogTab = 'conversation') => {
      setDialogInitialTab(item ? initialTab : 'conversation')
      setGithubTaskDrawerWorkItem(item)
    },
    [setGithubTaskDrawerWorkItem]
  )
  useEffect(() => {
    if (!pageData.openGitHubWorkItem) {
      setDialogWorkItem(null)
      return
    }
    setGithubMode('items')
    setDialogWorkItem(pageData.openGitHubWorkItem, pageData.openGitHubInitialTab)
  }, [pageData.openGitHubInitialTab, pageData.openGitHubWorkItem, setDialogWorkItem, setGithubMode])
  const openGitHubDetailPage = useCallback(
    (item: GitHubWorkItem, initialTab: ItemDialogTab = 'conversation') => {
      const scrollTop = getTaskPageScrollTop(githubListScrollRef, githubListScrollTopRef.current)
      githubListScrollTopRef.current = scrollTop
      pendingGithubScrollRestoreRef.current = scrollTop
      taskListPositionRef.current = {
        contextKey: githubResumeContextKey,
        page: currentPageRef.current,
        scrollTop
      }
      useAppStore.getState().setTaskListPosition(taskListPositionRef.current)
      openTaskPage(
        {
          taskSource: 'github',
          preselectedRepoId: item.repoId,
          openGitHubWorkItem: item,
          openGitHubSourceContext: getTaskPageRepoSourceContext(repoMap.get(item.repoId), 'github'),
          openGitHubInitialTab: initialTab
        },
        {
          recordTasksInteraction: false
        }
      )
    },
    [
      currentPageRef,
      githubListScrollRef,
      githubListScrollTopRef,
      githubResumeContextKey,
      openTaskPage,
      pendingGithubScrollRestoreRef,
      repoMap,
      taskListPositionRef
    ]
  )
  const nextModel = model as typeof model & {
    githubTaskDrawerWorkItem: typeof githubTaskDrawerWorkItem
    setGithubTaskDrawerWorkItem: typeof setGithubTaskDrawerWorkItem
    dialogInitialTab: typeof dialogInitialTab
    setDialogInitialTab: typeof setDialogInitialTab
    dialogWorkItemKey: typeof dialogWorkItemKey
    appliedWorkItemsCacheQuery: typeof appliedWorkItemsCacheQuery
    selectedWorkItemsCacheEntries: typeof selectedWorkItemsCacheEntries
    cachedDialogWorkItem: typeof cachedDialogWorkItem
    dialogWorkItem: typeof dialogWorkItem
    dialogRepoPath: typeof dialogRepoPath
    dialogSourceContext: typeof dialogSourceContext
    setDialogWorkItem: typeof setDialogWorkItem
    openGitHubDetailPage: typeof openGitHubDetailPage
  }
  nextModel.githubTaskDrawerWorkItem = githubTaskDrawerWorkItem
  nextModel.setGithubTaskDrawerWorkItem = setGithubTaskDrawerWorkItem
  nextModel.dialogInitialTab = dialogInitialTab
  nextModel.setDialogInitialTab = setDialogInitialTab
  nextModel.dialogWorkItemKey = dialogWorkItemKey
  nextModel.appliedWorkItemsCacheQuery = appliedWorkItemsCacheQuery
  nextModel.selectedWorkItemsCacheEntries = selectedWorkItemsCacheEntries
  nextModel.cachedDialogWorkItem = cachedDialogWorkItem
  nextModel.dialogWorkItem = dialogWorkItem
  nextModel.dialogRepoPath = dialogRepoPath
  nextModel.dialogSourceContext = dialogSourceContext
  nextModel.setDialogWorkItem = setDialogWorkItem
  nextModel.openGitHubDetailPage = openGitHubDetailPage
  return nextModel
}
export type TaskPageGitHubDetailModel = ReturnType<typeof useTaskPageGitHubDetail>
