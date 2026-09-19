import type { ActivateAndRevealResult } from '@/lib/worktree-activation'

export function finalizeFullCreation(args: {
  setSidebarOpen: (open: boolean) => void
  persistDraft: boolean
  clearNewWorkspaceDraft: () => void
  onCreated?: () => void
  worktreeId: string
  activation: ActivateAndRevealResult | false
  queueWorkspaceActivationTerminalFocus: (
    worktreeId: string,
    activation: ActivateAndRevealResult | false
  ) => void
}): void {
  args.setSidebarOpen(true)
  if (args.persistDraft) {
    args.clearNewWorkspaceDraft()
  }
  args.onCreated?.()
  args.queueWorkspaceActivationTerminalFocus(args.worktreeId, args.activation)
}
