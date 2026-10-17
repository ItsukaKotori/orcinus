import { comparePaletteRankedItems } from '@/lib/cmd-j-section-leadership'
import type { WorkspaceTabPaletteSearchResult } from '@/lib/workspace-tab-palette-search'
import type { OpenTabPaletteItem, WorkspaceTabPaletteItem } from './worktree-jump-palette-model'

export function buildWorkspaceTabPaletteItems(
  results: readonly WorkspaceTabPaletteSearchResult[]
): WorkspaceTabPaletteItem[] {
  return results.map((result) => ({
    id: result.paletteIdentity,
    type: 'workspace-tab',
    result
  }))
}

export function buildOpenTabPaletteItems({
  workspaceTabItems
}: {
  workspaceTabItems: readonly WorkspaceTabPaletteItem[]
}): OpenTabPaletteItem[] {
  return [...workspaceTabItems].sort((left, right) =>
    comparePaletteRankedItems(
      {
        rank: left.result.rank,
        order: left.result.score,
        identity: left.id,
        activity: left.result.activity
      },
      {
        rank: right.result.rank,
        order: right.result.score,
        identity: right.id,
        activity: right.result.activity
      }
    )
  )
}
