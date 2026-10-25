/**
 * `ade_git::branch_ops` 的 TS 镜像。改 Rust 侧必须同步这里 ——
 * `specta_export::tests::bindings_are_fresh` 只锁生成文件
 * (`src/bridge/real/generated/tauri-bindings.ts`)，**不锁这份手抄镜像**
 * （同 `git-ref-types.ts` 的约定；仓库有过手抄镜像漂移的坑）。
 *
 * 与生成件的唯一字面差异是 `?: T` 而非 `?: T | null`：Rust 侧
 * `skip_serializing_if = "Option::is_none"` 会整键省略，运行时只会是
 * `undefined`，永不是 `null`。这里按运行时真相写，让 `outcome.branch == null`
 * 这种「把缺省和 null 混为一谈」的写法在类型上无所适从。
 *
 * `status` 的可判别联合形状有 Rust 侧测试逐字锁住
 * （`ade-bridge/tests/git_checkout.rs::refusal_serializes_as_discriminated_union`），
 * 所以这份镜像漂移时那条会先红。
 */
export type GitCheckoutTargetKind = 'local' | 'remote'

export type GitCheckoutTarget = {
  kind: GitCheckoutTargetKind
  /** 本地为短名 `feature`；远程为 `origin/feature`。 */
  name: string
}

export type GitCheckoutRefusal = 'checked-out-elsewhere'

export type GitCheckoutStatus =
  | 'switched'
  | 'already-on-branch'
  | 'invalid-target'
  | { refused: GitCheckoutRefusal }

export type GitCheckoutOutcome = {
  status: GitCheckoutStatus
  /** 成功 / 幂等时是分支短名；`checked-out-elsewhere` 时是被占用的那个短名。 */
  branch?: string
  /**
   * `checked-out-elsewhere` 时是占用方 worktree 路径 —— 前端据此给
   * 「跳到那个 workspace」的动作。与 `branch` 分两个字段是为了不让人
   * 拿同一个 `string | undefined` 表达两种语义。
   */
  worktreePath?: string
  message?: string
}

// 刻意不提供 `isCheckoutSettled(outcome)` 这类「帮前端分支」的辅助函数：
// 2A 里没有生产消费者也没有测试，属投机 API。2B 接动作表时按它真正需要的
// 判定再加（`typeof status === 'object' && 'refused' in status` 就够用）。
