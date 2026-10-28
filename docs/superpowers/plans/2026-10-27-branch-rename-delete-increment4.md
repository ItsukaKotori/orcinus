# 破坏性分支动作：Rename + Delete（增量 4）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给分支树补两个**破坏性**本地动作：`Rename…`（新命令 `git_branch_rename`，`git branch -m`，仅未检出分支，F2 触发）与 `Delete`（新命令 `git_branch_delete`，`-d` 未合并→`-D` 二次确认，仅未检出分支）；并把增量3 deferred 的 **§8.2 in-flight UI**（写操作进行中行尾 spinner + 全部写动作置灰）一并落地。

**Architecture:** 照增量3（2A `branch_ops`）的形状加 `ade_git::branch_ops::rename()` / `delete()`，复用共享 `BranchOperationRefusal`（本增量加 `NotMergeable { unmergedCount }` 变体）+ `resolve_create_branch_name`（新名校验）+ `worktree_list`（占用复核）。业务拒绝以 `Ok(Outcome.status)` 透出、`Err` 只留环境级失败。两条新 tauri 命令各走 §3.5 七面镜像。前端在 2B 的 `branch-actions`/`branch-menu` 接缝 + 2C 常驻区块上接 `rename`/`delete`，新增 Rename 名字框与 Delete 强制二次确认框；`BranchRefTree` 加 F2 键接缝。

**Tech Stack:** Rust（ade-git/ade-bridge + bindings 再生 + 真 git 集成测试）、TS/React/Radix、Vitest+happy-dom、cargo。无网络命令（本地 git 写）。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` §7.3（F2）/§7.4（Rename/Delete 矩阵）/§8.1（结构化拒绝）/§8.2（in-flight）/§8.3（刷新分级）/§9/§3.5/§3.6/§3.8/§11.1；路线图 `docs/superpowers/plans/2026-10-26-branch-actions-roadmap-increments3-5.md`。模板：`2026-10-26-branch-create-upstream-increment3.md`（命令+7 面+接线全套）。

## Global Constraints

- **破坏性 → 全量 `pnpm test` 门禁**（§10 增量3；O-2 live-shell 环境性挂起除外并标注）+ `cargo test --workspace` + 增量末全分支审查。
- **仅未检出分支可 Rename/Delete**（决策 5 / §7.4）：前端 advisory 置灰（`row.checkedOutBy !== null`）+ 后端 `worktree_list` 结构化复核回 `CheckedOut { worktreePath }`。二者都做。
- **结构化拒绝**：`git_branch_rename`/`git_branch_delete` 业务拒绝走 `Ok(Outcome.status)`（`NameTaken` / `CheckedOut{worktreePath}` / `NotMergeable{unmergedCount}`），**不经 `BridgeError`**。共享 `BranchOperationRefusal` 加 `NotMergeable`。
- **不依赖文本匹配 stderr 分类**（避 §3.8）：占用与否用 `worktree_list`；"未合并"用 `git branch -d` 失败 + 结构化兜底（先 `worktree_list` 排已检出，剩余失败即未合并），**新路径不碰** `git-branch-delete-refusal.ts` 的文本匹配器。
- **不重构 `worktree_remove::delete_branch`（Ruling，见下）**：它是"移除工作区后永不让删除失败"的 Preserved/Skipped 语义，与分支菜单 Delete 的"结构化拒绝 + 强制二次确认"契约不同；强行复用会动其 22 条既有测试，属低收益高风险。两流仅共享**非文本**的 `is_branch_checked_out`（`worktree_remove.rs` 改 `pub(crate)`）。`worktree_remove` 既有测试**零改动**。
- **`rename` 不动 ade 元数据/config**（决策 5）：实测 `git branch -m` 只迁它认识的键、不动自定义 `branch.<x>.base`；未检出分支无 workspace 引用 → ade 侧无需同步元数据。ade 只跑 `git branch -m -- <old> <new>`。
- **create/rename 的新名都过 `resolve_create_branch_name`**；rename 前 `old` 必须存在于 `refs/heads/`；`git branch -m`/`-d`/`-D` 的 name 作独立 argv（`--` 分隔），绝不当任意 revision。
- **§8.2 in-flight**：三个既有写动作（checkout/new-branch/set-upstream）+ 本增量 rename/delete，**共用一个 in-flight 态**；进行中：全部写动作菜单项 `enabled=false`（置灰）+ 发起前复查拒绝（防 F2/快捷键绕过）。`writeInFlightRef` 升级为 `useState` 驱动的 `writeInFlight`（ref 仍做权威互斥，state 驱动 UI）。
- **刷新分级（§8.3）**：Rename/Delete 目标恒为非当前分支（当前分支 isHead → 两动作不可见）→ 成功仅 `invalidate()`，不刷当前状态。
- **本地 only**（§3.6）：非 local runtime 目标由 runtime 客户端显式抛。i18n 只补 en+zh。无 pre-commit 钩子，门禁手动。

## 已核实事实（本计划撰写前实测）

| 项 | 结论 |
|---|---|
| 既有删除逻辑 | `worktree_remove.rs::delete_branch(repo, branch_ref, force) -> BranchDeleteOutcome{Deleted,Preserved{branch_name,head},Skipped}`（:134）——移除工作区后的清理，**永不 Err**。`force_delete_branch(repo, name, expected_head)` CAS 强删（:196）。`is_branch_checked_out(repo,name)`（:242，worktree_list，非文本）+ `is_checked_out_refusal(stderr)`（:256，文本匹配，本增量不用）。 |
| 未合并计数 | 实测 `git rev-list --count HEAD..<branch>` 给出未合入 HEAD 的提交数（用例=1）；`git branch -d` 对未合并分支退 1（"not fully merged"）。计数字符串进确认框。 |
| rename config | 实测 `git branch -m -- feat feat2` **不**迁自定义 `branch.feat.base`（只迁 git 已知键）→ ade 无需动 config/元数据（决策5）。 |
| 命令现状 | ade-bridge 无 `git_branch_delete`/`git_branch_rename`（仅 ade-git 内部有 delete 逻辑）→ 两条**新 tauri 命令**，各走七面。 |
| F2 接缝 | `branch-ref-tree.tsx` `handleKeyDown`（:110-155）处理 ↑↓←→ + 行是 `<button data-row-index>`；无 F2。加 `onRenameRequest?(row)` prop + F2 分支（对焦点行）。 |
| 共享 enum | `BranchOperationRefusal` 现为 `NameTaken \| CheckedOut{worktreePath} \| UpstreamMissing`（`git-branch-ops-types.ts` + Rust）。加 `NotMergeable{unmergedCount}`（serde kebab → `not-mergeable`）。 |
| 复用 helper | `resolve_create_branch_name`（`branch.rs:130`）、`command::run`/`run_text`、`ref_exists`（`branch_ops.rs`）、`worktree_list`。`is_branch_checked_out` 由 `pub(crate)` 起（原私有）。 |

## 文件结构

**新建**：`ade-git/tests/branch_rename.rs`、`ade-git/tests/branch_delete.rs`、`ade-bridge/tests/git_branch_rename.rs`、`ade-bridge/tests/git_branch_delete.rs`、`src/renderer/src/runtime` 复用 `runtime-git-branch-client.ts`（加两函数）、`git-branch/rename-dialog.tsx`、`git-branch/delete-confirm-dialog.tsx`（+测试）。
**修改**：`ade-git/src/branch_ops.rs`（加 `rename`/`delete` + `NotMergeable`）、`ade-git/src/worktree_remove.rs`（`is_branch_checked_out` 改 `pub(crate)`）、`ade-git/src/lib.rs`（再导出）、`ade-bridge`（两命令 + collect_commands + 花名册）、`tauri-bindings.ts`（再生）、`real/git.ts`(+`git.test.ts`)、`preload-api`、`git-branch-ops-types.ts`、`runtime-git-branch-client.ts`+barrel+契约列表、`branch-actions.ts`（+`rename`/`delete` 动作 + ctx + in-flight enabled）、`branch-ref-tree.tsx`（F2）、`source-control-branch-tree-section.tsx`（接线两动作 + in-flight state/spinner + 两对话框）、i18n en/zh。

---

## Task 1: `ade_git::branch_ops::delete`（后端 + 集成测试）

**Interfaces:** Produces（Rust）`GitBranchDeleteInput { branch: String, force: bool }`、`GitBranchDeleteStatus { Deleted, Refused(BranchOperationRefusal) }`、`GitBranchDeleteOutcome { status, branch?, unmerged_count? }`；`delete(worktree_path, &input) -> Result<GitBranchDeleteOutcome, CoreError>`。共享 enum 加 `NotMergeable { #[serde(rename="unmergedCount")] unmerged_count: u32 }`。

- [ ] **Step 1: 失败集成测试 `ade-git/tests/branch_delete.rs`**（复用 branch_create 的 git/commit/repo helper 形态）：
  - `deletes_merged_branch`：main 上、`feat` 已并入 HEAD（`git merge feat` 或 feat==main）→ delete force=false → `Deleted`，ref 消失，HEAD 不变。
  - `refuses_unmerged_with_count`：`feat` 领先 main 2 提交、未 force → `Refused(NotMergeable{unmerged_count:2})`，**ref 仍在**、无副作用。
  - `force_deletes_unmerged`：同上 + force=true → `Deleted`。
  - `refuses_checked_out_elsewhere`：`git worktree add <tmp> feat` → delete → `Refused(CheckedOut{worktree_path ends_with tmp})`，ref 仍在。
  - `unknown_branch_is_error_or_refused`：不存在的 branch → 定 `Err(InvalidInput)`（Step 3 锁）。
  - `does_not_touch_head`：全程 current_branch 不变。
- [ ] **Step 2: `cargo test -p ade-git --test branch_delete` 失败。**
- [ ] **Step 3: 实现（`branch_ops.rs`）**
```rust
// enum 加变体：
#[serde(rename_all = "camelCase")]
NotMergeable { #[serde(rename = "unmergedCount")] unmerged_count: u32 },

#[derive(Serialize /* +specta */)] #[serde(rename_all="kebab-case")]
pub enum GitBranchDeleteStatus { Deleted, Refused(BranchOperationRefusal) }

pub fn delete(worktree_path: &str, args: &GitBranchDeleteInput) -> Result<GitBranchDeleteOutcome, CoreError> {
    let branch = args.branch.trim();
    if branch.is_empty() { return Err(CoreError::InvalidInput("branch name must not be empty".into())); }
    if !ref_exists(worktree_path, &format!("refs/heads/{branch}")) {
        return Err(CoreError::InvalidInput(format!("unknown local branch: {branch}")));
    }
    // 占用：结构化 worktree_list 复核（复用 worktree_remove::is_branch_checked_out，改 pub(crate)），
    // 命中 → CheckedOut{path}。当前 worktree 检出该分支时 isHead 已令 Delete 不可见，仍兜一层。
    if let Some(path) = checked_out_worktree_path(worktree_path, branch)? {
        return Ok(refused(GitBranchDeleteOutcome::..., BranchOperationRefusal::CheckedOut { worktree_path: path }, branch));
    }
    let flag = if args.force { "-D" } else { "-d" };
    let output = run_git_in(worktree_path, &["branch", flag, "--", branch], REGISTRATION?, None)?;
    if output.status.success() { return Ok(Deleted); }
    if args.force { return Err(git_command_failed(&["branch", flag, branch], &output)); } // -D 失败（罕见）= 环境级
    // -d 失败且未占用 → 未合并：计数 = `git rev-list --count HEAD..<branch>`（实测语义），回 NotMergeable。
    let count = unmerged_count(worktree_path, branch)?;
    Ok(Refused(NotMergeable { unmerged_count: count }))
}
```
`checked_out_worktree_path`（返回占用方路径或 None，走 `worktree_list` + `are_worktree_paths_equal`，排除"当前 worktree 自己"无意义——删除非检出分支时当前分支即 isHead 已被前端挡；后端**任何** worktree（含当前）检出该分支都算占用，返回其 path）、`unmerged_count`（`run_text(worktree, &["rev-list","--count","HEAD..{branch}"])` 解析 u32，失败回落 0）。`is_branch_checked_out` 从 `worktree_remove.rs` 提为 `pub(crate)` 或在 branch_ops 内以 `worktree_list` 现算（择一，避免循环依赖）。
- [ ] **Step 4: `cargo test -p ade-git --test branch_delete` 绿。** `worktree_remove` 既有测试**不跑不改**（行为未动）。
- [ ] **Step 5: 提交**。

## Task 2: `ade_git::branch_ops::rename`（后端 + 集成测试）

**Interfaces:** `GitBranchRenameInput { old_name, new_name }`、`GitBranchRenameStatus { Renamed, Refused(BranchOperationRefusal) }`、`GitBranchRenameOutcome { status, old_name?, new_name? }`；`rename(worktree_path,&input)->Result<_,CoreError>`。

- [ ] **Step 1: 失败测试 `ade-git/tests/branch_rename.rs`**：
  - `renames_local_branch`：`branch -m` old→new，`refs/heads/new` 存在、`refs/heads/old` 消失、**HEAD 不变**。
  - `refuses_name_taken`：new 已存在 → `Refused(NameTaken)`，old/new 都在、无副作用。
  - `refuses_checked_out_old`：old 被别 worktree 检出 → `Refused(CheckedOut{path})`。
  - `rejects_invalid_new_name`：new = `"bad name"`/`-x`/空 → `Err`（断错误来源含 `check-ref-format`/`must not start with`/`empty`，同增量3 判别力手法）。
  - `unknown_old_branch` → `Err(InvalidInput)`（含 `unknown local branch`）。
- [ ] **Step 2 失败 → Step 3 实现**：`new` 过 `resolve_create_branch_name`；`old` 存在否则 Err；new 已存在→NameTaken；old 被任一 worktree 检出→CheckedOut；`run(worktree, &["branch","-m","--",old,new])`。
- [ ] **Step 4 `cargo test -p ade-git --test branch_rename` 绿。Step 5 提交**。

## Task 3: 两命令的七面镜像（bridge + bindings + runtime + TS 契约）

照增量3 Task1/2 Step5 的六处登记，对 `git_branch_rename` 与 `git_branch_delete` 各做一遍：
- `ade-bridge/commands/git.rs`：`GitBranchRenameArgs`/`GitBranchDeleteArgs`（`{ worktree_path, args }`）+ `*_impl` + `#[tauri::command]`（鉴权 + run_blocking，软错误 Ok 透出不经 BridgeError）。
- `specta_export.rs`：`collect_commands!` 两条 + 花名册两条。
- 再生 `export-bindings` → `tauri-bindings.ts`。
- `real/git.ts`：`branchRename`/`branchDelete` 映射 + `git.test.ts` commandCases 两条。
- `preload-api/api/git-operation-api.ts`：两方法类型。
- `git-branch-ops-types.ts`：加 `BranchOperationRefusal` 的 `NotMergeable{unmergedCount}` + `GitBranchRename{Input,Status,Outcome}` + `GitBranchDelete{Input,Status,Outcome}`（kebab/camel 与生成件对齐）。
- `runtime-git-branch-client.ts`：`renameRuntimeGitBranch`/`deleteRuntimeGitBranch`（本地 only、非 local 抛、空路径抛、拒绝 resolve 不 reject）+ barrel 再导出 + `runtime-git-client-api-contract.test.ts` 两条 + runtime 测试（照 create/upstream）。
- `ade-bridge/tests/git_branch_rename.rs`/`git_branch_delete.rs`：透传契约 + `refusal_serializes_as_discriminated_union`（含 `not-mergeable` 的 `{ "not-mergeable": { unmergedCount } }` 形状锁）。

**门禁**：`cargo test -p ade-bridge`（`bindings_are_fresh`+`command_inventory`+两契约）、`pnpm vitest run src/renderer/src/runtime src/bridge/real/git.test.ts`、`pnpm typecheck`。提交。

## Task 4: 动作表 + F2 接缝（rename / delete 模型 + in-flight enabled）

**Files:** `branch-actions.ts`、`branch-menu.tsx`、`branch-ref-tree.tsx` + 测试。
- [ ] `BranchActionId` 加 `'rename' | 'delete'`。`BranchActionContext` 加 `rename?: (ref)=>void`、`requestDelete?: (ref)=>void`、`opInFlight?: boolean`。
- [ ] `rename` 动作：`visible` = local && !isHead && `ctx.rename!==undefined`；`enabled` = `!opInFlight`（in-flight 置灰）且 `checkedOutBy===null`；`run`→`ctx.rename(ref)`。`delete` 动作同（`requestDelete`）。二者仅 local（§7.4：Remote/Tag 不出 Rename/Delete）。
- [ ] 既有 `checkout`/`new-branch`/`set-upstream` 动作 `enabled` 追加 `&& !ctx.opInFlight`（统一 §8.2）。
- [ ] `branch-ref-tree.tsx`：`BranchRefTreeProps` 加 `onRenameRequest?: (row: BranchTreeRow)=>void`；`handleKeyDown` 加 `if (event.key==='F2')` → 取焦点行（`data-row-index`）对应 `rows[i]`，`row.kind==='ref' && rename-eligible` 才 `onRenameRequest(row)`，否则吞掉（不可用则无操作，§7.3）。
- [ ] 测试：`branch-actions.test.ts` 断 rename/delete visible（local only、isHead/remote/tag 不可见、不声明回调不可见）、`enabled` 随 `opInFlight`/`checkedOutBy`；`branch-ref-tree.test.tsx` F2 用例（焦点 ref 行按 F2 → `onRenameRequest` 带该行；group/synthetic 不触发）；`branch-menu.test.tsx` opInFlight 时写项 disabled。
- [ ] 提交 + 判别力实证（删 F2 分支 → 用例红；删 opInFlight → enabled 用例红）。

## Task 5: 宿主 A 接线（Rename 框 / Delete 二次确认 / in-flight UI / i18n）

**Files:** 新建 `rename-dialog.tsx`、`delete-confirm-dialog.tsx`（+测试）；改 `source-control-branch-tree-section.tsx`（+测试）；i18n。
- [ ] **Rename**：`renameTarget` 状态；`RenameDialog`（受控新名 + 非空 gate，复用 NewBranchDialog 形状）；`executeRename(old,new)`→`renameRuntimeGitBranch` → Renamed：`invalidate()`、关框；`NameTaken`/`CheckedOut`：`toast.error`（按变体分文案，同增量3 Ruling）；环境 `Err`→toast。
- [ ] **Delete（两步）**：`deletePending: GitRefEntry|null`（首次）+ `deleteForce: {ref, unmergedCount}|null`（二次）。`executeDelete(ref, force)`→`deleteRuntimeGitBranch`：`Deleted`→`invalidate()`+关；`NotMergeable{count}`（force=false 时）→ 弹**强制确认框**（文案带分支名 + N 个未合并提交；无"输入名字"式摩擦）→ 确认→`executeDelete(ref, true)`；`CheckedOut{path}`→跳转（复用 checkout 的 owners.get(outcome.branch) 短名跳转，path 兜底提示）。
- [ ] **in-flight UI**：`writeInFlightRef` 保留权威互斥；新增 `const [opInFlight,setOpInFlight]=useState(false)`，三/五个写动作执行前后 set true/false（finally），`ctx.opInFlight = opInFlight`（置灰）；行尾 spinner 可选（§8.2 "spinner"——最小实现：给区块头/操作行一个 `Loader2`，或对不可达的 per-row spinner 记 deferred）。Ruling：per-row spinner 若成本高，本增量至少落"置灰 + 发起前复查"，spinner 留最小或 deferred。
- [ ] `HOST_A_ALLOWED` 加 `rename`、`delete`；`ctx` 注入 `rename`/`requestDelete`。
- [ ] **i18n** en+zh：`rename`、`renameDialogTitle`/`renameCreate`("Rename branch")、`delete`、`deleteConfirmTitle`/`deleteConfirmBody`("{{branch}} 有 {{count}} 个未合并提交，强制删除？")、`deleteForce`("Force delete")、`renameNameTaken`/`renameFailed`/`deleteCheckedOut`/`deleteFailed`。复用已有 `checkoutCancel`/`nameTaken` 模式。
- [ ] **测试**：section 集成（照增量3 writes 测试的 mock 菜单原语+dialog 内联+runtime spy）——rename 成功→rename 命令收 `{oldName,newName}`+invalidate+不刷状态；delete 未合并→先 force=false 拿 NotMergeable→弹二次确认→确认 force=true→Deleted→invalidate；delete 当前/被检出→不出现或跳转；rename/delete 在 opInFlight 时禁用（可选）。NewBranchDialog/RenameDialog 组件测试。
- [ ] 提交。

## 收尾门禁（全量，破坏性）

- [ ] `cargo test --workspace`（先 `pnpm build:orcinus-cli` 产 sidecar，若 workspace 需要）。
- [ ] `pnpm typecheck`；`pnpm build:web`。
- [ ] **`pnpm test` 全量**（live-shell O-2 环境性挂起除外并标注）。
- [ ] 增量末全分支 CodeReview（含 §8.2 in-flight、Delete 两步、结构化拒绝、worktree_remove 未被波及）。
- [ ] 手工验收（`pnpm dev`）：F2 改名、Delete 未合并二次确认、已检出置灰、in-flight 置灰。

## 范围外
- Show Diff 宿主接线 → 增量3b。Update/Push/fastForward（O-5、远端并发）→ 增量5。远程 runtime → 维持显式拒绝。`worktree_remove::delete_branch` 不重构（Ruling）。
