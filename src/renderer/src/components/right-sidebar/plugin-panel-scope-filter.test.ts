import { describe, expect, it } from 'vitest'
import { filterRightSidebarPluginEntries } from './plugin-panel-activity-items'
import { MOCK_GLOBAL_PLUGINS, MOCK_PROJECT_PLUGINS } from '../../../../bridge/mock/fixtures'

describe('filterRightSidebarPluginEntries', () => {
  it('keeps project-scoped and unset entries, drops global', () => {
    const kept = filterRightSidebarPluginEntries([
      ...MOCK_GLOBAL_PLUGINS,
      ...MOCK_PROJECT_PLUGINS,
      { pluginKey: 'legacy', scope: undefined }
    ] as Array<{ pluginKey: string; scope?: 'global' | 'project' }>)
    expect(kept.map((e) => e.pluginKey)).toEqual(['repo-notes', 'legacy'])
  })
})
