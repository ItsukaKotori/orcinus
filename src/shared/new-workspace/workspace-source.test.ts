import { describe, expect, it } from 'vitest'
import {
  buildWorkspaceSourceSelection,
  getWorkspaceSourceName,
  getWorkspaceSourceProvider,
  shouldApplyWorkspaceSourceAutoName,
  shouldPreserveWorkspaceSourceOnRepoChange
} from './workspace-source'

describe('workspace source policy', () => {
  it('names and selects a GitHub source', () => {
    const source = {
      provider: 'github' as const,
      type: 'issue' as const,
      number: 42,
      title: 'Ship mobile parity',
      url: 'https://github.com/acme/orca/issues/42'
    }
    expect(getWorkspaceSourceName(source)).toEqual({
      seedName: 'ship-mobile-parity',
      displayName: 'Ship mobile parity'
    })
    expect(buildWorkspaceSourceSelection({ linkedWorkItem: source })).toMatchObject({
      kind: 'github-issue',
      label: '#42 Ship mobile parity'
    })
  })

  it('preserves only workspace-scoped sources on repo change', () => {
    expect(
      shouldPreserveWorkspaceSourceOnRepoChange({
        provider: 'github',
        type: 'issue',
        number: 1,
        title: 'Repo scoped',
        url: 'https://github.com/o/r/issues/1'
      })
    ).toBe(false)
    expect(shouldPreserveWorkspaceSourceOnRepoChange(null)).toBe(false)
  })

  it('shares provider inference and auto-name gates', () => {
    const legacyGitLab = {
      type: 'issue' as const,
      number: 7,
      title: 'Self hosted',
      url: 'https://gitlab.example.com/g/p/-/work_items/7'
    }
    expect(getWorkspaceSourceProvider(legacyGitLab)).toBe('gitlab')
    expect(buildWorkspaceSourceSelection({ linkedWorkItem: legacyGitLab })).toMatchObject({
      kind: 'gitlab-issue',
      label: '#7 Self hosted'
    })
    expect(shouldApplyWorkspaceSourceAutoName({ currentName: '#42', lastAutoName: 'old' })).toBe(
      true
    )
    expect(
      shouldApplyWorkspaceSourceAutoName({ currentName: 'my workspace', lastAutoName: 'old' })
    ).toBe(false)
  })
})
