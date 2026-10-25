# 2B 交接卡（已核实事实 + 待核实清单）

生成于 2A 完成后。2A 的产物已在 `idea-branch-tree-switcher` 分支上（HEAD 见 `git log`）。
本文件是**被跟踪**的，不像 `.superpowers/sdd/` 下的 ledger 会被 `git clean -fdx` 清掉。

权威文档：spec `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` §13；
2A 计划 `docs/superpowers/plans/2026-10-25-branch-checkout-increment2a.md`（已完成）。

## 2B 要交付什么（spec §13.2 / §13.6 / §13.7）

1. `branch-menu.tsx`：分支行的右键 + 行尾 `⋯` 菜单，从 `BRANCH_ACTIONS` 取子集。
2. 动作表加 `checkout`，并成为宿主 A（Source Control）的**主动作**；
   `switch`（跳 workspace）降级为菜单次要项。
3. Checkout 前确认框（三段：目标 / 未提交改动 / 活跃终端）。
4. **spec §13.4 要求的元数据跟随测试**（2A 明确没做，无东西守着，不能再顺延）。

## 已核实、可直接依赖的事实

| 项 | 结论 |
|---|---|
| checkout 后端 | `window.api.git.checkout({ worktreePath, target })` → `Promise<GitCheckoutOutcome>`。手抄契约 `src/shared/git-checkout-types.ts`。 |
| 拒绝的表示 | `GitCheckoutOutcome.status` 是可判别联合：`'switched' \| 'already-on-branch' \| 'invalid-target' \| { refused: 'checked-out-elsewhere' }`。占用方路径在 `worktreePath` 字段。**不经 `BridgeError`**（其 `Serialize` 只留 `{message}`）。 |
| 后端 message 语言 | 英文，且**不是**用户文案。用户可见文案必须由前端按 `status` 走 i18n（spec §3.4：新命名空间只补 en+zh，其余靠 `translate` 的 defaultValue 兜底）。 |
| 远程 runtime | `checkoutRuntimeGit` 对非 local 目标**显式抛错**，不走 RPC。 |
| `useBranchRefs` 有 `invalidate()` | `src/renderer/src/components/git-branch/use-branch-refs.ts`，checkout 成功后要调。 |
| 树组件契约 | `BranchRefTree` 只吃 `onActivate: (row: BranchTreeRow) => void`，不含业务语义；宿主裁剪靠「不传某个回调」。 |
| 动作表 | `BRANCH_ACTIONS` / `branchActionForRef` / `primaryBranchAction` 在 `branch-actions.ts`；`switch.visible` 已委托给 `branchActionForRef`（不再重抄谓词）。 |
| 宿主 A 现状 | `source-control-branch-switcher.tsx` 里 ctx 只有 `activateWorkspace` / `newWorkspaceFrom`，**没有** `viewInGitLog`。 |
| 宿主 B 现状 | `git-log/Toolbar.tsx` 的 `onActivate` 直接映射到 `onPickRef`，**完全没接 `BRANCH_ACTIONS`** → 这就是 spec §13.6 末尾说的「裁剪机制要从『不传 viewInGitLog』扩成『宿主声明可用动作 id 集合』」的落点。 |
| 右键菜单原语 | `components/ui/context-menu.tsx` 导出 `ContextMenu / ContextMenuTrigger / ContextMenuContent`（188 行起）。 |
| 确认框先例 | `source-control/commit/discard-dialog.tsx` 用 `DialogContent` + `onConfirm` 回调，无内部确认态。可照抄其形状。 |
| 元数据同步机制 | `git-status-refresh.ts:147-153` 会 `updateWorktreeGitIdentity(worktreeId, { head, branch: status.branch ?? null })`。 |
| `Worktree.branch` | **完整 ref**（`refs/heads/x`）。与 `GitRefEntry.name`（短名）对齐必须过 `lib/git-utils.ts` 的 `branchName()`。 |
| 路径比较 | 比 worktree 路径一律用 `ade_git::are_worktree_paths_equal`（macOS `/var` → `/private/var`）。 |
| 新 tauri 命令的镜像面 | spec §3.5 的 7 面；另有两处**非强制但属仓库惯例**的登记：`real/git.ts` 的 `commandCases`（`git.test.ts`）与 `specta_export.rs` 的 `export_lists_every_command` 花名册。 |
| 无 pre-commit 钩子 | 门禁必须手动跑：`pnpm typecheck` + 定向 `pnpm vitest run` + `cargo test`。 |

## 写 2B 计划前**必须先核实**的（我这次没查到，别凭印象写）

1. **未提交改动条目从哪读**：`grouped` 出自 `source-control/listing/use-file-projection.ts`，
   但 `SourceControlBranchSwitcher` 是独立组件、不吃 panel foundation。要么它自己从 store 取
   （需定位 `gitStatus` 在 store 的确切键名与形状 —— `worktree-helpers.ts` 里没有），
   要么由宿主把计数传进去。**这条决定确认框的数据源设计，必须先查。**
2. **活跃终端计数**：`tabsByWorktree` 的 store 形状没定位到；已知 tab 上有
   `contentType === 'terminal'`，且 `group-tab-order.ts:54` 把 `'agent-session'` 与 terminal 并列处理
   —— 确认框文案要不要把 agent 会话也算进去，是个产品判断，问用户。
3. **`refreshActiveGitStatusAfterMutation` 的可达性**：签名是 `() => Promise<void>`
   （见 `commit/use-entry-mutations.ts:25`），但它在 foundation/hooks 里，
   `SourceControlBranchSwitcher` 目前不接 foundation。要么走 store 上的同名 action
   （需确认它是否在 `AppState` 暴露），要么宿主注入。**这条直接决定「60 秒才追上」那个 bug 会不会发生。**

## 必须写进 2B 计划的守卫（2A 已验证过它们有效）

- **元数据跟随测试**：checkout 成功后断言 store 里该 worktree 的 `branch` 变成新分支的完整 ref。
  没有它，2A reviewer 的 claim 3（不显式刷新则最长 60s 才追上，且轮询被 `isWindowVisible()` 门控）无人守。
- **`switch` 与 `checkout` 不串**：宿主 B 绝不能出现 `checkout` 动作（Git Log 面板不该原地改工作区）。
- **`checked-out-elsewhere` 给跳转而非普通报错**：断言 `worktreePath` 被用上了。
- **回归测试要实证判别力**：新增守卫后，临时还原被修的实现确认它会红，再还原。
  这一条在 2A 抓到过两个假守卫（`--track` 与 symlink）。
- **fixture 用生产形态**：`Worktree.branch` 写完整 ref；别用 `as unknown as` 把类型系统能报的错封住。

## 范围外遗留

**O-1 —— 已修（commit 4370f0d0）**：`git_fetch` / `git_pull` 已补齐，右侧栏 pull/sync
按钮的后端不再是 unimplemented。要点：
- 网络命令必须走 `ade_git::runner::run_git_in_with_env` 传 `GIT_TERMINAL_PROMPT=0`，
  否则私有 HTTPS 仓无凭证 helper 时会**挂到 300s 超时**而不是快速失败。
  `git_push` 目前**仍走旧的 `run_git_in`，没有这个保护** —— 同类隐患，尚未修。
- `git_fast_forward` / `syncFork` / `rebaseFromBase` **仍然不存在**（只补了 fetch/pull）。
  spec §3.7 里 Update 动作需要的就是 fetch+pull，已够；但 `fastForwardRuntimeGit`
  的本地路径依旧 reject。
- fetch 默认 `--prune`；pull 不加 `--ff-only`/`--rebase`（spec §12.2）。
- 集成测试用**本地 bare 仓当 remote**（无网、不弹窗、真跑）—— 这是测网络命令的可复用套路。

**O-3 —— 已修（commit 593f857d）**：`git_push` 改走 `run_git_in_with_env` +
`GIT_NETWORK_ENVS` + `GIT_NETWORK_TIMEOUT`，与 fetch/pull 同一入口。
顺带把超时从 120s 提到 300s（大仓推送走 120s 读超时是误杀）。
既有真推送用例（本地 bare remote + 断言 upstream 真被设上）在新配置下仍过。

**O-4（新）**：`GIT_TERMINAL_PROMPT=0` 本身**没有能区分的自动化守卫** ——
fetch/pull/push 三条 unreachable 用例用的都是本地路径 remote，git 对本地路径
根本不弹凭证提示，去掉那个 env 它们照样绿。能自动化的只有
`network_envs_constant_disables_terminal_prompt`（断常量内容）。
要真验证需要一个会要凭证的 HTTPS 远端（无网环境造不出来）。
测试注释里已写明这点，不要让后人误以为那三条用例守着 env。

**O-5（新）**：`fastForwardRuntimeGit` / `syncRuntimeGitForkDefaultBranch` /
`rebaseRuntimeGitFromBase` 的本地路径仍调 `window.api.git.fastForward/syncFork/rebaseFromBase`，
而这三个**仍无后端命令**。O-1 只补了 fetch/pull。

**O-2**：全量 `pnpm test` 有 1 条环境性失败
`src/shared/startup-shell-portability.live-shell.test.ts`（`Error: Command failed: /bin/zsh -i`），
本分支未碰该文件，但**尚未在用户机器上确认可复现**。

## 增量 1 的手工验收（尚未做）

需真开应用看，测试替代不了。`pnpm dev` 后逐项核：

1. 头部 `main` 可点（hover 有下划线），点开出树
2. 树里有 `HEAD (Current Branch)` / `Local` / `Remote > origin > …`，当前分支带 🏷 且在 Local 置顶
3. 搜索输 `f/` 之类段前缀能裁剪；**清空后折叠态不丢**
4. 点有 workspace 检出的分支 → 面板切过去，弹层关闭
5. 点无 workspace 的分支 → 新建工作区 composer 打开且基线已预填
6. Git Log：左栏没了、图谱变宽；点工具栏分支 chip 出同一棵树，顶部多 `All Branches`
7. 在 Git Log 树里选分支 → 按该 ref 重查；选 `All Branches` → 全量
8. ahead/behind 数字与 `git for-each-ref --format='%(refname) %(upstream:track)'` 逐条对得上
9. **弹层打开后不点任何东西、直接打字，列表应被过滤**
   —— 这条是唯一能验 rAF 聚焦的（`autoFocus` 在 Radix 焦点陷阱里活不下来，happy-dom 测不到）

注：2C 会移除头部弹层入口，第 1–5 条届时会重做；但**现在就做一遍**能尽早暴露
增量 1 自身的问题，而不是和 2B/2C 的改动混在一起排查。
