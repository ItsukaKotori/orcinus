import type { WorkspaceCleanupReviewInfo } from './workspace-cleanup-presentation'

/** Outline-pill tone for the cleanup review chip: purple merged, rose closed, slate draft, emerald open, neutral unknown. */
export function getWorkspaceCleanupReviewStateTone(
  state: WorkspaceCleanupReviewInfo['state']
): string {
  if (state === 'merged') {
    return 'border-purple-500/30 bg-purple-500/10 text-purple-600 dark:text-purple-300'
  }
  if (state === 'draft') {
    return 'border-slate-500/30 bg-slate-500/10 text-slate-600 dark:text-slate-300'
  }
  if (state === 'closed') {
    return 'border-rose-500/30 bg-rose-500/10 text-rose-600 dark:text-rose-300'
  }
  if (state === 'open') {
    return 'border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
  }
  return 'border-border bg-background text-muted-foreground'
}
