import React from 'react'
import { Github } from 'lucide-react'

import { createLocalizedCatalog } from '@/i18n/localized-catalog'
import { translate } from '@/i18n/i18n'
import { getTaskPresetQuery } from '../../../shared/task-preset-query'
import type { TaskProvider } from '../../../shared/task-providers'
import type { TaskViewPresetId } from '../../../shared/ui-chrome-types'

export type TaskQueryPreset = {
  id: TaskViewPresetId
  label: string
  query: string
}

export type GitHubTaskKind = 'issues' | 'prs'

export type SourceOption = {
  id: TaskProvider
  label: string
  Icon: (props: { className?: string }) => React.JSX.Element
  disabled?: boolean
}

export type GitHubModeButton = { id: GitHubTaskKind | 'project'; label: string }

const getIssueTaskQueryPresets = createLocalizedCatalog((): TaskQueryPreset[] => [
  {
    id: 'issues',
    label: translate('auto.components.TaskPage.606a85c774', 'Open'),
    query: getTaskPresetQuery('issues')
  },
  {
    id: 'my-issues',
    label: translate('auto.components.TaskPage.94f0339621', 'Assigned to me'),
    query: getTaskPresetQuery('my-issues')
  }
])

const getPRTaskQueryPresets = createLocalizedCatalog((): TaskQueryPreset[] => [
  {
    id: 'prs',
    label: translate('auto.components.TaskPage.606a85c774', 'Open'),
    query: getTaskPresetQuery('prs')
  },
  {
    id: 'my-prs',
    label: translate('auto.components.TaskPage.7698af5263', 'Mine'),
    query: getTaskPresetQuery('my-prs')
  },
  {
    id: 'review',
    label: translate('auto.components.TaskPage.524f095d55', 'Needs review'),
    query: getTaskPresetQuery('review')
  }
])

export function getGitHubTaskKindPresets(kind: GitHubTaskKind): TaskQueryPreset[] {
  return kind === 'prs' ? getPRTaskQueryPresets() : getIssueTaskQueryPresets()
}

export const getSourceOptions = createLocalizedCatalog((): SourceOption[] => [
  {
    id: 'github',
    label: translate('auto.components.TaskPage.acef77f7ca', 'GitHub'),
    Icon: ({ className }) => <Github className={className} />
  }
])

export const getGitHubModeButtons = createLocalizedCatalog((): GitHubModeButton[] => [
  { id: 'issues', label: translate('auto.components.TaskPage.dfc0c79bd8', 'Issues') },
  { id: 'prs', label: translate('auto.components.TaskPage.137e2a8a01', 'PRs') },
  { id: 'project', label: translate('auto.components.TaskPage.727069bee5', 'Projects') }
])
