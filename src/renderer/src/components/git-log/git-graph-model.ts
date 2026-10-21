import {
  buildDefaultGitHistoryColorMap,
  buildGitHistoryViewModels,
  getGitHistoryItemLaneIndex,
  type GitHistoryItemViewModel
} from '../../../../shared/git-history-graph'
import type { GitLogSnapshot } from './types'

export type GraphRow = {
  viewModel: GitHistoryItemViewModel
  laneIndex: number
}

// 复用 shared 的泳道布局算法（electron-free、只读、不修改），把 GitHistoryResult
// 转成逐行视图模型 + 该行主泳道索引。SVG 几何/配色由复用的宿主 GitHistoryGraphSvg 负责。
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
