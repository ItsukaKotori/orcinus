# 分支树动作路线图：增量 3 / 4 / 5 切分（idea-branch-tree-switcher）

生成于 2C 完成后。2A（checkout 后端）+ 2B（checkout 前端）+ 2C（常驻区块）已交付「切分支」这条轴。
本文件把 spec §10 里排在 checkout 之后的**全部剩余动作**按风险切成 3 个独立计划，各自可绿可合。
每份计划动笔前必须先跑该节的「核实清单」——沿用 2B 交接卡的做法，不凭 spec 概述臆写实现细节。

权威 spec：`docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md`（§3.1 动作成本表、§3.5 七面镜像、§7.4 菜单矩阵、§8 错误/并发/刷新、§9 后端命令、§10 增量切分）。

## 切分总览（按风险递增）

| 增量 | 主题 | 新增后端 | 破坏性 | 远端 op（并发） | 门禁 |
|---|---|---|---|---|---|
| **3** | 非破坏·本地：Show Diff + New Branch from… + Set Upstream To… | `git_branch_create`、`git_branch_set_upstream` | 否 | 否（纯本地） | cargo ade-git/ade-bridge + typecheck + 定向 vitest |
| **4** | 破坏性：Delete + Rename + BranchOperationRefusal + F2 | `git_branch_rename`；抽公共 `ade_git::delete_branch` | **是** | 否 | 全量 `pnpm test` + `cargo test --workspace` + 全分支审查 |
| **5** | 网络/远端：Update + Push… 接线 + O-5 处置 | 视 O-5：可能 `git_fast_forward`/syncFork/rebase（或明确裁剪） | 否 | **是**（beginRemoteOperation 单飞） | 网络命令防挂起 + cargo + 全量 |

顺序理由：先本地非破坏（最低风险、无远端网络、无破坏），再破坏性（抽取 delete 须行为等价、二次确认），最后远端（并发最微妙 + 依赖 O-5 决策）。三者无硬依赖，但 4 的 `BranchOperationRefusal` 透出若 3 先做了 create 的结构化拒绝，可共用同一枚举——见下"接缝复用"。

## 接缝复用（跨增量，避免漂移）

- **结构化拒绝**：spec §8.1/§13.5 明确**不能经 `BridgeError`**（`Serialize` 压成 `{message}`），必须 `Ok(结果结构体)` + 可判别 `status`（2A 的 `GitCheckoutOutcome` 是先例）。增量 3 的 `GitBranchCreateOutcome`/`GitSetUpstreamOutcome` 与增量 4 的 `GitRenameOutcome`/`GitDeleteOutcome` 应**共用一个 `BranchOperationRefusal` 家族**（`NameTaken`/`CheckedOut`/`UpstreamMissing`/`NotMergeable`）。→ 增量 3 落地时把它抽成 `ade-git` 里的共享 enum，别各写各的。
- **7 面镜像**：每条新 tauri 命令都走 §3.5 的 7 面（实现/`git.rs` impl+command/`specta_export.rs` `collect_commands!`/再生 bindings/`real/git.ts` 映射/`preload-api` 类型/mock 不补）。模板 = 2A 计划 `2026-10-25-branch-checkout-increment2a.md`。
- **动作表扩展**：`branch-actions.ts` 目前 `BranchActionId = checkout|switch|new-workspace|view-in-log`。新增 `show-diff`/`new-branch`/`set-upstream`/`rename`/`delete` 逐个加。`BranchAction` 已有 `enabled?`（2B）。子菜单动作（Set Upstream）需要给 `BranchAction` 加「子项来源」维度——见增量 3 核实清单。
- **刷新分级**（§8.3）：目标==当前 workspace 分支 → `refreshActiveGitStatusAfterMutation` + `invalidate`；否则仅 `invalidate`。常驻区块（2C）已具备 invalidate。
- **i18n**：新文案进 `auto.components.git-branch`，只补 en+zh（§3.4）。
- **并发单飞**（§8.2，仅远端 op）：`beginRemoteOperation(kind)`/`endRemoteOperation()` try/finally 成对；发起前查 `isRemoteOperationActive` 为真则拒绝 + 菜单置灰。归增量 5。

## 增量 3 范围（本路线图的下一个计划，详表见 `2026-10-26-branch-create-upstream-increment3.md`）

动作（§7.4）：`Show Diff with Working Tree`（非当前分支，Local+Remote）+ `New Branch from…`（任意 ref 的 start-point）+ `Set Upstream To…`（子菜单列 `refs/remotes/*`，同名优先，仅 Local）。全部纯本地、无远端网络、非破坏。

## 增量 4 范围 + 动笔前必须核实

破坏性：抽公共 `ade_git::delete_branch`（从 `worktree_remove.rs:147` 的 `["branch", -d|-D, --, name]` + `is_checked_out_refusal`/prune 逻辑抽出，**worktree_remove 改为复用且既有测试断言不改**——抽取必须行为等价）；`git_branch_rename`（`git branch -m -- <old> <new>`，执行前 `worktree_list` 复核 `CheckedOut`，新名走 `resolve_create_branch_name`）；`Delete` 的 `-d`→`-D` 未合并二次确认（文案带分支名 + 未合并提交数，**不加**"输入分支名"式摩擦，§决策）；`F2` → Rename；Rename/Delete 仅未检出分支（advisory 置灰 + 后端复核）。
核实清单：`is_checked_out_refusal` 的现有判定（文本匹配，§3.8 脆弱点，抽取时不得让它更易错）；`-d` 失败如何区分"未合并"vs"被检出"（要类型化拒绝）；未合并提交数从哪来（`git rev-list --count` 还是既有 compare 结果）；Rename 对 `branch.<name>.*` config 的连带（git -m 自动迁 config？需实测，§决策 5 说未检出分支无 workspace 引用）。

## 增量 5 范围 + 动笔前必须核实

`Update`（fetch + 合并/ff + upstream 刷新）+ `Push…`（复用 `pushBranch`/`git_push`）+ **O-5 处置**：`git_fast_forward`/`syncRuntimeGitFork`/`rebaseFromBase` 三后端**仍不存在**，其前端方法（`runtime-git-sync-client.ts`）仍调缺失命令 → 死路径。要么补 `git_fast_forward`（`git merge --ff-only`）并把 Update 定义清楚，要么明确 Update = `git_pull --ff-only`（O-1 已给 fetch/pull）、砍掉 fastForward/syncFork/rebase 的 UI 入口。**这是产品决策，动笔前问用户**。并发单飞（§8.2）全部适用；网络命令防挂起（`GIT_TERMINAL_PROMPT=0` via `run_git_in_with_env`，见 ade git 网络命令记忆）。
核实清单：`runtime-git-sync-client.ts` 本地分支现调哪些 `window.api.git.*`、签名与软错误形状；`pushBranch` 参数与 pushTarget 语义；O-1 的 fetch/pull 是否已能支撑 Update 的最小形态；面板现有 pull/sync 按钮走的哪条链（增量 5 会顺带修好它们，收益超分支树）。

## 手工验收（各增量都需，测试替代不了）

起 `pnpm dev` 真窗口：树内右键/`⋯` 出对应动作、名字输入框可键入并校验、子菜单展开、破坏性二次确认、（增量5）远端 op 期间 spinner + 二次点击被拒。
