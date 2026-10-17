import { useEffect } from 'react'
import { useAppStore } from '@/store'
import { translate } from '@/i18n/i18n'

const CAPABILITIES = [
  {
    key: 'components.onboarding.integrations.capabilities.startWorkspaceFromIssue',
    fallback:
      'Start a workspace from a GitLab, Bitbucket, Azure DevOps, or Gitea issue or pull request, prefilled with its title and context'
  },
  {
    key: 'components.onboarding.integrations.capabilities.browseIssues',
    fallback: 'Browse issues and pull requests from those hosts without leaving Orca'
  },
  {
    key: 'components.onboarding.integrations.capabilities.reviewStatus',
    fallback: 'See review status and merge readiness on every worktree'
  },
  {
    key: 'components.onboarding.integrations.capabilities.managePullRequests',
    fallback: 'Read, comment on, and merge merge requests without leaving Orca'
  }
] as const

export function IntegrationsStep(): React.JSX.Element {
  const refreshPreflightStatus = useAppStore((s) => s.refreshPreflightStatus)

  useEffect(() => {
    void refreshPreflightStatus()
  }, [refreshPreflightStatus])

  return (
    <div className="space-y-6">
      <ul className="-mt-6 space-y-1.5 text-[14px] leading-relaxed text-muted-foreground">
        {CAPABILITIES.map(({ key, fallback }) => (
          <li key={key} className="flex gap-2.5">
            <span className="mt-2 size-1 shrink-0 rounded-full bg-muted-foreground" aria-hidden />
            <span>{translate(key, fallback)}</span>
          </li>
        ))}
      </ul>

      <div className="space-y-3">
        <div className="rounded-xl border border-border bg-muted/10 px-5 py-4">
          <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
            <span className="text-[14px] font-medium text-foreground/70">
              {translate(
                'auto.components.onboarding.IntegrationsStep.3a3e360289',
                'More task sources'
              )}
            </span>
            <span className="text-[13px] leading-relaxed text-muted-foreground">
              {translate(
                'auto.components.onboarding.IntegrationsStep.277f30eb34',
                'GitLab, Bitbucket, Azure DevOps, and Gitea live in Settings > Integrations.'
              )}
            </span>
          </div>
        </div>
      </div>
    </div>
  )
}
