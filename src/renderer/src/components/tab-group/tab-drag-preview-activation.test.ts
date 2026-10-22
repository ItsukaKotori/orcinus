import { beforeEach, describe, expect, it } from 'vitest'
import type { Tab } from '../../../../shared/tab-types'
import { useAppStore } from '../../store'
import {
  applyDragPreviewTab,
  captureTabDragActivationSnapshot,
  restoreTabDragActivationSnapshot
} from './tab-drag-preview-activation'

const WT = 'wt-preview-restore'

describe('restoreTabDragActivationSnapshot', () => {
  beforeEach(() => {
    useAppStore.setState({
      activeWorktreeId: WT,
      activeTabType: 'terminal',
      activeTabId: 'terminal-1',
      activeTabIdByWorktree: { [WT]: 'terminal-1' },
      activeTabTypeByWorktree: { [WT]: 'terminal' },
      groupsByWorktree: {
        [WT]: [
          {
            id: 'group-1',
            worktreeId: WT,
            activeTabId: 'tab-1',
            tabOrder: ['tab-1', 'tab-2']
          }
        ]
      },
      unifiedTabsByWorktree: {
        [WT]: [
          {
            id: 'tab-1',
            groupId: 'group-1',
            worktreeId: WT,
            contentType: 'terminal',
            entityId: 'terminal-1',
            label: 'Terminal 1',
            customLabel: null,
            color: null,
            sortOrder: 0,
            createdAt: 0
          } satisfies Tab,
          {
            id: 'tab-2',
            groupId: 'group-1',
            worktreeId: WT,
            contentType: 'editor',
            entityId: 'editor-1',
            label: 'Editor',
            customLabel: null,
            color: null,
            sortOrder: 1,
            createdAt: 1
          } satisfies Tab
        ]
      },
      activeGroupIdByWorktree: { [WT]: 'group-1' }
    })
  })

  it('restores active-surface fields after a drag preview is cancelled', () => {
    const snapshot = captureTabDragActivationSnapshot(WT)

    applyDragPreviewTab({
      worktreeId: WT,
      groupId: 'group-1',
      tabId: 'tab-2',
      activeGroupId: 'group-1'
    })

    expect(useAppStore.getState().activeTabType).toBe('editor')
    expect(useAppStore.getState().activeFileId).toBe('editor-1')

    restoreTabDragActivationSnapshot(WT, snapshot)

    const state = useAppStore.getState()
    expect(state.groupsByWorktree[WT]?.[0]?.activeTabId).toBe('tab-1')
    expect(state.activeTabType).toBe('terminal')
    expect(state.activeTabId).toBe('terminal-1')
    expect(state.activeTabIdByWorktree[WT]).toBe('terminal-1')
  })

  it('does not fold a git-log tab into the editor surface during drag preview', () => {
    // Why: previewActiveSurfacePatch is a hand-copied mirror that once omitted
    // the git-log branch, so hovering/reordering while Git Log is active wrote
    // activeFileId = 'git-log/<wt>' + activeTabType 'editor' (blank editor pane,
    // lost highlight, and a permanently-wrong surface after a same-group reorder).
    useAppStore.setState((state) => ({
      unifiedTabsByWorktree: {
        [WT]: [
          ...(state.unifiedTabsByWorktree[WT] ?? []),
          {
            id: 'git-log-tab',
            groupId: 'group-1',
            worktreeId: WT,
            contentType: 'git-log',
            entityId: 'git-log-tab',
            label: 'Git Log',
            customLabel: null,
            color: null,
            sortOrder: 2,
            createdAt: 2,
            gitLog: { worktreeId: WT }
          } satisfies Tab
        ]
      }
    }))

    applyDragPreviewTab({
      worktreeId: WT,
      groupId: 'group-1',
      tabId: 'git-log-tab',
      activeGroupId: 'group-1'
    })

    const state = useAppStore.getState()
    expect(state.activeTabType).toBe('git-log')
    expect(state.activeTabTypeByWorktree[WT]).toBe('git-log')
    expect(state.activeFileId ?? null).not.toBe('git-log-tab')
  })
})
