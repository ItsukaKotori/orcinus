/**
 * `ade_git::branch_ops` 的 create / set-upstream 结果的 TS 手抄镜像。
 * 改 Rust 侧必须同步这里 —— `specta_export::tests::bindings_are_fresh` 只锁生成件
 * (`src/bridge/real/generated/tauri-bindings.ts`)，**不锁这份手抄镜像**（同
 * `git-checkout-types.ts` 的约定）。与生成件的唯一字面差异是可选字段写 `?: T`
 * 而非 `?: T | null`：Rust `skip_serializing_if = "Option::is_none"` 会整键省略，
 * 运行时只会是 `undefined`，永不是 `null`。
 *
 * `BranchOperationRefusal` 是增量 3–4 的分支写操作**共用**枚举：create 用 `NameTaken`，
 * set-upstream 用 `UpstreamMissing`，rename/delete（增量 4）用 `CheckedOut`。
 * `CheckedOut` 序列化成 `{ 'checked-out': { worktreePath } }`（externally tagged，
 * kebab-case），与 `ade-bridge/tests/git_branch_create.rs` 的可判别联合契约测试对齐。
 */
export type BranchOperationRefusal =
  | 'name-taken'
  | { 'checked-out': { worktreePath: string } }
  | 'upstream-missing'

export type GitBranchCreateInput = {
  /** 新分支短名，可含 `/`。后端过 `resolve_create_branch_name` 校验。 */
  name: string
  /** start-point：可解析成 commit 的真实 rev（通常用 `GitRefEntry.oid`）。 */
  startPoint: string
}

export type GitBranchCreateStatus = 'created' | { refused: BranchOperationRefusal }

export type GitBranchCreateOutcome = {
  status: GitBranchCreateStatus
  /** 成功为新建分支短名；`NameTaken` 为被占用的短名。 */
  branch?: string
  message?: string
}
