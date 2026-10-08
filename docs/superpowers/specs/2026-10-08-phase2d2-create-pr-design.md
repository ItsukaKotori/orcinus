# Phase 2 子项目 D.2：创建 PR 全链路 设计规格

- 日期：2026-10-08
- 状态：brainstorming 输出（范围 = GitHub.com eligibility + push + create、不做 stacked/fetch/pull/fast-forward、架构 = 白名单只读 `git_read` + TS 编排——用户「均按推荐」指令下的修订版，已批准；本规格为实现依据）
- 基线：`main@7d2ac0f4`
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§8 Phase 2「GitHub 等 Provider」）；2D.1 规格与记录（只读面，已交付）
- 前置事实（2026-10-08 本会话双探索代理）：
  - 创建消费面完整且已挂载：store slice `getHostedReviewCreationEligibility/createHostedReview/createStackedHostedReview`（`store/slices/hosted-review.ts:69-149`）被 Source Control 作曲家（`components/right-sidebar/source-control/review/use-hosted-review-creation.ts`）、Checks 面板先 push 再 create（`checks-panel/use-checks-panel-create-review.tsx`）、一键意图流（`use-create-pr-intent-run.ts`）消费；桥接 `hostedReview.getCreationEligibility/create/createStacked` 仍是 fallback reject（`real/hosted-review.ts:9-27`，parity `missing` 锁定）。
  - **push 完全缺失**：Rust git 域 17 命令无 push/fetch/pull；`real/git.ts` 注释明示 push 等维持 fallback；渲染层 `pushBranch` → `window.api.git.push`（`store/slices/editor/actions/git-remote-push-pull.ts:20-70`）直接 reject。`git_upstream_status` 已接真（`git.rs:525`）。
  - 共享解析助手已移植但无 host 消费：`shared/git-push-target-resolution.ts`（`resolveConfiguredGitPushTarget`）、`shared/git-configured-branch-target.ts`、`shared/git-push-target-validation.ts`（`assertGitPushTargetShape`）、`shared/git-effective-upstream.ts`。
  - 参照实现：eligibility 决策序 `orca:src/main/source-control/hosted-review-creation.ts:161-238`；base 探测 `hosted-review-creation-git-state.ts:185-249` 与 `orca:src/main/git/repo-default-base-ref.ts:21-26,95-115`；auth 探测 `hosted-review-creation-provider.ts:22-142`；create argv `orca:src/main/github/client/create/create-github-pull-request.ts:88-105`；模板 6 路径与回退 `pull-request-template.ts:11-83`；错误分类 `create-pr-error-classification.ts:3-74`；blockers→结果映射文案 `hosted-review-creation-blocking.ts:9-102`；push argv/目标解析 `orca:src/main/git/remote.ts:22-61` + `shared/git-push-target-resolution.ts:111-140`；渲染层错误归一化端口已有（`src/renderer/src/lib/source-control-remote-error.ts`）。
  - 2D.1 已交付可复用：`gh_exec`/`gh_env_probe`/`git_remote_urls`、`createGhExecClient`、`parseAuthStatus/computeAuthDiagnostic`、身份解析、`createHostedReviewClient.forBranch`（含 TTL 缓存）、preflight gh 探针。

## 1. 背景与目标

2D.1 点亮了 GitHub 只读面。本子项目（2D.2）打通「推送分支 → 创建 PR」闭环，使 Source Control 作曲家与 Checks 面板的先 push 再 create 两条路径在 Tauri 真实可用：

- **git 读取面**：白名单只读 `git_read`（复用已移植的共享解析助手）
- **push**：`git_push`（目标解析在 TS，命令只执行）
- **创建**：`hostedReview.getCreationEligibility/create` 接真（GitHub.com），含模板、草稿、already_exists 回退与错误分类
- **创建后**：失效 2D.1 review 缓存，渲染层既有 linkedPR 持久化与强制刷新原样工作

**验收（自动 + 手工）**：真实 gh + 有远端仓库下，新分支 push → 作曲家创建 PR（普通/草稿/模板）成功并在 GitHub 可见；已有 PR 的分支显示 `existing_review` 并给链接；dirty/no_upstream/needs_push 等 blockers 正确显示并可操作。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| 只读 git | `ade-bridge` 新命令 `git_read` | 白名单首参 + 路径守卫（§3.1） |
| push | `ade-bridge` 新命令 `git_push` | argv 执行；目标解析在 TS（§3.2） |
| eligibility/create | `src/renderer/src/lib/github/hosted-review-create.ts`（新） | blockers 序、base 探测、auth、create/回退/分类（§3.3） |
| git 读客户端 | `src/renderer/src/lib/github/git-read-client.ts`（新） | invoke 包装 + `runGit` 适配器 |
| `gh_exec` stdin | `ade-bridge` `gh_exec` 扩展 + bindings | `--body-file -` 语义（§3.1） |
| 桥接 | `real/git.ts`（push）、`real/hosted-review.ts`（creation） | parity/create-api 同步 |
| 缓存失效 | 2D.1 hosted-review client 增加 `invalidate` | 创建成功后 |

### 2.2 明确不做（防蔓延）

stacked 创建（`createStacked` 维持 fallback、`stackedCreationSupported:false`，UI 开关隐藏，顺延 2D.2.1）；`git_fetch/git_pull/git_fast_forward/syncFork/rebaseFromBase`（一键意图流的 sync/快进分支不可用、非快进 push 的自动 fetch 恢复不生效——披露）；非 GitHub provider；GHES 创建（非默认 host → `unsupported_provider`）；fork pushTarget 物化（`remoteUrl` 出现即报错）与 `resolvePrBase`/pushTarget 自动恢复；PR 刷新协调器；merge 生命周期（merge/auto-merge/ready/close）；PR 评论/评审线程；AI 字段生成（`generatePullRequestFields`，既有缺口）；远端 runtime 的 `hostedReview.*`/`git.push`（无本地 host）；Windows/WSL。

## 3. 架构

### 3.1 Rust（ade-bridge）

- **`git_read(args:{worktreePath: string, args: string[]}) -> {stdout, stderr, code}`**
  - 白名单首参（仅本阶段所需五项）：`config`、`rev-parse`、`symbolic-ref`、`show-ref`、`check-ref-format`；其余首参拒绝（`BridgeError`）。
  - `config` 仅允许读形式：argv 必须含 `--get|--get-all|--get-regexp|--list` 之一；拒绝 `--unset*|--add|--replace-all|--edit|--rename-section|--remove-section|--set` 及任何「非选项位置参数 ≥2」的裸写形式。
  - 执行：`run_git_in(worktreePath, args, 120s, None)`（对齐参照读命令超时）；10MiB maxBuffer 由 runner 提供；`require_authorized_worktree` 守卫；**非零退出是结果**（`{code, stdout, stderr}`），仅 spawn/超时为 Err。
- **`git_push(args:{worktreePath: string, remote?: string, refspec?: string, forceWithLease?: boolean}) -> null`**
  - 防御性校验：`remote` 非空且匹配安全段（`^[A-Za-z0-9][A-Za-z0-9._-]*$`、≤100、段非 `.`/`..`）；`refspec` 非空且不以 `-` 开头。
  - argv：`git push [--force-with-lease] --set-upstream <remote> <refspec>`；缺省 `origin HEAD`。
  - 原始 stderr 进 `BridgeError.message`（凭据清洗与用户文案由渲染层既有归一化负责）。
- **`gh_exec` 扩展**：`GhExecArgs` 增加 `stdin?: string`；spawn 时 `Stdio::piped()` 并在独立线程写入（写完关闭）；写入失败不影响读取；bindings 重生成。

### 3.2 TS git 面

- **`git-read-client.ts`**：`defaultGitReadExecutor(worktreePath)` → `(args: string[]) => Promise<{stdout, stderr, code}>`（invoke `git_read`）；`createRunGit(executor)` → 共享助手期望的 `runGit(args) => {stdout}`（非零抛 `GitReadError`）。
- **`real/git.ts` `push`**：
  1. `args.pushTarget` 存在 → `assertGitPushTargetShape`（共享）+ `check-ref-format --branch <branchName>`（经 `git_read`）→ `{remote: remoteName, refspec: 'HEAD:<branchName>'}`；`remoteUrl` 非空 → 明确报错（物化顺延）。
  2. 无 target → `resolveConfiguredGitPushTarget(runGit)`（共享，已移植；优先级 `branch.<b>.pushRemote` → `remote.pushDefault` → `branch.<b>.remote`+`branch.<b>.merge`，含 origin/pushDefault 守卫与 `branch.<b>.base` 守卫）→ null 则 `{remote:'origin', refspec:'HEAD'}`。
  3. `invokeCommand('git_push', {args:{worktreePath, remote, refspec, forceWithLease}})`。
  - `publish` 与 `push` argv 相同（参照语义）；`--set-upstream` 恒有。

### 3.3 TS 创建面（`hosted-review-create.ts`）

`createHostedReviewCreation({ client, identity, reviewLookup, gitRead, readStatus, readUpstream, fs })`：

- **`getCreationEligibility(args)`**（决策序逐字对齐参照 `hosted-review-creation.ts:161-238`，首个命中即返回）：
  1. provider：`identity.getRepoSlug(worktreePath)`（上游优先、origin 兜底；非默认 host 或 null → `unsupported_provider`，`canCreate:false`）。
  2. base：`candidateBase = args.base?.trim() || null`；`candidateBaseOnRemote`（`baseRefExistsOnRemote`）；命中 → `defaultBaseRef = candidateBase`；否则 `defaultBaseRef = getDefaultBaseRef() ?? candidateBase`。
  3. 已有 review：`reviewLookup.forBranch({...})`；抛错 → `lookupFailed`（`reviewLookupOutcome:'unavailable'`）；null → `not_found`；有值 → `found`。
  4. `stackedCreationSupported: false`（本阶段不做）。
  5. blockers（顺序）：`!branch || branch === 'HEAD'` → `detached_head`；review found → `existing_review` + `nextAction:'open_existing_review'`；provider 不支持 → `unsupported_provider`；`baseBranch` 大小写不敏感等于 branch → `default_branch`；`args.hasUncommittedChanges` → `dirty`+`'commit'`；`hasUpstream === false` → `no_upstream`+`'publish'`；`hasUpstream !== true` → `canCreate:false, blockedReason:null`；`behind > 0` → `needs_sync`+`'sync'`；auth 失败 → `auth_required`+`'authenticate'`；`ahead > 0` → `needs_push`+`'push'`；`enforceBaseOnRemote && candidateBase && !candidateBaseOnRemote` → `base_not_on_remote`；否则 `canCreate = !lookupFailed && Boolean(baseBranch)`。
  - `getDefaultBaseRef`（移植 `repo-default-base-ref.ts:21-26,95-115`）：`symbolic-ref --quiet refs/remotes/origin/HEAD`（`rev-parse --verify --quiet` 校验）→ `refs/remotes/origin/main` → `refs/remotes/origin/master` → `refs/heads/main` → `refs/heads/master`。
  - `baseRefExistsOnRemote`（移植 `hosted-review-creation-git-state.ts:185-249`）：归一化（剥 `refs/heads/`、`refs/remotes/<r>/`、`origin/`、`upstream/`）；`isSafeGitRefName` 不通过 → false；精确探测 `refs/remotes/<base>`（base 含 `/` 时）与 `refs/remotes/{origin,upstream}/<base>`（`show-ref --verify --quiet`）；后缀扫描 `show-ref -- <base>`（仅接受 `refs/remotes/<单段>/<base>`）；**未知错误 fail-open true**，exit-1 无匹配 → 确定 false。
  - auth：`gh auth status --hostname github.com`（`client.run`；非零也解析 stdout+stderr）→ `parseAuthStatus` 存在 active 账号 → 通过；spawn 类失败 → 不通过。
- **`create(args)`**：
  1. 支持/provider 校验（同上）。
  2. preflight：`currentBranch`（`rev-parse --abbrev-ref HEAD` 经 gitRead）与 `head` 不一致 → `validation`（"switch back to the selected branch…" 逐字）；dirty（`readStatus` 复用 `git_status` 的 entries）与 upstream（`readUpstream` 复用 `git_upstream_status`）复核；eligibility 复跑 `enforceBaseOnRemote:true`；`reviewLookupOutcome === 'unavailable'` → `validation`（"could not confirm whether this branch already has…" 逐字）。
  3. 正文：`useTemplate && !body?.trim()` → 模板 6 路径按序读（fs 域 `fs_read_file`；二进制跳过；全 miss → `''`），否则 `body ?? ''`。
  4. `gh pr create --repo O/R --base B --title T --body-file - [--head H] [--draft]`（60s；`stdin`=正文；`--head` 仅在提供时；`--draft` 仅在 `draft && provider 支持`）。
  5. 解析：JSON `{number,url}` 优先 → URL 正则（任意 host）→ 不可解析且 head 存在 → 回退 `gh pr list --repo O/R --head H --base B --state open --limit 2 --json number,url`（恰好 1 条 → ok）→ 无匹配 → `unknown_completion`（"may have completed…" 逐字）。
  6. 失败：分类逐字移植（auth/`already exists`/timeout→`unknown_completion`/validation 422/unknown），`already_exists` 与 `unknown_completion` 带 head 时执行同一回退查询，命中 → `already_exists` + `existingReview`。
  7. 成功：失效 2D.1 review 缓存（`reviewLookup.invalidate(repoPath)` 新增导出），返回 `{ok:true, number, url}`。
- blockers→结果映射与全部用户文案逐字移植 `hosted-review-creation-blocking.ts:9-102` 与 `create-pr-error-classification.ts:3-51`（含 `auth_required/unsupported_provider/validation/already_exists/unknown_completion/unknown`；`timeout`/`push_failed` 参照亦不产出）。

## 4. 数据流

1. 作曲家/Checks 面板轮询 `getCreationEligibility`（store 既有，30s 超时）→ 真实桥接返回 blockers。
2. `needs_push`（或生成后 rebase）时：`pushBranch` → `real/git.push`（target 解析 → `git_push`）→ `fetchUpstreamStatus`（既有命令）→ 重新 eligibility → `hostedReview.create`。
3. create 成功 → 桥接失效 review 缓存 → 渲染层既有 `handlePullRequestCreated`（`linkedPR` 持久化、force 刷新、切 Checks 面板、可选打开 URL）原样工作。
4. 错误路径：push 失败 → 渲染层既有归一化 toast，不调 create；create 失败 → 结果码 + 文案进作曲家内联提示/链接。

## 5. 错误处理与边界

| 场景 | 行为 |
|---|---|
| `git_read` 非白名单/`config` 写形式 | `BridgeError`，命令拒绝 |
| `git_read` 非零退出 | 结果透传 `{code, stdout, stderr}`，TS 分类 |
| pushTarget 形状非法 / `check-ref-format` 失败 | 不执行 push，报错（渲染层 toast） |
| pushTarget.remoteUrl 非空 | 明确报错（fork 物化顺延，披露） |
| push 非快进/认证/网络/钩子失败 | 原始 stderr → 渲染层既有归一化文案；自动 fetch 恢复不生效（fetch 未实现，披露） |
| eligibility review 查询失败 | `reviewLookupOutcome:'unavailable'`，绝不 `canCreate:true` |
| create 前 branch 被切换 | `validation`（参照文案） |
| create 输出不可解析且无回退命中 | `unknown_completion`（提示可能已创建） |
| 已有 PR | `already_exists` + `existingReview`（UI 给链接） |
| GHES 远端 | `unsupported_provider`（创建不支持；身份仍可解析） |
| `gh_exec` stdin 写失败 | 读取照常；结果仍按 code/stderr 分类 |

## 6. 测试与门禁

### 6.1 Rust

- `git_read`：白名单接受/拒绝矩阵（含 `config --get` 通过、`config k v` 与 `--unset` 拒绝、`check-ref-format` 通过、`fetch`/`status` 拒绝）；真实临时仓库 `rev-parse/symbolic-ref/show-ref` 冒烟；非零退出透传。
- `git_push`：本地裸仓库真实 push（新建分支 + `--set-upstream` + `HEAD:<branch>`）；`--force-with-lease` 顺序（`push --force-with-lease --set-upstream …`）；缺省 `origin HEAD`；非法 remote/refspec 拒绝。
- `gh_exec` stdin：fake gh `cat` 回显 stdin；stdin 缺省不注入。
- bindings 新鲜度 + 命令清单（`git_read`、`git_push`）。

### 6.2 TS

- `git-read-client`：非零抛 `GitReadError`；`runGit` 适配器供共享助手。
- `hosted-review-create`：blockers 全表（12 序）逐条；base 归一化/精确/后缀扫描/fail-open；auth 通过/失败/spawn 失败；create argv（含 `--draft`/`--head`/`--body-file -` + stdin）；JSON/URL/回退/`unknown_completion`；分类表逐条；模板 6 路径与二进制跳过；`unavailable` 拒绝；成功失效缓存。
- `real/git.ts` push 路由：pushTarget 校验 + `check-ref-format` + 配置解析矩阵（pushRemote/pushDefault/remote+merge/守卫/兜底 origin HEAD）→ `git_push` argv；`real/hosted-review.ts` creation 路由；parity/create-api 更新（creation 从 missing 转 explicit）。
- 既有 store/组件测试（`hosted-review.test.ts`、`use-checks-panel-create-review.test.tsx`、`source-control-create-pr-intent-flow.test.ts`、`editor-remote-branch-actions.test.ts`）保持全绿；`editor-remote-branch-actions.test.ts` 的 push 断言与新路由一致。

### 6.3 手工验收

1. 新分支（未推送）→ 作曲家显示 `needs_push`/Publish → 推送成功 → eligibility 转可创建 → 创建 PR 成功并在 GitHub 可见；
2. 草稿勾选 → PR 为 Draft；
3. `useTemplate` 勾选且正文空 → 正文来自仓库模板；
4. 已有 PR 的分支 → `existing_review` 显示并可打开链接；
5. dirty 工作区 → `dirty` blocker（nextAction commit）；主分支 → `default_branch`；
6. 无 gh 登录 → `auth_required`。

### 6.4 门禁

`cargo test --workspace`、`pnpm typecheck && pnpm build:web`、`pnpm test`。

## 7. 风险与偏差备案

1. **无 fetch/pull/fast-forward**：一键意图流的 `needs_sync`/快进分支报错；非快进 push 的自动 fetch 恢复不生效（推送错误本身仍正确显示）。
2. **无 stacked**：`createStacked` 维持 fallback、`stackedCreationSupported:false`（UI 开关隐藏）；顺延 2D.2.1。
3. **GHES 创建不支持**：非默认 host → `unsupported_provider`（`gh_exec` 无 host 参数，同 2D.1 边界）；身份解析仍可用。
4. **fork pushTarget 未物化**：`pushTarget.remoteUrl` 出现即报错；`resolvePrBase` 未接，故该路径当前无生产者。
5. **`git_read` 白名单**：新只读子命令需求出现时需扩展白名单（有意的能力面收敛）。
6. **读命令 120s 超时**：对齐参照读命令；本阶段无网络类变更命令。
7. **`publish` 与 `push` 同 argv**：参照语义（`--set-upstream` 恒有），沿用。
8. **`gh_exec` stdin**：能力面微扩（向子进程 stdin 写字符串）；仅创建路径使用。
9. **共享助手消费面**：`resolveConfiguredGitPushTarget`/`assertGitPushTargetShape` 本阶段获得首个 host 消费；`git-fork-sync`/`git-effective-upstream` 等仍无消费方，不清理（披露）。
