import { describe, expect, it } from 'vitest'
import type { PluginHostListEntry } from '../../../../preload/api-types'
import { collectPluginTabMenuEntries } from './plugin-tab-menu-entries'

function entry(
  pluginKey: string,
  options: {
    scope: 'global' | 'project'
    repoId?: string
    status?: PluginHostListEntry['status']
    tabs?: { id: string; title: string; icon?: string }[]
  }
): PluginHostListEntry {
  return {
    pluginKey,
    name: `${pluginKey} name`,
    scope: options.scope,
    ...(options.repoId ? { repoId: options.repoId } : {}),
    status: options.status ?? 'idle',
    tabs: options.tabs ?? [{ id: 'dashboard', title: 'Dashboard' }]
  } as PluginHostListEntry
}

describe('collectPluginTabMenuEntries', () => {
  it('lists global plugin tabs and the active repo project tabs only', () => {
    const plugins = [
      entry('global.one', { scope: 'global', tabs: [{ id: 'board', title: 'Board', icon: 'kanban' }] }),
      entry('repo.notes', { scope: 'project', repoId: 'repo-1' }),
      entry('other.notes', { scope: 'project', repoId: 'repo-2' }),
      entry('legacy.notes', { scope: 'project' })
    ]

    const entries = collectPluginTabMenuEntries(plugins, 'repo-1')

    expect(entries.map((item) => [item.pluginKey, item.tabId, item.title])).toEqual([
      ['global.one', 'board', 'Board'],
      ['repo.notes', 'dashboard', 'Dashboard']
    ])
    expect(entries[0]?.scope).toBe('global')
    expect(entries[0]?.icon).toBe('kanban')
    expect(entries[1]?.repoId).toBe('repo-1')
    expect(entries[1]?.pluginName).toBe('repo.notes name')
  })

  it('hides project entries when no repo is active', () => {
    const plugins = [
      entry('global.one', { scope: 'global' }),
      entry('repo.notes', { scope: 'project', repoId: 'repo-1' })
    ]

    expect(collectPluginTabMenuEntries(plugins, null).map((item) => item.pluginKey)).toEqual([
      'global.one'
    ])
  })

  it('hides plugins awaiting consent or disabled/errored', () => {
    const plugins = [
      entry('pending.one', { scope: 'global', status: 'pending' }),
      entry('disabled.one', { scope: 'global', status: 'disabled' }),
      entry('broken.one', { scope: 'global', status: 'errored' }),
      entry('invalid.one', { scope: 'global', status: 'invalid' }),
      entry('running.one', { scope: 'global', status: 'running' })
    ]

    expect(collectPluginTabMenuEntries(plugins, null).map((item) => item.pluginKey)).toEqual([
      'running.one'
    ])
  })

  it('flattens every declared tab and falls back to the tab id for a blank title', () => {
    const plugins = [
      entry('multi.one', {
        scope: 'global',
        tabs: [
          { id: 'first', title: 'First' },
          { id: 'second', title: '  ' }
        ]
      })
    ]

    expect(collectPluginTabMenuEntries(plugins, null).map((item) => item.title)).toEqual([
      'First',
      'second'
    ])
  })

  it('returns nothing when there are no available tabs', () => {
    expect(collectPluginTabMenuEntries([], 'repo-1')).toEqual([])
    expect(
      collectPluginTabMenuEntries([entry('empty.one', { scope: 'global', tabs: [] })], 'repo-1')
    ).toEqual([])
  })
})
