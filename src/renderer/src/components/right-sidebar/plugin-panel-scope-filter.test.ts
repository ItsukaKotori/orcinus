import { describe, expect, it } from 'vitest'
import type { PluginHostListEntry } from '../../../../preload/api-types'
import { filterRightSidebarPluginEntries } from './plugin-panel-activity-items'

function entry(
  pluginKey: string,
  scope: 'global' | 'project',
  repoId?: string
): PluginHostListEntry {
  return { pluginKey, scope, ...(repoId ? { repoId } : {}) } as PluginHostListEntry
}

const entries = [
  entry('global-plugin', 'global'),
  entry('repo-notes', 'project', 'repo-1'),
  entry('other-repo-notes', 'project', 'repo-2'),
  entry('legacy-no-repo', 'project')
]

describe('filterRightSidebarPluginEntries', () => {
  it('keeps only project entries installed into the active repo', () => {
    expect(
      filterRightSidebarPluginEntries(entries, 'repo-1').map((e) => e.pluginKey)
    ).toEqual(['repo-notes'])
  })

  it('drops global plugins and other repos project plugins', () => {
    const kept = filterRightSidebarPluginEntries(entries, 'repo-2')
    expect(kept.map((e) => e.pluginKey)).toEqual(['other-repo-notes'])
    expect(kept.some((e) => e.pluginKey === 'global-plugin')).toBe(false)
    expect(kept.some((e) => e.pluginKey === 'repo-notes')).toBe(false)
  })

  it('shows nothing without an active repo', () => {
    expect(filterRightSidebarPluginEntries(entries, null)).toEqual([])
  })
})
