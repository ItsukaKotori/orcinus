export const BRANCH_SECTION_VIEW_STATE_STORAGE_KEY = 'orca-source-control-branch-section-v1'

function getWindowStorage(): Storage | null {
  return typeof window === 'undefined' ? null : window.localStorage
}

/**
 * Source Control 常驻分支树区块的折叠态。持久化写法照 `floating-terminal-panel-view-state.ts`：
 * try/catch 包裹、逐字段回落、存储不可用时静默。默认展开 —— 读不到 / 损坏载荷都算 `false`。
 *
 * Why 独立于 `use-panel-view-state.ts` 的 `collapsedSections`：后者是内存态、切 worktree 即重置，
 * 不满足 spec §13.6「折叠态持久化到 localStorage」。分支区块的展开/收起是用户跨会话偏好。
 */
export function readPersistedBranchSectionCollapsed(): boolean {
  try {
    const raw = getWindowStorage()?.getItem(BRANCH_SECTION_VIEW_STATE_STORAGE_KEY)
    if (!raw) {
      return false
    }
    const parsed: unknown = JSON.parse(raw)
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return false
    }
    return (parsed as Record<string, unknown>).collapsed === true
  } catch {
    return false
  }
}

export function persistBranchSectionCollapsed(collapsed: boolean): void {
  try {
    getWindowStorage()?.setItem(
      BRANCH_SECTION_VIEW_STATE_STORAGE_KEY,
      JSON.stringify({ collapsed })
    )
  } catch {
    // 存储不可用/满：丢一次恢复状态不值得崩面板。
  }
}
