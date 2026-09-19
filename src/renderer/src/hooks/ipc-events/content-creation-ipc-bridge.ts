import {
  createFloatingWorkspaceBrowserTab,
  createFloatingWorkspaceMarkdownTab,
  isFloatingWorkspacePanelFocused
} from '@/lib/floating-workspace-terminal-actions'
import { translate } from '@/i18n/i18n'
import { toast } from 'sonner'
import { useAppStore } from '../../store'

export function registerContentCreationIpcBridge(unsubs: (() => void)[]): void {
  unsubs.push(
    window.api.ui.onNewBrowserTab(() => {
      const store = useAppStore.getState()
      if (isFloatingWorkspacePanelFocused()) {
        void createFloatingWorkspaceBrowserTab(store).catch((error) => {
          toast.error(error instanceof Error ? error.message : String(error))
        })
        return
      }
      const worktreeId = store.activeWorktreeId
      if (!worktreeId) {
        return
      }
      const targetGroupId =
        store.activeGroupIdByWorktree[worktreeId] ?? store.groupsByWorktree[worktreeId]?.[0]?.id
      if (!targetGroupId) {
        return
      }
      void store.openNewBrowserTabInActiveWorkspace(targetGroupId).catch((error) => {
        toast.error(error instanceof Error ? error.message : String(error))
      })
    })
  )

  unsubs.push(
    window.api.ui.onNewMarkdownTab(() => {
      const store = useAppStore.getState()
      if (isFloatingWorkspacePanelFocused()) {
        void createFloatingWorkspaceMarkdownTab(store).catch((err) => {
          toast.error(
            err instanceof Error
              ? err.message
              : translate(
                  'auto.hooks.useIpcEvents.56d3ec4203',
                  'Failed to create untitled markdown file.'
                )
          )
        })
        return
      }
      const worktreeId = store.activeWorktreeId
      if (!worktreeId) {
        return
      }
      const targetGroupId =
        store.activeGroupIdByWorktree[worktreeId] ?? store.groupsByWorktree[worktreeId]?.[0]?.id
      if (targetGroupId) {
        void store.openNewMarkdownInActiveWorkspace(targetGroupId)
      }
    })
  )

  // Why: reply with the page ID so main can await registerGuest before returning to the CLI.
}
