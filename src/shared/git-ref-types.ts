/**
 * `ade_git::ref_entry` 的 TS 镜像。刻意与 `git-history-types.ts` 的
 * `GitHistoryItemRef`（commit decoration）分开：分支树要 ahead/behind/upstream，
 * 那些字段对 decoration 无意义。
 *
 * 字段名逐字对齐 serde 的 camelCase 输出；改 Rust 侧必须同步这里 ——
 * `specta_export::tests::bindings_are_fresh` 只锁生成文件
 * (`src/bridge/real/generated/tauri-bindings.ts`)，**不锁这份手抄镜像**。
 *
 * 与生成文件的一处刻意差异：specta 把 `skip_serializing_if` 的字段渲染成
 * `?: T | null`，但 Rust 侧 None 时整个 key 被跳过，运行时只会是 `undefined`，
 * 永不是 `null`。这里按运行时真相写 `?: T`，让 `ref.upstream == null` 这种
 * 「把缺省和 null 混为一谈」的写法在类型上就无所适从，而不是被默默放过。
 */
export type GitRefCategory = 'local' | 'remote' | 'tag'

export type GitRefEntry = {
  /** 短名：`main` / `origin/main` / `v1.2.0`。 */
  name: string
  /** 完整 refname：`refs/heads/main`。 */
  fullName: string
  category: GitRefCategory
  /** 仅 `remote` 有值。 */
  remote?: string
  /** commit oid；annotated tag 已 peel 到 commit。 */
  oid: string
  /** git 报告该 ref 就是当前 HEAD 分支。 */
  isHead: boolean
  upstream?: string
  /** 相对 upstream 的领先提交数；无 upstream 或 git 未报告时缺省。 */
  ahead?: number
  behind?: number
}
