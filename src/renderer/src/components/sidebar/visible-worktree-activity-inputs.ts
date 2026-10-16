import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import { createWorktreeTabBucketProjection } from '@/lib/worktree-tab-bucket-projection'

export type TerminalActivityTab = Pick<TerminalTab, 'id'>

export function createVisibleWorktreeTerminalActivityProjection(
  onInspectBucket?: (worktreeId: string) => void
) {
  return createWorktreeTabBucketProjection<TerminalTab, TerminalActivityTab>({
    projectTab: (tab) => ({ id: tab.id }),
    isSameProjectedTab: (previousTab, nextTab) => previousTab.id === nextTab.id,
    onInspectBucket
  })
}

const terminalProjection = createVisibleWorktreeTerminalActivityProjection()

export function getVisibleWorktreeTerminalActivityTabs(
  tabsByWorktree: Record<string, TerminalTab[]>
): Record<string, TerminalActivityTab[]> {
  return terminalProjection.project(tabsByWorktree)
}

