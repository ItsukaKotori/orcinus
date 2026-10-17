import { describe, expect, it } from 'vitest'
import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import {
  createVisibleWorktreeTerminalActivityProjection,
  getVisibleWorktreeTerminalActivityTabs
} from './visible-worktree-activity-inputs'

function terminalTab(id: string, title: string): TerminalTab {
  return {
    id,
    ptyId: id,
    worktreeId: 'wt-1',
    title,
    customTitle: null,
    color: null,
    sortOrder: 0,
    createdAt: 0
  }
}


describe('visible worktree activity inputs', () => {
  it('preserves terminal activity projection when only tab metadata changes', () => {
    const first = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'First')]
    })

    const second = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'Renamed')]
    })

    expect(second).toBe(first)
    expect(second['wt-1']).toBe(first['wt-1'])
  })
  it('inspects only the changed bucket in a 300-worktree title publication', () => {
    const inspected: string[] = []
    const projector = createVisibleWorktreeTerminalActivityProjection((worktreeId) =>
      inspected.push(worktreeId)
    )
    const tabs = Object.fromEntries(
      Array.from({ length: 300 }, (_, index) => {
        const worktreeId = `wt-${index}`
        return [worktreeId, [terminalTab(`tab-${index}`, `Title ${index}`)]]
      })
    )
    const first = projector.project(tabs)
    inspected.length = 0
    const changedWorktreeId = 'wt-173'
    const second = projector.project({
      ...tabs,
      [changedWorktreeId]: [{ ...tabs[changedWorktreeId][0], title: 'Changed display title' }]
    })

    expect(second).toBe(first)
    expect(inspected).toEqual([changedWorktreeId])
  })

  it('updates terminal activity projection when tab ids change', () => {
    const first = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'First')]
    })

    const second = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'First'), terminalTab('tab-2', 'Second')]
    })

    expect(second).not.toBe(first)
    expect(second['wt-1']?.map((tab) => tab.id)).toEqual(['tab-1', 'tab-2'])
  })

  it('reuses unchanged terminal worktree arrays when a sibling worktree changes', () => {
    const first = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'First')],
      'wt-2': [terminalTab('tab-2', 'Second')]
    })

    const second = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'Renamed')],
      'wt-2': [terminalTab('tab-3', 'Third')]
    })

    expect(second).not.toBe(first)
    expect(second['wt-1']).toBe(first['wt-1'])
    expect(second['wt-2']).not.toBe(first['wt-2'])
    expect(second['wt-2']?.map((tab) => tab.id)).toEqual(['tab-3'])
  })

  it('drops terminal projections for removed worktrees', () => {
    const first = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'First')],
      'wt-2': [terminalTab('tab-2', 'Second')]
    })

    const second = getVisibleWorktreeTerminalActivityTabs({
      'wt-1': [terminalTab('tab-1', 'Renamed')]
    })

    expect(second).not.toBe(first)
    expect(Object.keys(second)).toEqual(['wt-1'])
    expect(second['wt-1']).toBe(first['wt-1'])
  })

})
