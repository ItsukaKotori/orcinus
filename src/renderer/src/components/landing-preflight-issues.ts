import { translate } from '@/i18n/i18n'

export type PreflightIssue = {
  id: string
  title: string
  description: string
  fixLabel: string
  fixUrl: string
  /** Git is a hard global dependency and stays pinned; provider-specific CLI
   *  setup is a soft nudge the user can dismiss. */
  dismissible?: boolean
}

export type LandingPreflightStatus = {
  git: { installed: boolean }
}

export function getLandingPreflightIssues(status: LandingPreflightStatus): PreflightIssue[] {
  const issues: PreflightIssue[] = []

  if (!status.git.installed) {
    issues.push({
      id: 'git',
      title: translate('auto.components.Landing.e5b7296d9d', 'Git is not installed'),
      description: translate(
        'auto.components.Landing.b673e7cf1b',
        'Git is required for Git projects, source control, and workspace management.'
      ),
      fixLabel: 'Install Git',
      fixUrl: 'https://git-scm.com/downloads'
    })
  }

  return issues
}
