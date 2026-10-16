import { beforeEach, describe, expect, it, vi } from 'vitest'
import { toWebTerminalSurfaceTabId } from '../../../shared/terminal-surface-id'
import type { RuntimeMobileSessionTabsResult } from '../../../shared/runtime-types'
import type { Tab, TabGroupLayoutNode } from '../../../shared/tab-types'
import {
  recordWebSessionTerminalPlacement,
  resetWebSessionTerminalPlacementsForTests
} from './web-session-terminal-placement'
import { applyWebSessionTabsSnapshot, type WebSessionTabsSyncState } from './web-session-tabs-sync'
import { reconcileClientOwnedTabPlacement } from './web-session-client-owned-tab-placement'
import {
  ENV,
  LEAF_ID,
  NOW,
  SECOND_LEAF_ID,
  THIRD_LEAF_ID,
  WT,
  makeSnapshot,
  makeState,
  resetWebSessionTabsSyncTestState
} from './web-session-tabs-sync-test-harness'

vi.mock('../store', () => ({
  useAppStore: {
    setState: vi.fn()
  }
}))

const HOST_GROUP = 'host-group-1'
const PREVIEW_GROUP = 'client-preview-group'
const BROWSER_HOST_TAB = 'host-browser-tab'
const T1 = toWebTerminalSurfaceTabId('host-tab-1')
const T2 = toWebTerminalSurfaceTabId('host-tab-2')
const T3 = toWebTerminalSurfaceTabId('host-tab-3')

const SPLIT_LAYOUT: TabGroupLayoutNode = {
  type: 'split',
  direction: 'horizontal',
  first: { type: 'leaf', groupId: HOST_GROUP },
  second: { type: 'leaf', groupId: PREVIEW_GROUP },
  ratio: 0.5
}

function terminalSurface(
  hostTabId: string,
  leafId: string,
  terminalId: string,
  isActive = false
): RuntimeMobileSessionTabsResult['tabs'][number] {
  return {
    type: 'terminal',
    id: `${hostTabId}::${leafId}`,
    title: hostTabId,
    parentTabId: hostTabId,
    leafId,
    isActive,
    status: 'ready',
    terminal: terminalId
  }
}

function terminalUnifiedTab(id: string, groupId: string, sortOrder: number): Tab {
  return {
    id,
    worktreeId: WT,
    groupId,
    contentType: 'terminal',
    entityId: id,
    label: id,
    sortOrder,
    createdAt: NOW,
    isPreview: false,
    isPinned: false,
    customLabel: null,
    color: null
  }
}

/** Client already split: left pane mirrors the host group, right pane is a client-only pane. */
function splitClientState(
  overrides: Partial<WebSessionTabsSyncState> = {}
): WebSessionTabsSyncState {
  return makeState({
    activeGroupIdByWorktree: { [WT]: PREVIEW_GROUP },
    activeTabIdByWorktree: { [WT]: T2 },
    groupsByWorktree: {
      [WT]: [
        { id: HOST_GROUP, worktreeId: WT, activeTabId: T2, tabOrder: [T1, T2] },
        {
          id: PREVIEW_GROUP,
          worktreeId: WT,
          activeTabId: BROWSER_HOST_TAB,
          tabOrder: [BROWSER_HOST_TAB]
        }
      ]
    },
    layoutByWorktree: { [WT]: SPLIT_LAYOUT },
    unifiedTabsByWorktree: {
      [WT]: [terminalUnifiedTab(T1, HOST_GROUP, 0), terminalUnifiedTab(T2, HOST_GROUP, 1)]
    },
    ...overrides
  })
}

function groupById(patch: Partial<WebSessionTabsSyncState>, groupId: string) {
  return patch.groupsByWorktree?.[WT]?.find((group) => group.id === groupId)
}

function layoutLeafGroupIds(layout: TabGroupLayoutNode): string[] {
  return layout.type === 'leaf'
    ? [layout.groupId]
    : [...layoutLeafGroupIds(layout.first), ...layoutLeafGroupIds(layout.second)]
}

describe('client-owned tab placement for paired worktrees', () => {
  beforeEach(() => {
    resetWebSessionTabsSyncTestState()
    resetWebSessionTerminalPlacementsForTests()
  })

  it('adopts the host group order when the client holds no groups for the worktree', () => {
    const patch = applyWebSessionTabsSnapshot(
      makeState(),
      makeSnapshot(
        [
          terminalSurface('host-tab-1', LEAF_ID, 'terminal-1', true),
          terminalSurface('host-tab-2', SECOND_LEAF_ID, 'terminal-2'),
          terminalSurface('host-tab-3', THIRD_LEAF_ID, 'terminal-3')
        ],
        {
          activeGroupId: HOST_GROUP,
          activeTabId: `host-tab-1::${LEAF_ID}`,
          tabGroups: [
            {
              id: HOST_GROUP,
              activeTabId: 'host-tab-1',
              tabOrder: ['host-tab-1', 'host-tab-2', 'host-tab-3'],
              recentTabIds: []
            }
          ]
        }
      ),
      ENV,
      NOW
    ) as Partial<WebSessionTabsSyncState>

    expect(patch.groupsByWorktree?.[WT]?.map((group) => group.id)).toEqual([HOST_GROUP])
    expect(groupById(patch, HOST_GROUP)?.tabOrder).toEqual([T1, T2, T3])
  })

  // Relative placement of the surviving tabs is pinned by the append test above.

  // RED: a client-only pane holding only mirrored terminals is folded back into the host group, so
  // the next create lands in the left pane. Not covered above.
  it('keeps a client-only pane that holds a mirrored terminal focused across an ambient snapshot', () => {
    const state = makeState({
      activeGroupIdByWorktree: { [WT]: PREVIEW_GROUP },
      activeTabIdByWorktree: { [WT]: T2 },
      groupsByWorktree: {
        [WT]: [
          { id: HOST_GROUP, worktreeId: WT, activeTabId: T1, tabOrder: [T1] },
          { id: PREVIEW_GROUP, worktreeId: WT, activeTabId: T2, tabOrder: [T2] }
        ]
      },
      layoutByWorktree: { [WT]: SPLIT_LAYOUT },
      unifiedTabsByWorktree: {
        [WT]: [terminalUnifiedTab(T1, HOST_GROUP, 0), terminalUnifiedTab(T2, PREVIEW_GROUP, 1)]
      }
    })
    const patch = applyWebSessionTabsSnapshot(
      state,
      makeSnapshot(
        [
          terminalSurface('host-tab-1', LEAF_ID, 'terminal-1', true),
          terminalSurface('host-tab-2', SECOND_LEAF_ID, 'terminal-2')
        ],
        {
          activeGroupId: HOST_GROUP,
          activeTabId: `host-tab-1::${LEAF_ID}`,
          tabGroups: [
            {
              id: HOST_GROUP,
              activeTabId: 'host-tab-1',
              tabOrder: ['host-tab-1', 'host-tab-2'],
              recentTabIds: []
            }
          ],
          tabGroupLayout: { type: 'leaf', groupId: HOST_GROUP }
        }
      ),
      ENV,
      NOW
    ) as Partial<WebSessionTabsSyncState>

    // Why: an omitted patch key means unchanged — assert the effective state, not the diff.
    const effectiveGroups = patch.groupsByWorktree?.[WT] ?? state.groupsByWorktree[WT]
    expect(effectiveGroups?.map((group) => group.id)).toEqual([HOST_GROUP, PREVIEW_GROUP])
    expect((patch.activeGroupIdByWorktree ?? state.activeGroupIdByWorktree)[WT]).toBe(PREVIEW_GROUP)
  })

  // A remote-owned tab close round-trips through the host, so the snapshot — not
  // closeUnifiedTab — must collapse the pane the vanished tab leaves behind.
  it('collapses a split pane whose only tab vanished from the snapshot', () => {
    const state = splitClientState()
    const patch = applyWebSessionTabsSnapshot(
      state,
      makeSnapshot(
        [
          terminalSurface('host-tab-1', LEAF_ID, 'terminal-1', true),
          terminalSurface('host-tab-2', SECOND_LEAF_ID, 'terminal-2')
        ],
        {
          activeGroupId: HOST_GROUP,
          activeTabId: `host-tab-1::${LEAF_ID}`,
          tabGroups: [
            {
              id: HOST_GROUP,
              activeTabId: 'host-tab-1',
              tabOrder: ['host-tab-1', 'host-tab-2'],
              recentTabIds: []
            }
          ],
          tabGroupLayout: { type: 'leaf', groupId: HOST_GROUP }
        }
      ),
      ENV,
      NOW
    ) as Partial<WebSessionTabsSyncState>

    const effectiveGroups = patch.groupsByWorktree?.[WT] ?? state.groupsByWorktree[WT]
    expect(effectiveGroups?.map((group) => group.id)).toEqual([HOST_GROUP])
    const effectiveLayout = (patch.layoutByWorktree ?? state.layoutByWorktree)[WT]
    expect(effectiveLayout && layoutLeafGroupIds(effectiveLayout)).toEqual([HOST_GROUP])
    expect((patch.activeGroupIdByWorktree ?? state.activeGroupIdByWorktree)[WT]).toBe(HOST_GROUP)
  })

  // Unreachable through applyWebSessionTabsSnapshot (a no-tab snapshot bails before
  // placement), so pin the defensive branch at the unit level.
  it('keeps the last pane when every group empties at once', () => {
    const result = reconcileClientOwnedTabPlacement({
      currentGroups: [
        { id: HOST_GROUP, worktreeId: WT, activeTabId: T1, tabOrder: [T1] },
        {
          id: PREVIEW_GROUP,
          worktreeId: WT,
          activeTabId: BROWSER_HOST_TAB,
          tabOrder: [BROWSER_HOST_TAB]
        }
      ],
      worktreeId: WT,
      validUnifiedTabIds: new Set(),
      adoptedTabs: [],
      placementMoves: [],
      rekeyedTabIds: new Map(),
      intentTabId: null,
      reservedEmptyGroupFallbackTabId: null,
      currentActiveGroupId: PREVIEW_GROUP,
      currentLayout: SPLIT_LAYOUT,
      isGroupReserved: () => false
    })
    expect(result.groups?.map((group) => group.id)).toEqual([PREVIEW_GROUP])
    expect(result.layout && layoutLeafGroupIds(result.layout)).toEqual([PREVIEW_GROUP])
  })

  // The host drops client-minted group ids, so the client's own record must land the terminal.
  it('places a terminal created for a client pane in that pane via its placement record', () => {
    recordWebSessionTerminalPlacement({
      environmentId: ENV,
      worktreeId: WT,
      hostTabId: 'host-tab-2',
      groupId: PREVIEW_GROUP
    })

    const patch = applyWebSessionTabsSnapshot(
      makeState({
        activeGroupIdByWorktree: { [WT]: PREVIEW_GROUP },
        groupsByWorktree: {
          [WT]: [
            { id: HOST_GROUP, worktreeId: WT, activeTabId: T1, tabOrder: [T1] },
            { id: PREVIEW_GROUP, worktreeId: WT, activeTabId: null, tabOrder: [] }
          ]
        },
        layoutByWorktree: { [WT]: SPLIT_LAYOUT },
        unifiedTabsByWorktree: { [WT]: [terminalUnifiedTab(T1, HOST_GROUP, 0)] }
      }),
      makeSnapshot(
        [
          terminalSurface('host-tab-1', LEAF_ID, 'terminal-1'),
          terminalSurface('host-tab-2', SECOND_LEAF_ID, 'terminal-2', true)
        ],
        {
          activeGroupId: HOST_GROUP,
          activeTabId: `host-tab-2::${SECOND_LEAF_ID}`,
          activeTabType: 'terminal',
          tabGroups: [
            {
              id: HOST_GROUP,
              activeTabId: 'host-tab-2',
              tabOrder: ['host-tab-1', 'host-tab-2'],
              recentTabIds: []
            }
          ]
        }
      ),
      ENV,
      NOW
    ) as Partial<WebSessionTabsSyncState>

    expect(groupById(patch, PREVIEW_GROUP)?.tabOrder).toEqual([T2])
    expect(groupById(patch, HOST_GROUP)?.tabOrder).toEqual([T1])
    expect(patch.unifiedTabsByWorktree?.[WT]?.find((tab) => tab.id === T2)?.groupId).toBe(
      PREVIEW_GROUP
    )
  })
})
