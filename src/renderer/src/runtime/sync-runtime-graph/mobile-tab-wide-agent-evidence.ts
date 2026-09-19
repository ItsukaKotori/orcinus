import type {
  TerminalLayoutSnapshot,
  TerminalPaneLayoutNode
} from '../../../../shared/terminal-tab-types'
import type { TuiAgent } from '../../../../shared/tui-agent'

function layoutNodeContainsLeaf(node: TerminalPaneLayoutNode | null, leafId: string): boolean {
  if (!node) {
    return false
  }
  if (node.type === 'leaf') {
    return node.leafId === leafId
  }
  return layoutNodeContainsLeaf(node.first, leafId) || layoutNodeContainsLeaf(node.second, leafId)
}

export function resolveActiveLayoutLeafId(
  layout: TerminalLayoutSnapshot | null | undefined
): string | null {
  if (!layout) {
    return null
  }
  if (layout.activeLeafId) {
    // Why: close/hydration races can leave activeLeafId one snapshot behind
    // the topology; stale pane evidence must not route to a removed leaf.
    return !layout.root || layoutNodeContainsLeaf(layout.root, layout.activeLeafId)
      ? layout.activeLeafId
      : null
  }
  return layout.root?.type === 'leaf' ? layout.root.leafId : null
}

export function isTabWideFallbackSafe(
  layout: TerminalLayoutSnapshot | null | undefined
): boolean {
  if (!layout?.root) {
    return true
  }
  if (layout.root.type === 'split') {
    return false
  }
  // Why: a stale active id means the single-leaf collapse is not yet settled;
  // tab-wide launch/title evidence could still describe the removed sibling.
  return !layout.activeLeafId || layout.activeLeafId === layout.root.leafId
}

/** Whether tab-wide launch evidence (agent hint, launch draft) describes this
 *  leaf: it must still be the tab's sole pane and the one the evidence bound to. */
export function tabWideEvidenceOwnsLeaf(args: {
  ownerLeafId: string | null
  leafId: string | null
  leafIds: readonly string[]
}): boolean {
  const { ownerLeafId, leafId, leafIds } = args
  if (!ownerLeafId || !leafId) {
    return false
  }
  // Why: the evidence belongs to the tab's original pane. Once a split exists,
  // it says nothing about any particular sibling.
  return leafIds.length === 1 && leafIds[0] === leafId && ownerLeafId === leafId
}

export function launchAgentForLeaf(args: {
  launchAgent?: TuiAgent | null
  launchAgentLeafId: string | null
  leafId: string | null
  leafIds: readonly string[]
}): TuiAgent | null {
  const { launchAgent, launchAgentLeafId, leafId, leafIds } = args
  if (!launchAgent) {
    return null
  }
  return tabWideEvidenceOwnsLeaf({
    ownerLeafId: launchAgentLeafId,
    leafId,
    leafIds
  })
    ? launchAgent
    : null
}
