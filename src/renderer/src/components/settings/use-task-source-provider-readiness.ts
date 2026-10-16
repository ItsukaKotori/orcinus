import { useMemo } from 'react'
import type { TaskProvider } from '../../../../shared/task-providers'
import { getLocalPreflightContext, localPreflightContextKey } from '@/lib/local-preflight-context'
import { useAppStore } from '@/store'
import type { TaskProviderReadiness } from './task-source-setup-state'

export function useTaskSourceProviderReadiness(
  visibleProviders: readonly TaskProvider[]
): Record<TaskProvider, TaskProviderReadiness> {
  const preflightStatus = useAppStore((s) => s.preflightStatus)
  const preflightStatusChecked = useAppStore((s) => s.preflightStatusChecked)
  const preflightStatusContextKey = useAppStore((s) => s.preflightStatusContextKey)
  const preflightStatusError = useAppStore((s) => s.preflightStatusError)
  const preflightStatusLoading = useAppStore((s) => s.preflightStatusLoading)
  const expectedPreflightContextKey = useAppStore((s) =>
    localPreflightContextKey(getLocalPreflightContext(s))
  )
  const preflightCurrent = preflightStatusContextKey === expectedPreflightContextKey
  const reviewChecking = preflightStatusLoading || !preflightStatusChecked || !preflightCurrent
  // A failed preflight leaves the previous status object in place, so mirror
  // Integrations and refuse to read connection facts out of a stale snapshot.
  const reviewReadyForConnection = !reviewChecking && preflightStatusError === null
  const reviewUnavailable = !reviewChecking && preflightStatusError !== null
  const githubConnected =
    reviewReadyForConnection &&
    preflightStatus?.gh?.installed === true &&
    preflightStatus.gh.authenticated === true
  // Normalization returns a new array, so memoize by provider contents.
  const visibleProvidersKey = visibleProviders.join(',')

  return useMemo(() => {
    const visible = new Set(visibleProvidersKey.split(',') as TaskProvider[])
    return {
      github: {
        connected: githubConnected,
        checking: reviewChecking,
        unavailable: reviewUnavailable,
        visible: visible.has('github')
      },
      gitlab: {
        connected: false,
        checking: false,
        visible: false
      },
      linear: {
        connected: false,
        checking: false,
        visible: false
      },
      jira: {
        connected: false,
        checking: false,
        visible: false
      }
    }
  }, [githubConnected, reviewChecking, reviewUnavailable, visibleProvidersKey])
}
