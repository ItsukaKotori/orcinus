import type { SettingsSearchEntry } from './settings-search'
import { createLocalizedCatalog } from '@/i18n/localized-catalog'
import { translate } from '@/i18n/i18n'
import { translateSearchKeyword } from './settings-search-keywords'

export const getLeftSidebarAppearanceEntry = createLocalizedCatalog((): SettingsSearchEntry => ({
  title: translate(
    'auto.components.settings.appearance.search.leftSidebarAppearance.title',
    'Left Sidebar Appearance'
  ),
  description: translate(
    'auto.components.settings.appearance.search.leftSidebarAppearance.description',
    'Make the left sidebar match your terminal, stay default, or use a tint.'
  ),
  keywords: [
    ...translateSearchKeyword('auto.components.settings.appearance.search.5bff6a2ef0', 'sidebar'),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.leftSidebarAppearance.project',
      'project'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.leftSidebarAppearance.terminal',
      'terminal'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.leftSidebarAppearance.background',
      'background'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.leftSidebarAppearance.tint',
      'tint'
    )
  ]
}))

export const getWorkspaceCardLayoutEntry = createLocalizedCatalog((): SettingsSearchEntry => ({
  title: translate(
    'auto.components.settings.appearance.search.workspaceCardLayout.title',
    'Workspace Card Layout'
  ),
  description: translate(
    'auto.components.settings.appearance.search.workspaceCardLayout.description',
    'Workspace cards can use compact or detailed layouts.'
  ),
  keywords: [
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.workspaceCardLayout.compact',
      'compact'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.workspaceCardLayout.compactDisplay',
      'compact display'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.workspaceCardLayout.workspaceCards',
      'workspace cards'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.workspaceCardLayout.worktreeCards',
      'worktree cards'
    ),
    ...translateSearchKeyword('auto.components.settings.appearance.search.5bff6a2ef0', 'sidebar'),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.workspaceCardLayout.cardLayout',
      'card layout'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.workspaceCardLayout.workspaceOptions',
      'workspace options'
    ),
    ...translateSearchKeyword(
      'auto.components.settings.appearance.search.workspaceCardLayout.detailed',
      'detailed'
    )
  ]
}))

export const getShowPinnedWorktreesInGroupsEntry = createLocalizedCatalog(
  (): SettingsSearchEntry => ({
    title: translate(
      'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.title',
      'Also show pinned worktrees in their original lists'
    ),
    description: translate(
      'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.description',
      'Pinned worktrees stay in Pinned and also appear in All, Project, Status, and PR.'
    ),
    keywords: [
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.pinned',
        'pinned'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.worktree',
        'worktree'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.workspace',
        'workspace'
      ),
      ...translateSearchKeyword('auto.components.settings.appearance.search.5bff6a2ef0', 'sidebar'),
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.all',
        'all'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.project',
        'project'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.status',
        'status'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.pr',
        'pr'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.appearance.search.showPinnedWorktreesInGroups.duplicate',
        'duplicate'
      )
    ]
  })
)

export const getSidebarEntries = createLocalizedCatalog((): SettingsSearchEntry[] => [
  getWorkspaceCardLayoutEntry(),
  getLeftSidebarAppearanceEntry(),
  getShowPinnedWorktreesInGroupsEntry()
])
