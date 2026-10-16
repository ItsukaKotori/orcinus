import { useCallback } from 'react'
import type { TerminalTab } from '../../../../shared/terminal-tab-types'
import { useAppStore } from '../../store'
import { focusTerminalTabSurface } from '../../lib/focus-terminal-tab-surface'
import {
  createWebRuntimeSessionTerminal,
  isWebRuntimeSessionActive
} from '../../runtime/web-runtime-session'
import { openTabBarEntry, type TabCreateEntryArgs } from '../tab-bar/tab-create-entry-action'
import { getRuntimeEnvironmentIdForWorktree } from '@/lib/worktree-runtime-owner'

export function recordTerminalTabGroupSplit(createdTerminal: TerminalTab | null | undefined): void {
  if (!createdTerminal) {
    return
  }
  useAppStore.getState().recordFeatureInteraction('terminal-pane-split')
}

export function useTabGroupCreationCommands({
  groupId,
  worktreeId
}: {
  groupId: string
  worktreeId: string
}) {
  const focusGroup = useAppStore((state) => state.focusGroup)
  const createTab = useAppStore((state) => state.createTab)
  const setActiveTab = useAppStore((state) => state.setActiveTab)
  const setActiveTabType = useAppStore((state) => state.setActiveTabType)
  const createEmptySplitGroup = useAppStore((state) => state.createEmptySplitGroup)
  const openNewMarkdownInActiveWorkspace = useAppStore(
    (state) => state.openNewMarkdownInActiveWorkspace
  )
  const openNewTerminalTabInActiveWorkspace = useAppStore(
    (state) => state.openNewTerminalTabInActiveWorkspace
  )

  const createSplitGroup = useCallback(
    (direction: 'left' | 'right' | 'up' | 'down') => {
      focusGroup(worktreeId, groupId)
      const newGroupId = createEmptySplitGroup(worktreeId, groupId, direction)
      if (!newGroupId) {
        return
      }
      // Why: this Split entry point always seeds a fresh terminal (tab-drag can open other directions).
      const terminal = createTab(worktreeId, newGroupId)
      recordTerminalTabGroupSplit(terminal)
      setActiveTab(terminal.id)
      setActiveTabType('terminal')
    },
    [
      createEmptySplitGroup,
      createTab,
      focusGroup,
      groupId,
      setActiveTab,
      setActiveTabType,
      worktreeId
    ]
  )

  // Why: these stay unmemoized plain lambdas — the original built them inline in the returned commands object.
  return {
    createSplitGroup,
    openEntry: async (args: TabCreateEntryArgs) => {
      await openTabBarEntry(args)
    },
    // Why: target the owning group explicitly; the "+" menu can fire from an unfocused panel without updating global group focus.
    newFileTab: async () => {
      await openNewMarkdownInActiveWorkspace(groupId)
    },
    newTerminalTab: () => {
      void openNewTerminalTabInActiveWorkspace(groupId)
    },
    newTerminalWithShell: (shellOverride: string) => {
      void (async () => {
        const environmentId = getRuntimeEnvironmentIdForWorktree(useAppStore.getState(), worktreeId)
        const outcome = await createWebRuntimeSessionTerminal({
          worktreeId,
          environmentId,
          targetGroupId: groupId,
          command: shellOverride,
          activate: true
        })
        if (outcome.status === 'created' || isWebRuntimeSessionActive(environmentId)) {
          return
        }
        const terminal = createTab(worktreeId, groupId, shellOverride)
        setActiveTab(terminal.id)
        setActiveTabType('terminal')
        focusTerminalTabSurface(terminal.id)
      })()
    }
  }
}
