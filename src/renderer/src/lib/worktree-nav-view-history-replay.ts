import { useAppStore } from '@/store'
import type { WorktreeNavHistoryViewEntry } from '@/store/slices/worktree-nav-history'

// Why: page entries replay via setActiveView (not open*Page) so back/forward doesn't mutate previousViewBefore* or duplicate history (see navigateToIndex).
export function applyWorktreeNavViewEntry(entry: WorktreeNavHistoryViewEntry): void {
  useAppStore.getState().setActiveView(entry)
}
