import React, { useCallback, useEffect, useRef } from 'react'
import { ENTRY_REFRESH_GRACE_MS, shouldEntryRefresh } from '../checks-entry-refresh'
import { refreshHostedReviewCard } from '@/store/slices/hosted-review-card-refresh'

import type { ChecksPanelContextState } from './use-checks-panel-context-state'
import type { ChecksPanelControllerState } from './use-checks-panel-controller-state'
import type { ChecksPanelReviewState } from './use-checks-panel-review-state'
import type { ChecksPanelPollingState } from './use-checks-panel-polling'
import type { ChecksPanelReviewDataState } from './use-checks-panel-review-data'

type ChecksPanelEntryRefreshAndTitleActionsInput = Pick<
  ChecksPanelContextState,
  | 'activeReview'
  | 'clearTitleInputFocusTimer'
  | 'fallbackGitHubPRNumber'
  | 'isFolder'
  | 'linkedAzureDevOpsPR'
  | 'linkedBitbucketPR'
  | 'linkedGiteaPR'
  | 'linkedPR'
  | 'pr'
  | 'prCacheKey'
  | 'prNumber'
> &
  Pick<
    ChecksPanelControllerState,
    | 'activeWorktreeId'
    | 'branch'
    | 'enqueueGitHubPRRefresh'
    | 'fetchHostedReviewForBranch'
    | 'fetchPRForBranch'
    | 'isPanelVisible'
    | 'mountedRef'
    | 'pollIntervalRef'
    | 'prevChecksRef'
    | 'repo'
    | 'setEditingTitle'
    | 'setTitleDraft'
    | 'setTitleSaving'
    | 'titleDraft'
    | 'titleInputFocusTimerRef'
    | 'titleInputRef'
  > &
  Pick<ChecksPanelReviewState, 'checksFetchedAt' | 'commentsFetchedAt' | 'prFetchedAt'> &
  Pick<ChecksPanelPollingState, 'fetchChecks'> &
  Pick<ChecksPanelReviewDataState, 'fetchComments'>

export function useChecksPanelEntryRefreshAndTitleActions(
  model: ChecksPanelEntryRefreshAndTitleActionsInput
) {
  const {
    activeReview,
    activeWorktreeId,
    branch,
    checksFetchedAt,
    clearTitleInputFocusTimer,
    commentsFetchedAt,
    enqueueGitHubPRRefresh,
    fallbackGitHubPRNumber,
    fetchChecks,
    fetchComments,
    fetchHostedReviewForBranch,
    fetchPRForBranch,
    isFolder,
    isPanelVisible,
    linkedAzureDevOpsPR,
    linkedBitbucketPR,
    linkedGiteaPR,
    linkedPR,
    mountedRef,
    pollIntervalRef,
    pr,
    prCacheKey,
    prFetchedAt,
    prNumber,
    prevChecksRef,
    repo,
    setEditingTitle,
    setTitleDraft,
    setTitleSaving,
    titleDraft,
    titleInputFocusTimerRef,
    titleInputRef
  } = model
  const handleEntryRefresh = useCallback(
    (options: { refreshChecks: boolean; refreshComments: boolean }) => {
      if (!repo || !branch || !activeWorktreeId) {
        return
      }
      // Why: tab entry is automatic UI, not a user refresh; keep coordinator rate-limit guards and only force panes already proven stale.
      enqueueGitHubPRRefresh(activeWorktreeId, 'active', 80)
      if (options.refreshChecks) {
        void fetchChecks({ force: true })
      }
      if (options.refreshComments) {
        void fetchComments({ force: true })
      }
    },
    [
      activeWorktreeId,
      branch,
      enqueueGitHubPRRefresh,
      fetchChecks,
      fetchComments,
      repo
    ]
  )

  // Why: force a freshness check on each Checks-tab entry so externally-changed PRs appear without waiting for the cache TTL. See docs/refresh-on-checks-tab.md.
  const entryKey =
    isPanelVisible && repo && !isFolder && branch
      ? `${activeWorktreeId ?? ''}::${prCacheKey}`
      : ''
  const lastEntryKeyRef = useRef<string>('')
  useEffect(() => {
    if (!entryKey) {
      // Reset on hide so reopening the same PR re-evaluates freshness; a prevKey !== currentKey check alone would miss close-and-reopen.
      lastEntryKeyRef.current = ''
      return
    }
    if (lastEntryKeyRef.current === entryKey) {
      return
    }
    lastEntryKeyRef.current = entryKey

    const now = Date.now()
    const stale = shouldEntryRefresh({
      prFetchedAt,
      checksFetchedAt,
      commentsFetchedAt,
      prNumber,
      now,
      graceMs: ENTRY_REFRESH_GRACE_MS
    })
    if (!stale) {
      return
    }
    const cutoff = now - ENTRY_REFRESH_GRACE_MS
    const refreshChecks =
      prNumber !== null && (checksFetchedAt === undefined || checksFetchedAt < cutoff)
    const refreshComments =
      prNumber !== null && (commentsFetchedAt === undefined || commentsFetchedAt < cutoff)

    // Reset polling attention state so the forced fetch establishes a fresh baseline instead of colliding with the previous PR's backoff.
    pollIntervalRef.current = 30_000
    prevChecksRef.current = ''
    handleEntryRefresh({ refreshChecks, refreshComments })
  }, [
    entryKey,
    prFetchedAt,
    checksFetchedAt,
    commentsFetchedAt,
    prNumber,
    handleEntryRefresh,
    pollIntervalRef,
    prevChecksRef
  ])

  const refreshHostedReviewAfterMutation = useCallback(async () => {
    if (!repo || !branch) {
      return
    }
    const refreshedPR = await fetchPRForBranch(repo.path, branch, {
      force: true,
      repoId: repo.id,
      worktreeId: activeWorktreeId ?? undefined,
      linkedPRNumber: linkedPR,
      fallbackPRNumber: fallbackGitHubPRNumber
    })
    await refreshHostedReviewCard(fetchHostedReviewForBranch, {
      repoPath: repo.path,
      repoId: repo.id,
      branch,
      linkedGitHubPR: linkedPR,
      fallbackGitHubPR: refreshedPR?.number ?? fallbackGitHubPRNumber,
      linkedBitbucketPR,
      linkedAzureDevOpsPR,
      linkedGiteaPR
    })
  }, [
    activeWorktreeId,
    branch,
    fallbackGitHubPRNumber,
    fetchHostedReviewForBranch,
    fetchPRForBranch,
    linkedAzureDevOpsPR,
    linkedBitbucketPR,
    linkedGiteaPR,
    linkedPR,
    repo
  ])

  const handleStartEdit = useCallback(() => {
    if (!activeReview) {
      return
    }
    setTitleDraft(activeReview.title)
    setEditingTitle(true)
    clearTitleInputFocusTimer()
    titleInputFocusTimerRef.current = setTimeout(() => {
      titleInputFocusTimerRef.current = null
      titleInputRef.current?.focus()
    }, 0)
  }, [
    activeReview,
    clearTitleInputFocusTimer,
    setEditingTitle,
    titleInputFocusTimerRef,
    setTitleDraft,
    titleInputRef.current
  ])

  const handleCancelEdit = useCallback(() => {
    clearTitleInputFocusTimer()
    setEditingTitle(false)
    setTitleDraft('')
  }, [clearTitleInputFocusTimer, setEditingTitle, setTitleDraft])

  const handleSaveTitle = useCallback(async () => {
    const nextTitle = titleDraft.trim()
    if (!repo || !activeReview || !nextTitle || nextTitle === activeReview.title) {
      clearTitleInputFocusTimer()
      setEditingTitle(false)
      return
    }
    setTitleSaving(true)
    try {
      if (!pr) {
        return
      }
      const ok = await window.api.gh.updatePRTitle({
        repoPath: repo.path,
        repoId: repo.id,
        prNumber: pr.number,
        title: nextTitle,
        prRepo: pr.prRepo ?? null
      })
      if (ok) {
        await refreshHostedReviewAfterMutation()
      }
    } finally {
      clearTitleInputFocusTimer()
      if (mountedRef.current) {
        setTitleSaving(false)
        setEditingTitle(false)
      }
    }
  }, [
    activeReview,
    repo,
    pr,
    titleDraft,
    refreshHostedReviewAfterMutation,
    clearTitleInputFocusTimer,
    mountedRef,
    setEditingTitle,
    setTitleSaving
  ])

  const handleTitleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        e.preventDefault()
        void handleSaveTitle()
      } else if (e.key === 'Escape') {
        handleCancelEdit()
      }
    },
    [handleSaveTitle, handleCancelEdit]
  )
  return {
    refreshHostedReviewAfterMutation,
    handleStartEdit,
    handleCancelEdit,
    handleSaveTitle,
    handleTitleKeyDown
  }
}

export type ChecksPanelEntryRefreshAndTitleActionsState = ReturnType<
  typeof useChecksPanelEntryRefreshAndTitleActions
>
