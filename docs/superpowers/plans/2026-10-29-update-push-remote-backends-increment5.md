# Update / Push 全量与三条远端后端（增量 5）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一次性补齐分支特性的远端能力：(a) 分支树接 **Update** 与 **Push…**（复用现成 store 远端编排，无新后端）；(b) **§8.2 远端进行中门**（`isRemoteOperationActive` → 全部树写动作置灰 + 发起前复查）；(c) **补齐三条当前会 `reject` 的死后端**——`git_fast_forward`、`git_rebase_from_base`（连带复活 `abortRebase`/`abortMerge` 与 rebase 冲突态）、`git_fork_sync`（把已有 TS 编排 `syncForkDefaultBranch` 移植为 Rust 命令）。

**Architecture:** 远端编排层其实**已建好**（store `pushBranch/pullBranch/fastForwardBranch/syncBranch/rebaseFromBase/fetchBranch` 均走 `beginRemoteOperation(kind)`/`endRemoteOperation()` try/finally + toast + upstream 刷新；SourceControl 头部有完整远端下拉）。缺的只是：① 树里没接 Update/Push；② `window.api.git.fastForward/rebaseFromBase/syncFork` 三条在 `real/git.ts` 没映射 → 落 `withMethodFallback` 拒绝（`git_fetch`/`git_pull`/`git_push` 则已实现，见 `remote_ops.rs`）。新命令一律照 **`remote_ops.rs` 网络范式**（`run_git_in_with_env` + `GIT_NETWORK_ENVS`(`GIT_TERMINAL_PROMPT=0`) + `GIT_NETWORK_TIMEOUT` + 失败原样 `CoreError::GitCommandFailed`、**不做 §3.8 文本匹配**）。

**Tech Stack:** Rust（ade-git remote_ops/新模块 + ade-bridge 命令 + bindings 再生 + 真 git 集成测试）、TS/React/zustand、Vitest+happy-dom、cargo。含网络命令 → 防挂起硬约束贯穿。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` §3.7（死路径更正，本计划将其再更正：fetch/pull 已活，仅 ff/rebase/fork-sync 待补）/§7.4（Update/Push 矩阵）/§8.2（远端并发单飞）/§8.3（刷新分级）/§9；`docs/superpowers/plans/2026-10-26-branch-actions-roadmap-increments3-5.md`。**§3.7 的"fetch/pull 不存在"已被现网代码推翻**（`git_fetch`/`git_pull` 已注册），本计划据实重述 O-5。

## Global Constraints

- **网络命令防挂起**（记忆「Git 网络命令防挂起规范」/`remote_ops.rs` 头注）：所有触网的新命令必须 `GIT_TERMINAL_PROMPT=0`、超时 ≥ `GIT_NETWORK_TIMEOUT`（5min）、经 `run_git_in_with_env`；rebase 的客户端 RPC 超时用现成 `REBASE_FROM_BASE_RPC_TIMEOUT_MS`，fork-sync 用 `syncRuntimeGitForkDefaultBranch` 已有的 60s。
- **结构化/原样错误，绝不做 §3.8 文本匹配分类**：网络失败把 git stderr 原样 `GitCommandFailed` 上抛（同 `run_network`），前端用 `resolveRemoteOperationErrorMessage` 出文案。
- **§8.2 远端单飞**：远端 op 一律经既有 store 动作（已内含 `beginRemoteOperation/endRemoteOperation` try/finally + depth 嵌套），树里**不自己造**并发门。树所有写动作（checkout/switch/new-branch/rename/delete/set-upstream/show-diff/update/push）在 `isRemoteOperationActive===true` 时 `enabled=false`，且发起前复查一次（防 F2/快捷键/竞态绕过）。
- **仅"被某 workspace 检出且(Update 需)有 upstream"的 local 分支出现 Update/Push**（§7.4 line 331-336）；Remote/Tag 节点不出 Update/Push/Rename/Delete/Set Upstream。Update/Push 目标 = 该分支 owner 的 worktree（`ownerByBranch` → `findWorktreeById` 取 path/connectionId/settings；owner==当前则用当前上下文）。
- **刷新分级（§8.3）**：Update/Push/fast-forward/rebase/fork-sync 改的是当前分支态 → 成功后既有 store 动作已 fire-and-forget `fetchUpstreamStatus`；树侧对 owner==当前 worktree 的额外 `refreshActiveGitStatusAfterMutation`+`invalidate`，非当前仅 `invalidate`。
- **rebase 冲突态**：`git rebase` 可把仓库留在 rebase 态；本增量必须同时复活 `abortRebase`/`abortMerge`（现也是死映射），让既有冲突横幅（`status_read` 已识别 Rebase/Merge 态）能中止。**不把用户锁死在半完成态**。
- **不引入通用"跑任意 git"写执行器**（`git_read` 白名单是安全边界，不可为省事放宽）：fork-sync 的写/网络步骤进**专用 Rust 命令**内部，不外溢成任意 exec 面。
- 本地 only（§3.6）：远端 runtime 目标维持既有 store 动作的 runtime RPC 路径；本增量只补 real 本地后端映射。i18n 只补 en+zh。无 pre-commit 钩子，门禁手动。
- **破坏性 + 网络 → 收尾门禁最重**：`cargo test --workspace` + **全量 `pnpm test`**（O-2 live-shell 环境性挂起 `--exclude` 并标注）+ `build:web` + 全分支 CodeReview。

## 已核实事实（本计划撰写前实测）

| 项 | 结论 |
|---|---|
| 后端注册表现 | `git_fetch`/`git_pull`/`git_push`/`git_checkout`/`git_upstream_status` **在**；**无** `git_fast_forward`/`git_rebase*`/`git_abort_rebase`/`git_abort_merge`/`git_fork_sync`（后三者→`real/git.ts` 不映射→`withMethodFallback` 拒）。 |
| 网络范式模板 | `ade-git/src/remote_ops.rs`：`fetch`/`pull` 经 `run_network`（`run_git_in_with_env` + `GIT_NETWORK_ENVS` + `GIT_NETWORK_TIMEOUT` + `GitCommandFailed` 原样抛）。`pull_args` 刻意不加 `--ff-only`（§12.2 决策）。 |
| fast-forward arg 形状 | runtime：`window.api.git.fastForward({worktreePath,connectionId?,worktreeId?,pushTarget?})`→void；store `fastForwardBranch` 已 `beginRemoteOperation('fast_forward')`+ 后刷 upstream。 |
| rebase arg 形状 | `window.api.git.rebaseFromBase({worktreePath,baseRef,connectionId?})`→void；`git-rebase-source.ts` 已含 TS 侧 baseRef 归一/校验（`normalizeBaseRef`：拒空前导 `-`、剥 `refs/remotes/`）。冲突态 `status_read.rs` 已识别（rebase-merge/rebase-apply→`GitConflictOperation::Rebase`）。`abortRebase`/`abortMerge` runtime 已调但无后端。 |
| fork-sync arg 形状 | `window.api.git.syncFork({worktreePath,connectionId?,expectedUpstream:{owner,repo}})`→`GitForkSyncResult`（`git-fork-sync.ts`，含 status/blocked-reason/ahead/behind）。`syncForkDefaultBranch(runGit,…)` TS 编排**已存在**（跑 `remote get-url`/`ls-remote --symref`/`rev-parse`/`rev-list --left-right --count`/`check-ref-format`/`merge-base --is-ancestor`/`push`），但其中 `remote get-url`/`ls-remote`/`push` **不在 `git_read` 白名单** → 不能复用只读执行器 → 必须新实现。 |
| store 远端动作（树可直接复用） | `pushBranch(worktreeId,worktreePath,publish?,connectionId?,pushTarget?,options?)`、`pullBranch`、`fetchBranch`、`syncBranch`、`fastForwardBranch`、`rebaseFromBase`、`isRemoteOperationActive`/`beginRemoteOperation`/`endRemoteOperation`（`git-remote-push-pull.ts`/`git-remote-sync.ts`/`git-remote-status.ts`）。Update=fetch+pull 复用（无独立 update 动作，树里组合同步调 `pullBranch`，其内含 fetch? 见 Task2）。 |
| 头部下拉（须随之转活） | `source-control-dropdown-remote-items.ts` 的 Fast-forward/Rebase-from-base/Sync 项 → `use-remote-action-runner.ts` → 上述 store 动作；后端补齐后自然不再 reject。 |

## 文件结构

**新建**：`ade-git/tests/{branch_fast_forward,rebase_from_base,fork_sync}.rs`、`ade-bridge/tests/{git_fast_forward,git_rebase_from_base,git_fork_sync}.rs`、`src/renderer/src/components/git-branch/`（复用区块，无新组件文件）。
**修改**：`ade-git/src/remote_ops.rs`（加 `fast_forward`；新建 `rebase.rs`/`fork_sync.rs` 模块或在 remote_ops 内加）、`ade-git/src/lib.rs`（导出）、`ade-bridge/commands/git.rs`+`specta_export.rs`（命令×3+abort×2）、`tauri-bindings.ts`（再生）、`real/git.ts`（映射 fastForward/rebaseFromBase/syncFork/abortRebase/abortMerge）+`git.test.ts`、`preload-api`（补 abort 面类型，若缺）、`shared/git-branch-ops-types.ts` 或 fork-sync 的 Rust 镜像、`git-branch/{branch-actions.ts,source-control-branch-tree-section.tsx}`（Update/Push 动作 + §8.2 门）、i18n en/zh。

---

## Task 1: `git_fast_forward` 后端（最轻，先行）

**Interfaces:** ade-git `fast_forward(worktree_path, cancel)->Result<(),CoreError>`；`fast_forward_args()->vec!["pull","--ff-only"]`（一次网络+严格快进；非快进由 git 原样失败→`GitCommandFailed`，前端 `resolveRemoteOperationErrorMessage({isFastForward})`）。命令 `git_fast_forward{ worktree_path, args:{ ... } }`。
- [ ] `ade-git/tests/branch_fast_forward.rs`（真 git：本地 bare repo 作 remote，`git clone`→落后→ff 成功；分叉→拒且工作区不变）→ RED。
- [ ] `remote_ops.rs` 加 `fast_forward_args` + `pub fn fast_forward(...)= run_network(worktree, &fast_forward_args(), cancel)`。
- [ ] ade-bridge `git_fast_forward` 命令（`*_impl`+`#[tauri::command]`，鉴权+run_blocking，`GIT_TERMINAL_PROMPT=0` 经 run_network）+ `collect_commands!`+花名册。
- [ ] 再生 bindings；`real/git.ts` 把 `fastForward` 从 fallback 移入映射→`invokeCommand('git_fast_forward',{args:{worktreePath}})`；`git.test.ts` commandCases 加 `['fastForward','git_fast_forward']`；preload 已有类型则不补。
- [ ] `ade-bridge/tests/git_fast_forward.rs` 透传契约。门禁：`cargo -p ade-git --test branch_fast_forward`+`-p ade-bridge`+`vitest src/bridge/real/git.test.ts src/renderer/src/runtime`+typecheck。提交。

## Task 2: 分支树 Update 动作

**Files:** `branch-actions.ts`、`source-control-branch-tree-section.tsx` + 测试。
- [ ] `BranchActionId` 加 `'update'`；ctx 加 `update?: (ref)=>void`。`update` visible：local && `update!==undefined` && `row.checkedOutBy!==null`（须被某 workspace 检出，§7.4）&& 有 upstream（owner worktree 的 upstream 已知；无则不可见）。`enabled`: `!ctx.opInFlight && !ctx.remoteOpActive`。run→`ctx.update(ref)`。
- [ ] section `executeUpdate(ref)`：解析 owner worktree（`owners.get(ref.name)`→`findWorktreeById`→path/connectionId/settings），调既有 store `pullBranch(ownerWorktreeId, path, connectionId, pushTarget)`（Update=按 git 默认的 fetch+pull；若 `pullBranch` 不含 fetch 则先 `fetchBranch` 再 `pullBranch`，**在 Task2 起始核实 `pullBranch` 是否 fetch**——见待办核实项）。复用 store 即自带 `beginRemoteOperation`。成功：owner==当前则 `refreshActiveGitStatusAfterMutation`+`invalidate`，否则 `invalidate`。
- [ ] HOST_A_ALLOWED 加 `'update'`；i18n `update`。测试：可见性矩阵（有 owner+upstream 才现、remote/tag 无、未检出不现）；executeUpdate 用 owner 上下文调 pullBranch（store 动作 spy）。
- [ ] **待办核实（Task2 起始）**：`pullBranch` 内部是否已 fetch（决定 Update 是 `pullBranch` 单步还是 `fetchBranch`+`pullBranch`）。

## Task 3: 分支树 Push… 动作

- [ ] `BranchActionId` 加 `'push'`；ctx `push?: (ref)=>void`。visible：local && checked-out（有 owner，§7.4）&& `push!==undefined`。`enabled`: `!opInFlight && !remoteOpActive`。
- [ ] section `executePush(ref)`→解析 owner worktree 同 Task2→调 store `pushBranch(ownerWorktreeId, path, publish=!upstream?, connectionId, pushTarget)`（无 upstream→publish 建远程跟踪；有则普通 push）。复用 store 自带单飞+错误 toast+刷新。
- [ ] HOST_A_ALLOWED 加 `'push'`；i18n `push`。测试：publish 与已跟踪两种入参正确；非检出不现；remoteOpActive 置灰。
- [ ] 提交（Task2+3 一并亦可）。

## Task 4: §8.2 远端进行中门（树写动作统一置灰）

- [ ] section 订阅 `remoteOpActive = useAppStore(s=>s.isRemoteOperationActive)`，注入 `ctx.remoteOpActive`。
- [ ] `branch-actions.ts`：**全部写动作**（checkout/switch/new-branch/rename/delete/set-upstream/update/push）的 `enabled` 追加 `&& !ctx.remoteOpActive`（与既有 `!ctx.opInFlight` 并列；`opInFlight` 是本地写门，`remoteOpActive` 是远端门，正交）。
- [ ] F2/Enter 发起路径复查 `isRemoteOperationActive`（发起前守卫，同 2B/4 的"发起前复查"）。
- [ ] 测试：`remoteOpActive=true` 时逐项 enabled=false；反证 idle 时 true（判别力）。提交。

## Task 5: `git_rebase_from_base` 后端 + 复活 abort（rebase 冲突态闭环）

**Interfaces:** ade-git `rebase_from_base(worktree_path, base_ref, cancel)`：base_ref 过安全校验（照 `normalizeBaseRef`：拒空/前导 `-`、剥 `refs/remotes/`；remote 名复用 `is_safe_remote_name`）→ `run_network(worktree, fetch_args(remote-of-base))`（若 base 是 remote-tracking，先 fetch 该 remote）→ `run_local(worktree, &["rebase","--",base_ref])`（rebase 本地无网、但仍可能冲突失败留态：把 stderr 原样 `GitCommandFailed`）。`git_abort_rebase`=`git rebase --abort`、`git_abort_merge`=`git merge --abort`（本地，短超时）。
- [ ] `ade-git/tests/rebase_from_base.rs`：干净 rebase 成功（线性化）；冲突场景 → 命令失败且仓库进 rebase 态（`rebase-merge`/`rebase-apply` 存在）→ `abort_rebase` 回滚到干净。`branch_fast_forward` 式 repo 构造。→ RED。
- [ ] ade-git 新 `rebase.rs`（或 remote_ops 内）：`rebase_from_base` + `abort_rebase` + `abort_merge`。
- [ ] ade-bridge 三命令 `git_rebase_from_base`/`git_abort_rebase`/`git_abort_merge` + collect/花名册；再生 bindings；`real/git.ts` 映射 `rebaseFromBase`/`abortRebase`/`abortMerge`；`git.test.ts` commandCases 三条；preload 补 `abortRebase`/`abortMerge` 类型（若缺，runtime 已调）。
- [ ] 前端闭环：确认既有冲突横幅/`git_conflict_operation` 消费 `GitConflictOperation::Rebase` 时能触达 `abortRebase`（若原本因映射缺失是死的，现接通）。`rebaseRuntimeGitFromBase`/`abortRuntimeGitRebase` 本地不再 reject。
- [ ] `ade-bridge/tests/git_rebase_from_base.rs` 契约。门禁：cargo ade-git/ade-bridge + vitest runtime+git.test + typecheck。提交。

## Task 6: `git_fork_sync` 后端（移植 `syncForkDefaultBranch` 到 Rust）

**Interfaces:** ade-git 新 `fork_sync.rs`：`fork_sync(worktree_path, expected_upstream:{owner,repo}, cancel)->Result<GitForkSyncResult,CoreError>`——**以 `src/shared/git-fork-sync.ts` 的 `syncForkDefaultBranch` 为逐步骤参考实现**（remote 存在性→`remote get-url` 解析 GitHub owner/repo 比对 expectedUpstream→`ls-remote --symref upstream HEAD` 取默认分支→`rev-parse` oid→`rev-list --left-right --count originOid...upstreamOid`→非祖先/分叉判 blocked→`push origin upstreamOid:refs/heads/<branch>`）。全部经 `run_git_in_with_env` + `GIT_NETWORK_ENVS`（`ls-remote`/`fetch`/`push` 触网）。`GitForkSyncResult`/blocked-reason enum 在 Rust 定义并 serde 镜像 TS `git-fork-sync.ts`（status `up-to-date|synced|blocked`、originRemote/upstreamRemote/branchName/ahead/behind）。
- [ ] `ade-git/tests/fork_sync.rs`：搭 fork 形 repo（origin=本地 bare、upstream=另一本地 bare）→ behind→`synced`；已同步→`up-to-date`；缺 upstream remote→`blocked(missing-upstream)`；分叉→`blocked(diverged)`；`expectedUpstream` 不匹配→`blocked(upstream-mismatch)`。→ RED。
- [ ] 实现 `fork_sync.rs`（移植 TS 步骤，逐条对应）+ lib.rs 导出。
- [ ] ade-bridge `git_fork_sync` 命令 + collect/花名册；再生 bindings；`real/git.ts` 映射 `syncFork`→`invokeCommand('git_fork_sync',{args:{worktreePath,expectedUpstream}})`；`git.test.ts` `['syncFork','git_fork_sync']`；`git-branch-ops-types` 或既有 fork 类型面 Rust↔TS 对齐。
- [ ] `ade-bridge/tests/git_fork_sync.rs` 契约 + `fork_sync_result_serializes` 形状锁。门禁：cargo + vitest + typecheck。提交。

## Task 7: 头部死项转活 + 端到端回归

- [ ] 手动核 + 定向测试：`source-control-dropdown-remote-items.ts` 的 Fast-forward/Rebase/Sync-Fork 项经 store 动作打到现已实现的后端，不再 reject（`isRemoteOperationActive`/`inFlightRemoteOpKind` 正常翻转、`endRemoteOperation` finally 复位）。
- [ ] `source-control-dropdown-remote-items.test` 若存在则补断言这些 kind 映射到真实动作；无则跳过（后端契约已由 Task1/5/6 覆盖）。

## 收尾门禁（破坏性 + 网络，最重）

- [ ] `cargo test --workspace`（先 `pnpm build:orcinus-cli` 产 sidecar 若需）。
- [ ] `pnpm typecheck`；`pnpm build:web`。
- [ ] **`pnpm test` 全量**（O-2 live-shell 环境性挂起 `--exclude` 并标注）。
- [ ] 增量末**全分支 CodeReview**（重点：网络防挂起、rebase 冲突态闭环/abort、fork-sync 移植与 TS 参考逐步骤一致、§8.2 远端门无绕过、Update/Push owner-worktree 目标正确、软错误不经 BridgeError）。
- [ ] 手工验收（`pnpm dev`）：树里对已检出分支 Update/Push；Fast-forward/Rebase 触发；故意制造 rebase 冲突→能中止回滚；fork→Sync Fork 生效。

## 范围外 / 风险注记
- Update 的 merge/rebase 询问（§10 决策2：本期不问，按 git 默认）→ 不做。
- rebase 交互式/edit-todo、cherry-pick 等更深的 sequencer 态 → 仅覆盖"到 base 的 rebase + 冲突后 abort"。
- fork-sync 非 GitHub remote / 自定义 remote 名 → 以 `git-fork-sync.ts` 现有 GitHub 模型为准，未扩展。
- 大仓网络 fetch/pull 的真实延迟与凭证 → 依赖 `GIT_TERMINAL_PROMPT=0` 快速失败，不做交互式凭证。
