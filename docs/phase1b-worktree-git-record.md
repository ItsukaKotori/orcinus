# Phase 1 子项目 B：worktree + git 收尾记录

- 分支：`phase1b-worktree-git`（base `main@b0a14ae`，含 spec `a69caad` 与 plan `b0a14ae`）；本批次全程未 push、未 merge，合并/PR/保留由用户在合并前决定
- 日期：2026-09-28（macOS，darwin-arm64；Node v22.21.0，pnpm 12.4.2）
- 计划与 spec：`docs/superpowers/plans/2026-09-28-phase1b-worktree-git.md`；`docs/superpowers/specs/2026-09-28-phase1b-worktree-git-design.md`
- 证据目录（gitignored）：`.superpowers/sdd/2026-09-28-phase1b-worktree-git/`
  - 终局全量测试：`/var/folders/.../pnpm-test-phase1b.log`（exit 0）
  - GUI 冒烟：`dev-smoke-phase1b.log`（真实模式）、`dev-smoke-phase1b-mock.log`（mock 回退）
  - SDD 台账：`progress.md`；任务简报 `task-N-brief.md`；实现报告 `task-N-report.md`；评审包 `review-*.diff`

## 1. 批次与提交序（Task 0–15）

| Task | 提交 | 主题 |
|---|---|---|
| 0 | （无代码提交） | 分支与基线（cargo 351/0）、SDD 工作区、预检扫描 |
| 1 | `38f8fa9` | 容忍非零退出的 git runner 与取消/超时机制 |
| 2 | `bccfdd4` | porcelain v2 状态类型与增量解析器 |
| 3 | `f9f2d3e` | status 执行（限流/忽略/冲突/行统计） |
| 3 | `20638e8` | 修复：gitdir 标记按字节比较（字符边界 panic） |
| 4 | `a510a89` | blob 语义 diff 引擎（二进制/上限/compareAgainstHead） |
| 5 | `4acde77` | staging/discard/commit/upstream 本地操作 |
| 5 | `0fc15a9` | 修复：commit 将所有 git 执行失败折叠为 success:false |
| 6 | `e0673a4` | worktree 命名/路径/base ref 纯函数（oracle 对齐） |
| 7 | `2aed8ca` | worktree add/remove/prune 与分支保留/强删 |
| 7 | `3fd2f37` | 修复：分支删除降级保留 + 强删 checkout 守卫与恢复 |
| 8 | `a2d3d05` | branch/commit compare 与 history（控制者内联执行，见 §7） |
| 9 | `f1b3e57` | worktree 元数据持久化（worktrees.json） |
| 10 | `92695b4` | git 命令面、取消注册表与 bindings 登记 |
| 11 | `d115d98` | worktree 创建/删除/遗忘/强删/元数据命令 |
| 11 | `2afcf35` | 修复：displayName 投影语义、列表降级与 updateMeta 先校验 |
| 12 | `c76d2a4` | repos.create 与 base ref 查询命令 |
| 13 | `9a29e7c` | git 域真实适配层与 create-api 接线 |
| 14 | `e48402a` | worktrees/repos 真实适配扩展与 parity 更新 |
| 15 | `2615c7b` | 全量门禁、冒烟与收尾记录 |
| 15-fix | 本提交 | 终审修复：warnings 形状、symlink 路径归一、branch override 采用、git 授权守卫（4 Important） |

截至 Task 14：分支 diff 44 文件，+13238/−135（`git diff --shortstat main...HEAD`）。每个 Task 1–14 均经独立 reviewer 子代理评审（review 包存于证据目录）；Task 8 因 harness 子代理 SSE 超时改为控制者内联执行（§7）。

## 2. 增量命令面（+27，总计 78）

| 域 | 新增 | 命令 |
|---|---|---|
| git | 17 | `git_status git_cancel_status git_diff git_branch_compare git_commit_compare git_branch_diff git_commit_diff git_history git_stage git_bulk_stage git_unstage git_bulk_unstage git_discard git_bulk_discard git_commit git_upstream_status git_conflict_operation` |
| worktrees | 6 | `worktrees_create worktrees_remove worktrees_forget_local worktrees_force_delete_preserved_branch worktrees_update_meta worktrees_persist_sort_order` |
| repos | 4 | `repos_create repos_get_base_ref_default repos_search_base_refs repos_search_base_ref_details` |

- Rust：`ade-git` 新增 `runner/status/status_read/diff/staging/branch/worktree_create/worktree_remove/compare/history/command` 模块；`ade-store` 新增 `WorktreeMetaStore`（`worktrees.json`，内部 Mutex、全 `&self`）；`ade-bridge` 新增 `commands/git.rs`、`GitCancelRegistry`（`AppState.git_cancels`）、worktrees/repos 扩展；`CoreError` 增 `GitCommandFailed`/`GitCommandCancelled`；bindings 重生成（新增 27 命令与类型）。
- TS：新增 `src/bridge/real/git.ts`（17 方法）；`worktrees.ts`/`repos.ts` 各增 6/4 方法；`create-api.ts` 把 `git` 纳入 real 域；parity/契约测试同步迁移。

## 3. 对齐语义与偏差

### 3.1 spec §8 有意偏差（实现结果）

1. worktree 删除不实现 rename-to-trash 快速路径（直接 `worktree remove`；语义等价、性能略差）。
2. 创建不执行 setup/archive hook、不处理 `.worktreeinclude`/`orca.yaml` shared dirs、不处理 repo.symlinkPaths（留 C/Phase 2）。
3. `git-username` 前缀的 gh CLI 解析不实现（仅 `github.config github.user` → `user.username`）；emoji 名称目录不移植（emoji-only 名称报 `Invalid worktree name`）。
4. `upstreamStatus.behindCommitsArePatchEquivalent` 恒 false；status 缓存/负缓存不实现。
5. `git status` 用 `-z` 形态解析（oracle 非 -z + C-quote 解码），输出契约一致；status 为全量读入后截断（oracle 流式 kill；`didHitLimit`/`statusLength` 语义保持，内存上界弱于 oracle）。
6. worktree 列表 git 失败按 repo 降级为空并跳过（oracle 行为，替代 A 的 fail-fast）。
7. `repos.create` 在 projects 锁内同步跑 git（生命周期所致；最长 2×10s 阻塞其他 store 命令）。
8. compare 的 name-status 用 `-z`；硬读失败（diff）上抛 Err（oracle 降级空内容，UI 以错误卡片呈现）；`history` 的 log 失败上抛。
9. `worktrees.create` 结果只含 `{ worktree }`：Rust 不发射 `warnings`（TS `CreateWorktreeResult.warnings` 是 `WorktreeLineageWarning[]` 形状，B 无 lineage 数据），配置失败仅 `eprintln!` 日志（终审修复，替代 spec §4.4 的 `warnings?: []` 字面最小子集）。

### 3.2 review 暴露并修复的关键点

- status 解析器 gitdir 标记的字符边界 panic（修复 `20638e8`）。
- commit 的 spawn/超时未按 oracle 折叠 `{success:false,error}`（修复 `0fc15a9`）。
- 分支删除失败应降级为保留/Skipped（绝不判整体失败）；强删需 checked-out 前后守卫与恢复（修复 `3fd2f37`）。
- worktree 投影 `displayNameMode` 必须为 `'fixed'|'automatic'` 并按 `displayNameIsPinned`/`cliProvenance` 判定（修复 `2afcf35`）。
- `updateMeta` 必须先校验 worktree 存在再写盘；worktree 列表按 repo 降级（修复 `2afcf35`）。
- 终审 4 Important（修复本提交）：
  1. `WorktreesCreateResult.warnings` 的 Rust `string[]` 与 TS `WorktreeLineageWarning[]` 形状不符 → 移除字段，配置失败只记日志。
  2. symlink 工作区根下 create 用词法路径作 meta key、git 返回真实路径 → create 在 checkout 后报 "Worktree not found"、meta 孤儿 → 统一 `ade_git::canonical_worktree_path`；remove/updateMeta/forget 的 id 解析 exact-first、canonical 回退（保住 folder workspace 的 verbatim id）。
  3. `branchNameOverride` 命中已存在本地分支未走 checkout-existing → 按 oracle `canCheckoutExistingLocalBranch` 采用该分支（`git worktree add <path> <branch>`，无 `-b`/`--no-track`，跳过 base/upstream 配置，meta 写 `preserveBranchOnDelete:true`）；override 不再套 branchPrefix，并加 `check-ref-format --branch` 校验。
  4. git 命令未过 fs 授权 → 每个命令先 `FsService::resolve(worktreePath)`；含 `filePath`/`oldPath` 的命令拒绝绝对路径与含 `..` 段的相对路径。

### 3.3 已知近似与延后 minor（来自各任务评审，完整见台账）

- status：`conflict_compatibility` 仅 NotFound→deleted（缺 ENOTDIR）；ignored_paths 未随 limit 截断；entries 双存内存翻倍；`attach_line_stats` 无取消；历史遗留切片点。
- diff：超限 blob 先整体缓冲；`LargeDiffRenderLimit::Unlimited.limited` 为 bool；对 index 缺失回退 HEAD/二进制删除的测试缺口。
- staging/upstream：`rev-list` 畸形输出 (0,0)；TOCTOU 窗口。
- worktree：`delete_branch` squash-merge 树等价重试未移植（保留分支，安全方向）；`is_branch_checked_out` 忽略 prunable 注册；missing-registration 文案后缀缺失；`finish` 取消注册竞态（唯一 token 下理论）；名称冲突匹配偏宽；`updateMeta` 对 folder workspace 不投影；`persistSortOrder/forgetLocal` 未走 run_blocking；git 授权守卫的 `filePath` 只拒绝绝对路径与 `..` 段，未逐路径 canonicalize（worktree 内 symlink 父目录的间接逃逸未拦，按终审要求的最小面实现）。
- TS：mock 侧 worktrees.create/remove 等仍 reject（后续 mock 任务）；`setStatusUpstreamRefWatch` 维持 fallback（warn 噪音）；refs 排序用 `String::cmp`（非 locale）。
- 全量 `pnpm test` 既有性能 flake（`browser-history-match.performance`，phase1a 已记录；本次终局未触发）。

## 4. 测试与门禁证据（Task 15）

- Rust：`cargo test --manifest-path src-tauri/Cargo.toml --workspace` → **546 passed / 0 failed**（基线 351；+195）。
- fix wave（本提交）：`cargo test --workspace` → **556 passed / 0 failed**（+10：symlink 工作区根全链路、override adopt/去前缀/checked-out/diverged、folder id verbatim、git 授权守卫、相对路径守卫）；`pnpm vitest run src/bridge` → **352 passed**；`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` → exit 0（仅既有 chunk-size 警告）；bindings 重生成（`WorktreesCreateResult` 去 `warnings`）且 `bindings_are_fresh` 绿。
- 前端冷门禁：`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` → exit 0（仅既有 chunk-size 警告）。
- 全量前端：`pnpm test` → **3848 passed | 8 skipped（3856 文件）；34245 passed | 122 skipped（34367 测试）；0 failed**；540.04s；`grep -cE '^\s*FAIL'` = 0。较 phase1a 验收后基线（34204）**+41 测试**；`src/bridge` 域测试 352 通过（新增 git/worktrees/repos 契约与 parity）。

## 5. 手工验收（自动化部分）与待用户复核

### 5.1 启动冒烟（本任务执行）

真实模式（`dev-smoke-phase1b.log`）：

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.74s
Running `target/debug/orcinus-app`
```

- 进程存活（pid 27052），System Events 可见 `orcinus-app`；日志 0 error / 0 panic；TERM 后进程退出、端口 1420 释放、无残留。
- 注：本机 python 无 pyobjc，CGWindowList 证据不可用（phase1a 机器有）；以进程存活 + 日志 + 端口为准。

Mock 回退模式（`dev-smoke-phase1b-mock.log`）：`Running target/debug/orcinus-app`、进程存活（pid 32293）、0 error / 0 panic、退出干净。

### 5.2 待用户交互复核清单（对应计划 Task 15 Step 2）

1. 添加本地 git 仓库 → 创建 worktree（base 选择、nest 目录）→ 侧栏出现并可切换
2. 新 worktree 编辑文件 → Git 面板出现 status（modified/untracked）；stage/unstage/bulk → diff 显示两侧内容
3. commit → `git log` 可见；工作区变干净
4. 删除 worktree：干净直删；脏目录 force；未合并分支保留 → 强删
5. 重命名/pin/已读 → 重启恢复（`worktrees.json`）
6. 新建项目（repos.create）：空目录 → git init + Initial commit；identity 未配置时的提示文案
7. `VITE_ADE_BRIDGE=mock pnpm dev` 回退行为正常

补充待复核项（phase1a 遗留 + 本批）：大 payload diff 经 IPC 的往返表现；`git_branch_compare`/`history` 在真实仓库的 UI 呈现（本批仅组件级/契约级验证）。

## 6. 延后项

| 项 | 内容 | 跟踪 |
|---|---|---|
| 远端操作 | fetch/push/pull/fastForward/syncFork/upstream pushTarget、remoteFileUrl/remoteCommitUrl、PR/MR start point、克隆 | spec §2.2 |
| AI 辅助 | `generateCommitMessage`/cancel、`generatePullRequestFields`/cancel、`discoverCommitMessageModels` | spec §2.2 |
| 子模块 | `submoduleStatus`、submodule diff 路由 | spec §2.2 |
| 大仓库辅助 | `checkIgnored`/`findHugeFoldersToIgnore`/`appendGitignore` | spec §2.2 |
| worktree 创建增强 | `.worktreeinclude` 复制、orca.yaml hooks、symlinkPaths、退休名注册表、prefetchCreateBase、PR base 解析、sparse checkout | spec §8.2 |
| git 状态推送 | 主进程 `.git` 元数据 watch → `worktrees:gitStatusMetadataChanged` 等事件（当前靠渲染端信号 + 60s 安全轮询） | spec §2.2 |
| mock 残渣 | mock worktrees.create/remove 等仍 reject；远端域维持响亮未实现 | spec §9 |
| Windows | 路径大小写/长路径/git 输出差异、`projects.update` 落盘、全量测试与启动 | spec §8.6 |

## 7. 执行过程记录（Rulings）

- Task 8 三次子代理派发均因 harness SSE 超时中断（未提交）→ 控制者内联接手其脚手架并完成实现；评审仍由独立子代理完成。代价：控制者上下文增长、与 SDD 标准流程偏离。
- 预检修正（计划内直接修正并记录）：Store 方法 `&self` 化；Task 10 测试直调 `ade_git`；`includeLineStats: None→true` 的映射裁定。
- 终审 fix wave 裁定：`warnings` 选择移除字段而非对齐 lineage 形状（B 无 lineage 数据、TS 字段可选，绑定面最小）；canonical 化复用 `ade_git::canonical_worktree_path`，id 解析 exact-first + canonical 回退以保住 folder workspace 的 verbatim id；override 按 oracle 去前缀并加 `check-ref-format --branch` 校验；授权守卫在 async 命令体内锁外同步 `resolve`。
- 其余裁定见 SDD 台账 `progress.md`（每项含成本说明）。

## 8. 收尾状态

- 门禁：cargo 556/0（fix wave 后）、冷 `typecheck`+`build:web` exit 0、`pnpm test` 0 failed（3848 文件 / 34245 测试）、真实 + mock 冒烟通过；终审 4 Important 已全部修复（本提交）。
- 工作树干净；分支未 push、未 merge。
- 交付选项（用户决定）：合并到 `main` / 开 PR / 保留分支。
