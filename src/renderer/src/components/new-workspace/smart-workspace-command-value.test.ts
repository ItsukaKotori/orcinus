import { describe, expect, it } from 'vitest'
import {
  resolveSmartWorkspaceCommandValue,
  type SmartWorkspaceCommandRow
} from './smart-workspace-command-value'

function row(kind: SmartWorkspaceCommandRow['kind'], value: string): SmartWorkspaceCommandRow {
  return { kind, value }
}

describe('resolveSmartWorkspaceCommandValue', () => {
  it('keeps the current command value when the row still exists', () => {
    expect(
      resolveSmartWorkspaceCommandValue({
        currentValue: 'branch-main',
        rows: [row('use-name', 'use-name'), row('branch', 'branch-main')],
        isQueryStale: false
      })
    ).toBe('branch-main')
  })

  it('falls back to the first row when the current value is no longer rendered', () => {
    expect(
      resolveSmartWorkspaceCommandValue({
        currentValue: 'branch-other',
        rows: [row('use-name', 'use-name'), row('branch', 'branch-main')],
        isQueryStale: false
      })
    ).toBe('use-name')
  })

  it('keeps typed text armed while the query is ahead of debounced search', () => {
    expect(
      resolveSmartWorkspaceCommandValue({
        currentValue: 'branch-main',
        rows: [row('use-name', 'use-name'), row('branch', 'branch-main')],
        isQueryStale: true
      })
    ).toBe('use-name')
  })

  it('falls back to typed-text when a frozen arm is no longer rendered', () => {
    expect(
      resolveSmartWorkspaceCommandValue({
        currentValue: 'branch-other',
        rows: [row('use-name', 'use-name'), row('branch', 'branch-main')],
        isQueryStale: true
      })
    ).toBe('use-name')
  })

  it('does not resurrect a stale arm when the query settles', () => {
    const rows = [row('use-name', 'use-name'), row('branch', 'branch-main')]
    const typedTextArm = resolveSmartWorkspaceCommandValue({
      currentValue: 'branch-main',
      rows,
      isQueryStale: true
    })

    expect(
      resolveSmartWorkspaceCommandValue({
        currentValue: typedTextArm,
        rows,
        isQueryStale: false
      })
    ).toBe('use-name')
  })

  it('falls back to the first row when stale with no typed-text row', () => {
    expect(
      resolveSmartWorkspaceCommandValue({
        currentValue: 'branch-other',
        rows: [row('branch', 'branch-main')],
        isQueryStale: true
      })
    ).toBe('branch-main')
  })

  it('leaves the current value alone when no rows are rendered', () => {
    expect(
      resolveSmartWorkspaceCommandValue({
        currentValue: 'branch-main',
        rows: [],
        isQueryStale: false
      })
    ).toBe('branch-main')
  })
})
