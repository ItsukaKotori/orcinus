import {
  buildDefaultGitHistoryColorMap,
  buildGitHistoryViewModels,
  getGitHistoryItemLaneIndex,
  type GitHistoryItemViewModel
} from '@shared/git-history-graph'
import type { GitHistoryGraphColorId } from '@shared/git-history-types'
import type { GitLogSnapshot } from '../domain/types'

export type GraphRow = {
  viewModel: GitHistoryItemViewModel
  laneIndex: number
}

// 复用 shared 的泳道布局算法（electron-free、只读、不修改），把 GitHistoryResult
// 转成逐行视图模型 + 该行主泳道索引。
export function buildGraphRows(snapshot: GitLogSnapshot): GraphRow[] {
  const colorMap = buildDefaultGitHistoryColorMap({
    currentRef: snapshot.currentRef,
    remoteRef: snapshot.remoteRef,
    baseRef: snapshot.baseRef
  })
  const viewModels = buildGitHistoryViewModels(
    snapshot.items,
    colorMap,
    snapshot.currentRef,
    snapshot.remoteRef,
    snapshot.baseRef,
    snapshot.hasIncomingChanges,
    snapshot.hasOutgoingChanges,
    snapshot.mergeBase
  )
  return viewModels.map((viewModel) => ({ viewModel, laneIndex: getGitHistoryItemLaneIndex(viewModel) }))
}

// shared 用色 id（'git-graph-lane-1' 等）；映射到插件自带 CSS 变量（Task 2 定义）。
export function graphColorVar(colorId: GitHistoryGraphColorId): string {
  return `var(--git-graph-${colorId.replace(/^git-graph-/, '')})`
}

// 行渲染宽度（以泳道格计）：需覆盖输入/输出泳道与自身落位，否则顶端多父合并的分叉线会被裁掉。
export function graphLaneCellCount(row: GraphRow): number {
  return Math.max(row.viewModel.inputSwimlanes.length, row.viewModel.outputSwimlanes.length, row.laneIndex + 1)
}
