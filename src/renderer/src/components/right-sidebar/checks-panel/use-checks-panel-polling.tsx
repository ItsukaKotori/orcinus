import { useCallback, useEffect } from 'react'
import { installWindowVisibilityTimeoutPoller } from '@/lib/window-visibility-timeout-poller'
import { checksPanelAsyncResultKey } from '../checks-panel-async-result-key'
import type { ChecksPanelControllerState } from './use-checks-panel-controller-state'
import type { ChecksPanelComposerState } from './use-checks-panel-composer-state'
import type { ChecksPanelContextState } from './use-checks-panel-context-state'

type ChecksPanelPollingInput = Pick<ChecksPanelContextState, 'pr' | 'prCacheKey' | 'prNumber'> &
  Pick<
    ChecksPanelControllerState,
    | 'branch'
    | 'fetchPRChecks'
    | 'isPanelVisible'
    | 'pollIntervalRef'
    | 'prevChecksRef'
    | 'repo'
    | 'setChecks'
    | 'setChecksLoading'
  > &
  Pick<ChecksPanelComposerState, 'isCurrentAsyncResult'>

export function useChecksPanelPolling(model: ChecksPanelPollingInput) {
  const {
    branch,
    fetchPRChecks,
    isCurrentAsyncResult,
    isPanelVisible,
    pollIntervalRef,
    pr,
    prCacheKey,
    prNumber,
    prevChecksRef,
    repo,
    setChecks,
    setChecksLoading
  } = model
  // Fetch checks via cached store method
  const fetchChecks = useCallback(
    async ({
      force = false,
      prNumberOverride
    }: { force?: boolean; prNumberOverride?: number | null } = {}) => {
      const targetPRNumber = prNumberOverride ?? prNumber
      if (!repo || !targetPRNumber) {
        return
      }
      setChecksLoading(true)
      try {
        const requestKey = checksPanelAsyncResultKey(
          prCacheKey,
          branch,
          targetPRNumber,
          pr?.prRepo,
          pr?.headSha
        )
        const result = await fetchPRChecks(
          repo.path,
          targetPRNumber,
          branch,
          pr?.headSha,
          pr?.prRepo,
          {
            force,
            repoId: repo.id
          }
        )
        if (!isCurrentAsyncResult(requestKey)) {
          return
        }
        setChecks(result)

        // Exponential backoff: unchanged checks double the interval (cap 120s), changes reset to 30s.
        const signature = JSON.stringify(result.map((c) => `${c.name}:${c.status}:${c.conclusion}`))
        pollIntervalRef.current =
          signature === prevChecksRef.current
            ? Math.min(pollIntervalRef.current * 2, 120_000)
            : 30_000
        prevChecksRef.current = signature
      } catch (err) {
        if (
          !isCurrentAsyncResult(
            checksPanelAsyncResultKey(prCacheKey, branch, targetPRNumber, pr?.prRepo, pr?.headSha)
          )
        ) {
          return
        }
        console.warn('Failed to fetch PR checks:', err)
        setChecks([])
      } finally {
        if (
          isCurrentAsyncResult(
            checksPanelAsyncResultKey(prCacheKey, branch, targetPRNumber, pr?.prRepo, pr?.headSha)
          )
        ) {
          setChecksLoading(false)
        }
      }
    },
    [
      repo,
      prNumber,
      branch,
      pr?.headSha,
      pr?.prRepo,
      prCacheKey,
      fetchPRChecks,
      isCurrentAsyncResult,
      prevChecksRef,
      setChecksLoading,
      pollIntervalRef,
      setChecks
    ]
  )

  // Fetch checks on mount + poll with exponential backoff
  useEffect(() => {
    if (!prNumber || !isPanelVisible) {
      setChecks([])
      return
    }

    // Reset backoff state on PR change
    pollIntervalRef.current = 30_000
    prevChecksRef.current = ''
    // Why: check status is user-visible; keep visible unfocused windows fresh but stop timers/API work while hidden.
    return installWindowVisibilityTimeoutPoller({
      run: () => fetchChecks(),
      getDelayMs: () => pollIntervalRef.current
    })
  }, [fetchChecks, isPanelVisible, prNumber, pollIntervalRef, prevChecksRef, setChecks])

  return { fetchChecks }
}

export type ChecksPanelPollingState = ReturnType<typeof useChecksPanelPolling>
