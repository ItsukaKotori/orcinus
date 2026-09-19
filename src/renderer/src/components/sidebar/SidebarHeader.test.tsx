// @vitest-environment happy-dom

import React, { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import SidebarHeader from './SidebarHeader'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

const mocks = vi.hoisted(() => {
  const shortcutLabel: { current: string | null } = { current: '⌘N' }

  return {
    shortcutLabel,
    toast: vi.fn()
  }
})

type MockState = {
  groupBy: string
  keybindings: Record<string, string[]>
  openModal: (modal: string, data?: unknown) => void
}

let mockState: MockState

vi.mock('@/store', () => {
  const useAppStore = (selector: (state: MockState) => unknown) => selector(mockState)
  useAppStore.getState = () => mockState
  return { useAppStore }
})

vi.mock('./SidebarWorkspaceOptionsMenu', () => ({
  default: () => <button aria-label="Workspace options" type="button" />
}))

vi.mock('./workspace-options-menu-items', () => ({
  useWorkspaceOptionsFilterBadge: () => ({
    hasAnyFilter: false,
    activeFilterCount: 0,
    activeFilterLabel: '0 filters'
  }),
  WorkspaceOptionsMenuItems: () => null
}))

vi.mock('@/hooks/useShortcutLabel', () => ({
  useShortcutLabel: () => '⌘N',
  formatOptionalPrimaryShortcutLabel: () => mocks.shortcutLabel.current
}))

vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: React.ReactNode }) => <>{children}</>
}))

vi.mock('sonner', () => ({ toast: mocks.toast }))

let container: HTMLDivElement
let root: Root

function mountHeader(): void {
  if (root) {
    act(() => root.unmount())
    container.remove()
  }
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  act(() => {
    root.render(<SidebarHeader />)
  })
}

function headerButton(label: string): HTMLButtonElement {
  const button = container.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)
  if (!button) {
    throw new Error(`Header button not rendered: ${label}`)
  }
  return button
}

function createButton(): HTMLButtonElement {
  return headerButton('New workspace')
}

beforeEach(() => {
  mocks.toast.mockClear()
  mocks.shortcutLabel.current = '⌘N'
  mockState = {
    groupBy: 'repo',
    keybindings: {},
    openModal: vi.fn()
  }
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

describe('SidebarHeader', () => {
  it('keeps New workspace clickable with zero projects, since the composer adds the first one', async () => {
    mountHeader()

    expect(createButton().disabled).toBe(false)

    await act(async () => {
      createButton().click()
    })

    expect(mockState.openModal).toHaveBeenCalledWith('new-workspace-composer', {
      telemetrySource: 'sidebar'
    })
  })

  it('reaches Add project and New workspace in one click each, with no menu', async () => {
    mountHeader()

    expect(headerButton('New workspace')).toBeTruthy()
    expect(headerButton('Add project')).toBeTruthy()
    expect(container.querySelector('[data-slot="dropdown-menu-trigger"]')).toBeNull()

    await act(async () => {
      headerButton('Add project').click()
    })

    expect(mockState.openModal).toHaveBeenCalledWith('add-repo')
  })

  it('keeps the create button rightmost so the frequent action stays where it was', () => {
    mountHeader()

    const labels = [...container.querySelectorAll<HTMLElement>('[aria-label]')]
      .map((node) => node.getAttribute('aria-label'))
      .filter((label): label is string => label === 'Add project' || label === 'New workspace')
    expect(labels).toEqual(['Add project', 'New workspace'])
  })

  it('advertises the workspace shortcut on the create tooltip, and omits it when unassigned', () => {
    mountHeader()
    expect(container.textContent).toContain('⌘N')

    mocks.shortcutLabel.current = null
    mountHeader()
    expect(container.textContent).not.toContain('⌘N')
  })

  it('uses the legacy title based on workspace grouping', () => {
    mountHeader()

    expect(container.querySelector('[data-sidebar-section-title="projects"]')?.textContent).toBe(
      'Projects'
    )

    mockState.groupBy = 'workspace-status'
    mountHeader()
    expect(container.querySelector('[data-sidebar-section-title="workspaces"]')?.textContent).toBe(
      'Workspaces'
    )
  })

  it('keeps the header actions on one row at the default sidebar width', () => {
    mountHeader()

    const headerRow = container.querySelector('.mt-2')
    const headerClasses = new Set(headerRow?.className.split(/\s+/) ?? [])
    expect(headerClasses.has('flex-wrap')).toBe(false)
    expect(headerClasses.has('h-8')).toBe(true)
    expect(container.querySelector('[aria-label="Add project"]')).toBeTruthy()
    expect(container.querySelector('[aria-label="New workspace"]')).toBeTruthy()
  })

  it('renders the same actions on both sides of the old wide-layout breakpoint', () => {
    mountHeader()
    expect(container.querySelector('[aria-label="More workspace actions"]')).toBeNull()
    expect(container.querySelector('[aria-label="Add project"]')).toBeTruthy()
    expect(container.querySelector('[aria-label="New workspace"]')).toBeTruthy()
    expect(container.querySelector('[aria-label="Workspace options"]')).toBeTruthy()
  })
})
