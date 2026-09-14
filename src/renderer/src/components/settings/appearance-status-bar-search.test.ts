import { describe, expect, it, vi } from 'vitest'

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => fallback
}))

vi.mock('@/i18n/localized-catalog', () => ({
  createLocalizedCatalog:
    <T>(loader: () => T) =>
    () =>
      loader()
}))

vi.mock('./settings-search-keywords', () => ({
  translateSearchKeyword: (_key: string, fallback: string) => [fallback]
}))

import { getStatusBarToggles } from './appearance-status-bar-search'

describe('getStatusBarToggles', () => {
  it('keeps the surviving status bar toggles only', () => {
    expect(getStatusBarToggles().map((entry) => entry.id)).toEqual(['resource-usage', 'ports'])
  })

  it('drops the removed provider and ssh toggles', () => {
    const ids = getStatusBarToggles().map((entry) => entry.id)

    for (const removed of [
      'claude',
      'codex',
      'gemini',
      'antigravity',
      'opencode-go',
      'kimi',
      'minimax',
      'grok',
      'ssh'
    ]) {
      expect(ids).not.toContain(removed)
    }
  })
})
