// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PluginHostListEntry } from '../../../../preload/api-types'

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => fallback
}))

vi.mock('@/components/ui/dropdown-menu', () => ({
  DropdownMenuItem: ({
    children,
    onSelect
  }: {
    children: React.ReactNode
    onSelect?: () => void
  }) => (
    <button type="button" role="menuitem" onClick={onSelect}>
      {children}
    </button>
  ),
  DropdownMenuLabel: ({ children }: { children: React.ReactNode }) => (
    <div role="presentation">{children}</div>
  ),
  DropdownMenuSeparator: () => <div role="separator" />
}))

const { pluginState, ensureLoadedMock, appState } = vi.hoisted(() => ({
  pluginState: { plugins: [] as PluginHostListEntry[] },
  ensureLoadedMock: vi.fn(),
  appState: {
    openPluginTab: vi.fn(),
    repoIdByWorktree: {} as Record<string, string | null>
  }
}))

vi.mock('@/store/plugin-panels', () => ({
  usePluginPanelsStore: (selector: (state: { plugins: PluginHostListEntry[] }) => unknown) =>
    selector(pluginState),
  ensurePluginPanelsLoaded: ensureLoadedMock
}))

vi.mock('@/store', () => ({
  useAppStore: (
    selector: (state: {
      openPluginTab: typeof appState.openPluginTab
      getKnownWorktreeById: (worktreeId: string) => { repoId: string | null } | undefined
    }) => unknown
  ) =>
    selector({
      openPluginTab: appState.openPluginTab,
      getKnownWorktreeById: (worktreeId: string) =>
        worktreeId in appState.repoIdByWorktree
          ? { repoId: appState.repoIdByWorktree[worktreeId] ?? null }
          : undefined
    })
}))

import { TabBarPluginTabMenuItems } from './TabBarPluginTabMenuItems'

;(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true

function entry(
  pluginKey: string,
  options: {
    scope: 'global' | 'project'
    repoId?: string
    tabs: { id: string; title: string }[]
  }
): PluginHostListEntry {
  return {
    pluginKey,
    name: `${pluginKey} name`,
    scope: options.scope,
    ...(options.repoId ? { repoId: options.repoId } : {}),
    status: 'idle',
    tabs: options.tabs
  } as PluginHostListEntry
}

let container: HTMLDivElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  pluginState.plugins = []
  appState.repoIdByWorktree = {}
  appState.openPluginTab.mockReset()
  ensureLoadedMock.mockReset()
})

afterEach(async () => {
  await act(async () => {
    root.unmount()
  })
  container.remove()
})

function renderMenu(worktreeId: string): void {
  root.render(<TabBarPluginTabMenuItems worktreeId={worktreeId} />)
}

describe('TabBarPluginTabMenuItems', () => {
  it('lists global and active-repo tabs and hides other repos', async () => {
    pluginState.plugins = [
      entry('global.one', { scope: 'global', tabs: [{ id: 'board', title: 'Board' }] }),
      entry('repo.notes', {
        scope: 'project',
        repoId: 'repo-1',
        tabs: [{ id: 'notes', title: 'Notes' }]
      }),
      entry('other.notes', {
        scope: 'project',
        repoId: 'repo-2',
        tabs: [{ id: 'notes', title: 'Other' }]
      })
    ]
    appState.repoIdByWorktree = { wt1: 'repo-1' }

    await act(async () => {
      renderMenu('wt1')
    })

    const labels = Array.from(container.querySelectorAll('[role="menuitem"]')).map(
      (item) => item.textContent
    )
    expect(labels).toEqual(['Boardglobal.one name', 'Notesrepo.notes name'])
    expect(container.textContent).toContain('插件 Tab')
    expect(ensureLoadedMock).toHaveBeenCalled()
  })

  it('opens a fresh store instance per selection without params', async () => {
    pluginState.plugins = [
      entry('global.one', { scope: 'global', tabs: [{ id: 'board', title: 'Board' }] })
    ]

    await act(async () => {
      renderMenu('wt1')
    })

    const item = container.querySelector('[role="menuitem"]') as HTMLButtonElement
    await act(async () => {
      item.click()
      item.click()
    })

    expect(appState.openPluginTab).toHaveBeenCalledTimes(2)
    const first = appState.openPluginTab.mock.calls[0]![0] as Record<string, unknown>
    const second = appState.openPluginTab.mock.calls[1]![0] as Record<string, unknown>
    expect(first).toMatchObject({
      pluginKey: 'global.one',
      tabId: 'board',
      scope: 'global',
      title: 'Board',
      worktreeId: 'wt1'
    })
    expect(first).not.toHaveProperty('params')
    expect(typeof first.instanceId).toBe('string')
    expect(first.instanceId).not.toBe(second.instanceId)
  })

  it('renders nothing when no plugin tabs are available', async () => {
    pluginState.plugins = [
      entry('repo.notes', { scope: 'project', repoId: 'repo-9', tabs: [{ id: 'notes', title: 'Notes' }] })
    ]
    appState.repoIdByWorktree = { wt1: 'repo-1' }

    await act(async () => {
      renderMenu('wt1')
    })

    expect(container.textContent).toBe('')
    expect(container.querySelector('[role="menuitem"]')).toBeNull()
  })
})
