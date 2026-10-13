import { describe, expect, it } from 'vitest'
import { toVisibleTabType, type Tab, type TabGroup } from '../../../../../shared/tab-types'
import type { WorkspaceSessionState } from '../../../../../shared/workspace-session-state-types'
import { buildHydratedTabState } from '../tabs-hydration'
import { createTestStore, seedStore } from '../store-test-helpers'
import { openPluginTabBindingId, openPluginTabLabel } from './tabs-plugin-actions'

const WT = 'repo1::/workspace'
const OTHER_WT = 'repo1::/other'

type Store = ReturnType<typeof createTestStore>

function makeStore(activeWorktreeId: string | null = WT): Store {
  const store = createTestStore()
  seedStore(store, { activeWorktreeId })
  return store
}

const baseInput = {
  pluginKey: 'acme.plugin',
  tabId: 'dashboard',
  scope: 'global' as const,
  instanceId: 'inst-1',
  title: 'Dashboard',
  worktreeId: WT
}

describe('plugin tab model', () => {
  it('projects plugin content to the plugin visible type, editor still collapses', () => {
    expect(toVisibleTabType('plugin')).toBe('plugin')
    expect(toVisibleTabType('diff')).toBe('editor')
  })
})

describe('openPluginTab', () => {
  it('creates a plugin tab with the binding id, binding, and appended order', () => {
    const store = makeStore()

    const tab = store.getState().openPluginTab(baseInput)

    expect(tab).not.toBeNull()
    expect(tab?.contentType).toBe('plugin')
    expect(tab?.id).toBe('acme.plugin/dashboard/inst-1')
    expect(tab?.entityId).toBe(tab?.id)
    expect(tab?.worktreeId).toBe(WT)
    expect(tab?.label).toBe('Dashboard')
    expect(tab?.sortOrder).toBe(0)
    expect(tab?.pluginTab).toEqual({
      pluginKey: 'acme.plugin',
      tabId: 'dashboard',
      scope: 'global',
      instanceId: 'inst-1',
      titleHint: 'Dashboard'
    })
    const group = store.getState().groupsByWorktree[WT]?.[0]
    expect(group?.tabOrder).toEqual([tab?.id])
    expect(group?.activeTabId).toBe(tab?.id)
    expect(store.getState().activeGroupIdByWorktree[WT]).toBe(group?.id)
  })

  it('falls back to titleHint, then tabId, and caps the label at 128 chars', () => {
    const store = makeStore()
    const longTitle = 'x'.repeat(200)
    const { title: _omitted, ...withoutTitle } = baseInput

    store.getState().openPluginTab({ ...withoutTitle, instanceId: 'i1' })
    store.getState().openPluginTab({
      ...withoutTitle,
      title: '   ',
      instanceId: 'i2',
      params: { a: 1 }
    })
    store.getState().openPluginTab({ ...withoutTitle, title: longTitle, instanceId: 'i3' })

    const labels = (store.getState().unifiedTabsByWorktree[WT] ?? []).map((tab) => tab.label)
    expect(labels).toEqual(['dashboard', 'dashboard', longTitle.slice(0, 128)])
    expect(openPluginTabLabel({ title: '  ', titleHint: 'Hint', tabId: 'x' })).toBe('Hint')
  })

  it('dedupes by instanceId within the worktree and focuses the existing tab', () => {
    const store = makeStore()
    const first = store.getState().openPluginTab(baseInput)
    store.getState().openPluginTab({ ...baseInput, instanceId: 'inst-2', title: 'Other' })

    const reopened = store.getState().openPluginTab(baseInput)

    expect(reopened?.id).toBe(first?.id)
    expect(store.getState().unifiedTabsByWorktree[WT]).toHaveLength(2)
    expect(store.getState().getActiveTab(WT)?.id).toBe(first?.id)
    expect(store.getState().activeTabType).toBe('plugin')
  })

  it('creates in a non-active worktree without stealing focus', () => {
    const store = makeStore(WT)
    store.getState().openPluginTab({
      ...baseInput,
      worktreeId: OTHER_WT,
      instanceId: 'inst-bg'
    })

    const [bg] = store.getState().unifiedTabsByWorktree[OTHER_WT] ?? []
    expect(bg?.id).toBe('acme.plugin/dashboard/inst-bg')
    // Why: the tab is selected inside its own group (matching other openers),
    // but the visible worktree and active surface belong to the user's one.
    expect(store.getState().groupsByWorktree[OTHER_WT]?.[0]?.activeTabId).toBe(bg?.id)
    expect(store.getState().activeWorktreeId).toBe(WT)
    expect(store.getState().activeTabType).not.toBe('plugin')
    expect(store.getState().activeTabTypeByWorktree[OTHER_WT]).toBeUndefined()
  })
})

describe('plugin tab close / recently closed', () => {
  it('records the plugin kind in the recently-closed stack on close', () => {
    const store = makeStore()
    const tab = store.getState().openPluginTab(baseInput)

    store.getState().closeUnifiedTab(tab!.id)

    expect(store.getState().unifiedTabsByWorktree[WT]).toEqual([])
    expect(store.getState().recentlyClosedTabKindsByWorktree[WT]).toEqual(['plugin'])
  })

  it('reopen skips the plugin kind instead of misrouting to the editor stack', () => {
    const store = makeStore()
    const tab = store.getState().openPluginTab(baseInput)
    store.getState().closeUnifiedTab(tab!.id)

    expect(store.getState().reopenClosedTab(WT)).toBe(false)
    expect(store.getState().recentlyClosedTabKindsByWorktree[WT]).toEqual([])
  })

  it('pins and unpins plugin tabs generically', () => {
    const store = makeStore()
    const tab = store.getState().openPluginTab(baseInput)

    store.getState().pinTab(tab!.id)
    expect(store.getState().getTab(tab!.id)?.isPinned).toBe(true)
    store.getState().unpinTab(tab!.id)
    expect(store.getState().getTab(tab!.id)?.isPinned).toBe(false)
  })
})

describe('plugin tab hydration', () => {
  function group(overrides: Partial<TabGroup> = {}): TabGroup {
    return {
      id: 'group-1',
      worktreeId: WT,
      activeTabId: null,
      tabOrder: [],
      ...overrides
    }
  }

  function session(tabs: Tab[]): WorkspaceSessionState {
    return {
      tabsByWorktree: {},
      unifiedTabs: { [WT]: tabs },
      tabGroups: {
        [WT]: [group({ activeTabId: tabs[0]?.id ?? null, tabOrder: tabs.map((tab) => tab.id) })]
      },
      activeGroupIdByWorktree: { [WT]: 'group-1' },
      tabGroupLayouts: { [WT]: { type: 'leaf', groupId: 'group-1' } }
    } as unknown as WorkspaceSessionState
  }

  it('hydrates old persisted tabs without a pluginTab field unchanged', () => {
    const legacyTab: Tab = {
      id: 'legacy-terminal',
      entityId: 'legacy-terminal',
      groupId: 'group-1',
      worktreeId: WT,
      contentType: 'terminal',
      label: 'Terminal 1',
      customLabel: null,
      color: null,
      sortOrder: 0,
      createdAt: 1
    }

    const hydrated = buildHydratedTabState(session([legacyTab]), new Set([WT]))

    expect(hydrated.unifiedTabsByWorktree[WT]).toEqual([legacyTab])
    expect(hydrated.unifiedTabsByWorktree[WT]?.[0]?.pluginTab).toBeUndefined()
  })

  it('hydrates unknown plugin tabs as entries (plugin gone at render time)', () => {
    const unknownPluginTab: Tab = {
      id: openPluginTabBindingId({
        pluginKey: 'gone.plugin',
        tabId: 'panel',
        instanceId: 'inst-x'
      }),
      entityId: 'gone.plugin/panel/inst-x',
      groupId: 'group-1',
      worktreeId: WT,
      contentType: 'plugin',
      label: 'Panel',
      customLabel: null,
      color: null,
      sortOrder: 0,
      createdAt: 1,
      pluginTab: {
        pluginKey: 'gone.plugin',
        tabId: 'panel',
        scope: 'project',
        repoId: 'repo1',
        instanceId: 'inst-x',
        params: { query: 'q' }
      }
    }

    const hydrated = buildHydratedTabState(session([unknownPluginTab]), new Set([WT]))

    const [tab] = hydrated.unifiedTabsByWorktree[WT] ?? []
    expect(tab?.id).toBe(unknownPluginTab.id)
    expect(tab?.pluginTab).toEqual(unknownPluginTab.pluginTab)
    expect(hydrated.groupsByWorktree[WT]?.[0]?.tabOrder).toEqual([unknownPluginTab.id])
  })
})
