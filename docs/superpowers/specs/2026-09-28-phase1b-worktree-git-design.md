# Phase 1 子项目 B：worktree + git（status/diff/commit、worktree 创建/切换/删除）设计规格

- 日期：2026-09-28
- 状态：brainstorming 输出（用户已授权直接进入实现；本规格为实现依据）
- 基线：`main@c88ae2c`（Phase 1A 及其验收修复已合入，`pnpm test` 34204 通过、cargo 351 通过）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；下文 `orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§8 Phase 1）、`docs/superpowers/specs/2026-09-23-phase1a-open-project-design.md`（§2.3 B 范围、§10 偏差）

## 1. 背景与目标

Phase 1 拆为 A 打开项目纵切（已完成）、B worktree + git（本规格）、C 终端/PTY + agent。B 的目标是在 A 的真实 Rust 后端与真实桥接之上，接通渲染层 Git 面板与 worktree 生命周期：

**验收（自动 + 手工）**：`pnpm dev` 下 —— 创建 worktree（选 base ref、分支命名、目录落位）→ 侧栏出现并可切换 → 编辑文件 → Git 面板显示 status → stage/unstage → 打开 diff → commit 成功（`git log` 可见）→ 删除 worktree（干净直删、脏目录需 force、未合并分支保留并可强删）→ 重启后 worktree 列表与用户元数据（重命名/pin/已读）恢复。自动化门禁：`cargo test --workspace` 全绿 + `pnpm typecheck && pnpm build:web` exit 0 + `pnpm test` 全绿（既有 3847 文件套件 + 新增）。

## 2. 范围

### 2.1 B 实现（真实 IPC）

| 域 | 方法 | 说明 |
|---|---|---|
| git | `status` `cancelStatus` | porcelain v2 解析、branch/upstream、limit 1000、includeIgnored、行统计（includeLineStats）、requestToken 取消 |
| git | `diff` `branchDiff` `commitDiff` | 整文件两侧内容（blob 语义，非 patch），staged/unstaged/compareAgainstHead、二进制三态、10 MiB 上限 |
| git | `branchCompare` `commitCompare` | 文件清单 + summary（含行统计） |
| git | `history` | `git log` 提交历史（limit/baseRef、refs 装饰） |
| git | `stage` `bulkStage` `unstage` `bulkUnstage` `discard` `bulkDiscard` | 工作区操作 |
| git | `commit` | `git commit -m`，返回 `{success,error}`（不 reject 域错误） |
| git | `upstreamStatus` | 本地 ahead/behind（`rev-list`，不触网）；无 upstream 时 `{hasUpstream:false,ahead:0,behind:0}` |
| git | `conflictOperation` | merge/rebase/cherry-pick 进行中检测 |
| worktrees | `create` | base ref 解析、分支命名（前缀/清洗/去重）、目录计算（workspaceDir/nest）、`add --no-track -b`、`branch.<name>.base` 与 `push.autoSetupRemote` 配置 |
| worktrees | `remove` `forgetLocal` `forceDeletePreservedBranch` | 预检（locked/脏）、`worktree remove [--force]`、`branch -d` 保留语义、CAS 强删分支 |
| worktrees | `updateMeta` `persistSortOrder` | 用户元数据持久化（重命名/pin/归档/已读/comment/排序/保留分支等） |
| worktrees | `list` `listAll` `onChanged` | 既有投影扩展：合并元数据；git 失败按 repo 降级为空并跳过（对齐 oracle，替代 A 的 fail-fast） |
| repos | `create` | 新建项目：目录创建 + `git init` + 空初始提交（git kind），`{repo}\|{error}` 返回 |
| repos | `getBaseRefDefault` `searchBaseRefs` `searchBaseRefDetails` | 本地 ref 枚举（创建面板 base 选择） |

### 2.2 B 响亮未实现（保持 `UnimplementedBridgeError`）

- 远端操作：`git.fetch push pull fastForward syncFork upstreamStatus 的 pushTarget 解析`、`remoteFileUrl remoteCommitUrl`、`worktrees.resolvePrBase resolveMrBase`、`repos.clone cloneRemote createRemote addRemote cloneAbort onCloneProgress`
- AI/评审：`git.generateCommitMessage cancelGenerateCommitMessage generatePullRequestFields cancelGeneratePullRequestFields discoverCommitMessageModels`、`submoduleStatus`
- 巨大仓库辅助：`checkIgnored findHugeFoldersToIgnore appendGitignore`
- worktree 辅助/远端：`listRetiredNames prefetchCreateBase adoptProvisionedRoot listDetected 的 host 限定分支 listKnownForExecutionHost forgetRemovedForExecutionHost`、`updateLineage/listLineage` 维持既有空实现、`onCreateProgress/onGitStatusMetadataChanged/onHeadIdentitiesChanged/onBaseStatus/onRemoteBranchConflict` 维持 noop 订阅
- repos：`getGitUsername`（分支前缀 username 解析内联于 Rust，不单独开命令）

### 2.3 明确不做

- 终端/agent（C）、`.worktreeinclude` 复制与 `orca.yaml` setup/archive hook（B 创建不执行脚本，记录偏差）、sparse checkout、远端执行主机、Windows 验证、git 面板的历史轮询推送（沿用渲染层信号 + 60s 安全轮询）。

## 3. 架构

### 3.1 Rust crate 改动

```
src-tauri/crates/
├── ade-core/    # errors.rs 增补 Git 命令失败变体；models/worktree.rs 元数据合并所需字段
├── ade-store/   # 新增 worktree_meta_store.rs（worktrees.json 原子持久化）
├── ade-git/     # 扩展：command runner（容忍非零退出/保留 stderr/超时/取消）、status、diff、
│                # staging/commit、refs（base 解析/分支命名）、worktree add/remove/prune、compare/history
├── ade-bridge/  # 新增 commands/git.rs；worktrees.rs 增 create/remove/updateMeta 等；
│                # repos.rs 增 create/getBaseRefDefault/searchBaseRefs/searchBaseRefDetails；
│                # state.rs 增 worktree meta store 与 CancelRegistry；events 复用；specta 登记
└── orcinus-pty/ # 不动
```

### 3.2 命令面（新增，命名 `<域>_<方法 snake_case>`）

- git（17）：`git_status git_cancel_status git_diff git_branch_compare git_commit_compare git_branch_diff git_commit_diff git_history git_stage git_bulk_stage git_unstage git_bulk_unstage git_discard git_bulk_discard git_commit git_upstream_status git_conflict_operation`
- worktrees（6 新增）：`worktrees_create worktrees_remove worktrees_forget_local worktrees_force_delete_preserved_branch worktrees_update_meta worktrees_persist_sort_order`
- repos（4 新增）：`repos_create repos_get_base_ref_default repos_search_base_refs repos_search_base_ref_details`
- 参数统一 `{ args }` 包裹；Rust 结构体 `#[serde(rename_all = "camelCase")]` 与 `src/shared/preload-api/api/*.ts` 逐字对齐（实现前通读 `git-inspection-api.ts` / `git-operation-api.ts` / `worktree-api.ts` / `repository-api.ts` 与 `src/shared/{git-status-types,git-diff-compare-types,git-history-types,worktree/create-types,worktree/types,worktree/meta-types}.ts`）。

### 3.3 前端桥接

- 新增 `src/bridge/real/git.ts`（实现上述 git 方法；事件订阅无）；`src/bridge/create-api.ts` 把 `git` 加入 `RealDomains` 与 `createRealDomains`（当前完全缺席，real 模式落到 mock 空实现）。
- 扩展 `src/bridge/real/worktrees.ts`、`src/bridge/real/repos.ts`；`worktrees.create/remove` 的结果直接采用 Rust 返回形状。
- 契约/parity 测试更新：`parity.test.ts` 中对应方法从「未实现」清单迁出，新增命令名/参数包裹/错误透传/返回值关键字段断言。

### 3.4 持久化与授权

- 新增 `worktrees.json`（`WorktreeMetaStore`，schemaVersion 1）：`{ schemaVersion, items: { "<worktreeId>": { ...meta } } }`；原子写 + 备份轮换复用 `JsonFile`。worktreeId 维持 `"{repoId}::{path}"`。
- 投影合并：`list_worktrees` 读取 meta store 条目合并到 `Worktree`（displayName/comment/isArchived/isPinned/isUnread/sortOrder/manualOrder/workspaceStatus/lastActivityAt/preserveBranchOnDelete/createdWithAgent 等，以 `src/shared/worktree/meta-types.ts` 的 `WorktreeMeta` 白名单为准）；未持久化字段保持 oracle 默认。
- 授权：create 成功后 `fs.authorize_root(newPath)`；remove/forget 后若该路径不被任何 repo/folderWorkspace/其他 worktree meta 引用则 `revoke_root`（扩展 `revoke_root_if_unused` 判定或新增等价函数）。
- 事件：create/remove/forget/forceDelete 后发 `worktrees:changed {repoId}`；repos.create 后发 `repos:changed`；git 命令不新增事件（渲染端在变更命令后主动刷新，A 已具备）。

## 4. 行为语义（对齐 oracle，实现时必须逐条对照）

### 4.1 git status（`orca:src/main/git/source-control/status-read.ts:40`、`orca:src/shared/git-status-porcelain-parser.ts`、`orca:src/shared/git-status-types.ts`）

- 命令 `git -c core.quotePath=false status --porcelain=v2 --branch --untracked-files=all`（`includeIgnored` 追加 `--ignored=matching`）；env 强制 `GIT_OPTIONAL_LOCKS=0`；Rust 实现用 `-z` 形态（NUL 记录）以规避引号解码（输出契约不变，记录为实现细节）。
- 限流 `DEFAULT_GIT_STATUS_LIMIT = 1000`：观察计数 `> limit` 才截断（entries 取前 limit、`didHitLimit:true`、`statusLength` 为观察总数）；limit 0 关闭。
- branch 头：`branch` 规范化为 `refs/heads/<name>`，detached 为缺省（非空串）；`head` 为完整 OID；`upstreamStatus` 来自 `# branch.ab`。
- 失败语义：非 abort 的 git 失败 → 空 entries 成功返回（容错）；`requestToken` 取消 → reject（对齐 abort）。
- 行统计（`includeLineStats` 时）：`git diff --numstat -z -M`（unstaged）+ `--cached`（staged），untracked 走 fs 统计；字段 `added/removed`。
- `conflictOperation` 独立命令：检测 merge/rebase/cherry-pick 进行中（`orca:src/main/git/…` 对应实现为准）。

### 4.2 git diff（`orca:src/main/git/source-control/file-diff.ts:32`、`git-blob-read.ts`）

- 非 patch 语义：staged 左=`git show HEAD:<path>` 右=index blob；unstaged 左=index（失败回退 HEAD）右=工作区文件；`compareAgainstHead=true` 时 unstaged 左=HEAD。
- 二进制判定：前 8192 字节含 NUL → `kind:'binary'`；白名单 MIME（`.png .jpg .jpeg .gif .svg .webp .bmp .ico .pdf`）给 base64、`isImage`（PDF 也 true），其余内容置空；`modifiedDeleted` 仅在证明删除时置位。
- 单侧 > `MAX_GIT_SHOW_BYTES = 10 MiB` 按二进制处理（不报错）。
- 渲染级截断（120k 行/侧、6M 字符合计，`largeDiffRenderLimit`）按 oracle 的归属层实现（oracle `diff-result.ts:28-39`；实现前确认在主进程还是渲染端，保持一致）。
- `branchDiff`：左=`git show baseOid:<oldPath?>` 右=`git show headOid:<path>`；`commitDiff`：左=parentOid（或空树）右=commitOid。

### 4.3 stage/unstage/discard/commit/upstream（`orca:src/main/git/source-control/staging.ts`、`discard-changes.ts:15`、`commit-changes.ts:6`、`git-conflict-operation.ts`、`orca:src/main/ipc/filesystem/filesystem-git-commit-handlers.ts:13`）

- stage/unstage/discard 逐条对照 oracle `staging.ts`/`discard-changes.ts`（含无 HEAD 仓库的 unstage 语义、untracked discard 的删除路径、glob/路径转义、符号链接安全）；bulk 用一次多 pathspec 或循环，错误聚合语义对齐 oracle。
- commit：仅 `git commit -m <message>`（无 `--author`/`--no-verify`）；message trim 空 → reject `'Commit message is required'`；失败 → `{success:false,error}`，错误文本优先 stderr → stdout → 固定 `'Commit failed'`；成功 → `{success:true}`。
- upstreamStatus：`git rev-list --left-right --count <upstream>...HEAD`（本地）；`hasUpstream:false` 时 `{ahead:0,behind:0}`；`upstreamName` 取配置。（不实现 `behindCommitsArePatchEquivalent` 的深度探测，缺省 false；记录偏差。）

### 4.4 worktree create（`orca:src/main/git/worktree-add.ts:150`、`orca:src/main/ipc/worktree-logic.ts`、`orca:src/main/ipc/worktree-remote.ts:2301`、`orca:src/shared/branch-prefix.ts`）

- 名称：`sanitizeWorktreeName`（保留 Unicode 字母数字 `._-`，其余连续字符→`-`，`..`→`.`，首尾 `[.-]` 去除；空→error `'Invalid worktree name'`）。
- 分支名：设置 `branchPrefix`（默认 `git-username`）→ `custom`（`branchPrefixCustom`）→ `none`；前缀规范化折叠斜杠；username 解析 `git config --get github.user` → `git config --get user.username`（gh CLI 不实现，记录偏差）；最终 `<prefix>/<name>`（前缀为空则裸 name）。
- base ref：显式 `baseBranch` → repo.worktreeBaseRef → 默认探测 `refs/remotes/origin/main`→`origin/main`→`refs/remotes/origin/master`→`origin/master`→`refs/heads/main`→`main`→`refs/heads/master`→`master`（顺序与 `repo-default-base-ref.ts:21-26` 一致）；解析不到 reject `Could not resolve a default base ref...`。
- 目录：根 = repo.worktreeBasePath || settings.workspaceDir（默认 `{{HOME}}/orca/workspaces`）；`nestWorkspaces` 默认 true → `<root>/<repoBasename>/<name>`；越界校验 `ensurePathWithinWorkspace`。
- 执行：`git worktree add --no-track -b <branch> <path> <effectiveBase>`（分支已存在走 `<path> <branch>` 形态；B 本地不检出现有分支，除非 `branchNameOverride` 命中的分支已存在——实现时对照 oracle 重试分类器）；本地/远端分支或路径冲突 → 后缀 `-2..-100` 重试，超限 error `'Worktree creation failed ...'`（实现时对照 `worktree-create-candidates.ts:8,11` 与渲染端 `isRetryableWorktreeCreateConflict`）。
- 成功后：`git -C <path> config --local --replace-all branch.<branch>.base <effectiveBase>`（失败 unset branch section）、`push.autoSetupRemote true`（仅当未设置；warn-only）；写 meta（displayName=name、workspaceStatus 默认 `'in-progress'`、createdAt 等）；授权路径；发 `worktrees:changed`。
- 失败：不回滚已创建的 checkout（对齐 oracle；仅生成名退休，B 不实现退休注册表）。
- 返回 `{ worktree, warnings?: [] }`（最小子集，其余可选字段缺省；渲染端读取 `result.worktree` 为准）。

### 4.5 worktree remove / forget / forceDelete（`orca:src/main/git/worktree-removal.ts`、`worktree-removal-preflight.ts`、`worktree-branch-removal.ts`）

- 预检：worktree 必须存在且未 locked（locked reject，提示 `git worktree unlock`）；非 force 时 `git status --porcelain -z --untracked-files=all` 脏 → reject 文案逐字 `Worktree has uncommitted or untracked changes.`（附 stdout）。
- 删除：`git worktree remove [--force] <path>`（不实现 oracle 的 rename-to-trash 快速路径，记录偏差）；失败回退 `git worktree prune` 后重试校验。
- 分支：默认 `git branch -d -- <branch>`；未合并失败 → 保留并返回 `{ preservedBranch: { branchName, head? } }`；meta `preserveBranchOnDelete:true` → 不删；主工作树对应分支不可删（git 拒绝透传）。
- `forgetLocal`：仅移除 meta + revoke 授权 + `worktrees:changed`，不碰磁盘/git。
- `forceDeletePreservedBranch`：`git update-ref -d refs/heads/<name> <expectedHead>`（CAS 不匹配 reject）+ `git config --remove-section branch.<name>`；返回 `{deleted:true}`。
- 删除并发：同 worktree 删除进行中去重（行为对齐 oracle；B 可用简单 mutex 集合，若不实现则记录）。

### 4.6 worktrees.updateMeta / persistSortOrder

- `updateMeta({worktreeId, executionHostId?, updates})`：白名单合并（`WorktreeMeta` 键）→ worktrees.json 落盘 → 返回合并后的完整 `Worktree` 投影；`sortOrder`/`manualOrder` 数值、`isPinned/isArchived/isUnread` 布尔、`displayName/comment` 字符串原样；未知键忽略。`displayName` 置空串 → 清除 meta（回落投影默认名）。
- `persistSortOrder({orderedIds})`：按序写 sortOrder（0..n），落盘。

### 4.7 repos.create 与 base refs（`orca:src/main/ipc/repos/repo-creation-handlers.ts:132`）

- 校验：name trim 非空（空 → `{error:'Name cannot be empty'}`）、无 `\/` 且非 `.`/`..`（→ `{error:'Name cannot contain slashes or be "." / ".."'}`）、parentPath 绝对路径（→ `{error:'Parent directory must be an absolute path'}`）；`targetPath = join(parent, name)` 按 path 去重（同 `normalize_for_comparison`）。
- 目录：mkdir parent 递归；target 已存在且非空 → `{error:'"<name>" already exists at this location and is not empty.'}`；已存在且为空则沿用；不存在则 mkdir（EEXIST 竞态 → 复用已有 repo）。
- git kind：`git init` + `git commit --allow-empty -m 'Initial commit'`；commit 失败且 stderr 命中 identity 特征 → 逐字错误 `'Git author identity is not configured. Run `git config --global user.name "Your Name"` and `git config --global user.email "you@example.com"`, then try again.'`；其余 `{error:'Failed to initialize git repository: …'|'Failed to create initial commit: …'}`；失败清理：自建目录 rm、预存在目录仅 rm `.git`。
- 成功后 `add_repo`（复用 A 的 `add_repo`：badgeColor `#737373`、去重、授权）+ `repos:changed`。
- `getBaseRefDefault`：探测默认 base（顺序同 4.4），返回 `{defaultBaseRef, remoteCount}`；`searchBaseRefs/searchBaseRefDetails`：枚举 `refs/heads` + `refs/remotes`（排除 `*/HEAD`），按 query 子串过滤、limit 截断（对齐 oracle `searchBaseRefs` 排序/形状）。

## 5. 错误模型与取消

- `ade-core::errors::CoreError` 新增 `GitCommandFailed { command: String, stderr: String, exit_code: Option<i32> }`（Display 携带 stderr 首行/全文由 bridge 决定）；既有函数（`rev_parse_toplevel` 等）语义不变。
- `ade-bridge` 命令返回 `Result<T, BridgeError>`；域错误（commit 失败、repos.create 校验、base 解析失败）按 4.x 以返回值或 reject 表达，逐条对齐 oracle 与 TS 消费点。
- `CancelRegistry`（`Mutex<HashMap<String, Arc<AtomicBool>>>`）置于 `AppState`；`git_status` 注册 token，`git_cancel_status` 置位；runner 对 status 子进程流式读取并轮询取消位，取消即 kill 并 reject（对齐 abort 语义）；读超时 `GIT_READ_TIMEOUT_MS = 120_000`，worktree add 超时 `180_000`，删除 preflight 30_000。

## 6. 测试策略

- `ade-git`：解析器 fixtures 表驱动单测（status porcelain v2 含 rename/空格路径/detached/conflict code/ab 头；numstat）；tempdir + 真实 `git init` 集成测试（status/stage/commit/diff/worktree add-remove/compare/history），模板沿用 `ade-git/tests/cli_integration.rs`。
- `ade-store`：worktree meta store 往返/损坏回退/白名单合并单测。
- `ade-bridge`：纯函数投影测试（merge meta、授权回收、base 解析、分支命名/清洗/路径计算、错误映射）；命令级集成测试沿用 `ade-bridge/tests/repos.rs` 的 tempdir+git helper 模式；specta bindings 新鲜度与命令清单测试更新。
- TS：`src/bridge/real/git.test.ts`（命令名、`{args}` 包裹、错误透传、事件无）、`worktrees.test.ts`/`repos.test.ts` 扩展、`parity.test.ts` 迁移清单；`create-api.test.ts` 断言 git 走 real。
- 门禁（每任务）：`cargo test --workspace`；`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`；终局 `pnpm test` 全绿。

## 7. 平台与风险

- macOS 为验证平台；Windows（路径大小写、`core.quotePath`、长路径、`git worktree` 输出）记录跟踪不在本子项目验证。
- 风险：porcelain v2 细节漂移（用 oracle fixtures 锁定）；diff 大 payload 经 IPC 的内存表现（10 MiB/侧上限缓解）；worktree 创建重试分类器与渲染端 `isRetryableWorktreeCreateConflict` 耦合（错误文案需包含该分类器识别的特征，实现时对照 `orca:src/renderer/src/store/slices/worktrees/create/create-worktree.ts:187-227` 与 Orcinus 渲染层同文件）；meta store 与投影合并的字段漂移（白名单 + 契约测试）。

## 8. 有意偏差（与 oracle）

1. worktree 删除不实现 rename-to-trash 快速路径（直接 `worktree remove`），语义等价、性能略差。
2. worktree 创建不执行 setup/archive hook、不处理 `.worktreeinclude`/`orca.yaml` shared dirs（留 C/Phase 2）；repo.symlinkPaths 也不处理。
3. `git-username` 前缀的 gh CLI 解析不实现（仅 `github.user` → `user.username`）。
4. `upstreamStatus.behindCommitsArePatchEquivalent` 恒为 false；`didHitLimit` 之外的 status 缓存/负缓存不实现（无 oracle 的主进程缓存层，渲染端轮询频率不变）。
5. `git status` 用 `-z` 形态解析（oracle 非 -z + C-quote 解码），输出契约一致。
6. worktree 列表 git 失败按 repo 降级为空（oracle 行为），替代 A 的 fail-fast。

## 9. 交付与延后

- 交付：分支 `phase1b-worktree-git`（Task 0 创建，base `main@c88ae2c`）；收尾记录 `docs/phase1b-worktree-git-record.md`；验收后由用户决定合并。
- 延后：2.2 全部；`.worktreeinclude`/hooks；git 状态主进程 watch 推送；Windows 验证；`checkIgnored/appendGitignore`。
