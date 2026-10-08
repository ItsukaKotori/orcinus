# Phase 2 子项目 D.1：GitHub 连接与只读基础 设计规格

- 日期：2026-10-08
- 状态：brainstorming 输出（范围 = C 完整只读面、方案 = TS 编排 + Rust 薄执行器、平台 = macOS 优先、后台协调器不做——均已批准；本规格为实现依据）
- 基线：`main@d4780b4e`
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§8 Phase 2「GitHub 等 Provider」）
- 前置事实（2026-10-08 本会话双探索代理）：
  - 渲染层消费面完整且已挂载：53 个非测试文件调用 `window.api.gh.*` / `hostedReview.*`；桥接侧 `gh`/`hostedReview` 仍是 Phase-0 mock（`create-api.ts:108,111`），除 4 个 benign 方法外全部 reject（`bridge/mock/gh-api.ts`），`hostedReview.forBranch` 返回 null。
  - 参照实现全走 `gh` CLI（无 Octokit）：`orca:src/main/git/command-runner/gh-exec-file.ts:101` 统一 spawn（并发闸 4、重试 250ms/1s、幂等判定、`GH_PROMPT_DISABLED=1`、30s 默认超时、10MiB maxBuffer、进程树杀）；auth 解析 `orca:src/main/github/auth-diagnose.ts:31,93`；身份解析 `github-remote-identity-parsing.ts` + `github-repository-identity.ts:123`（30s/5min 缓存）+ `github-owner-repo-selection.ts:9`（upstream→origin，head=origin）；PR 阶梯 `client/lookup/branch-lookup-resolution.ts:31`；checks `client/check/get-pr-checks.ts:123`（GraphQL→REST→`gh pr checks`）与 `get-pr-check-details.ts:17`（25s 死线）；错误分类 `pr-refresh-error-classification.ts:20`（固定顺序）。
  - 端口已有可复用 TS 助手：`shared/git-remote-identity.ts`（`deriveGitRemoteIdentity`）、`shared/github/repository-identity-key.ts`、`shared/github/pull-request-for-branch-outcome.ts`、`shared/github/api-availability.ts`、`shared/github/auth-types.ts`、`shared/github/check-types.ts`、`shared/github/rate-limit-types.ts`、`shared/hosted-review.ts`。
  - Rust 无 gh runner、无 remote URL 命令（`ade-bridge/src/commands/git.rs` 17 个命令均不含 remote）；`repos_add` 不派生 `repo.upstream`/`gitRemoteIdentity`。
  - 未知项：`pull-request-execution.ts:111` 以 truthiness 探测 `refreshPRNow`，proxy 伪造的函数恒真，故 mock 下 `prForBranch` 回退臂不可达；本子项目两者都接真。

## 1. 背景与目标

Phase 2C 完成后，Phase 2 剩「GitHub 等 Provider」。参照实现约 30k 行且渲染层 UI 全部就绪，故拆分为 2D.1–2D.4 依次推进。本子项目（2D.1）接真**只读面**，点亮现有 UI 的 PR 状态/检查/身份/预算/就绪：

- **gh 执行器**（Rust 薄层）：PATH 解析、超时 + 进程组杀、并发闸、提示禁用
- **连接与身份**：`gh auth status` 容错解析 + 仓库 owner/repo/host 解析与缓存
- **PR 只读**：当前分支 PR 查询（legacy `prForBranch` + `refreshPRNow` outcome）、`hostedReview.forBranch`（GitHub）
- **checks 只读**：列表三阶降级 + 详情（无日志尾）
- **预算与就绪**：`gh.rateLimit` 快照、`preflight.check` gh 布尔

**验收（自动 + 手工）**：真实 gh 下 Settings 速率预算有值；仓库图标解析 avatar；有 PR 的 worktree 显示 PR pill/checks；Landing/Onboarding gh 就绪正确。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| gh 执行器 | `ade-bridge` 新命令 `gh_exec` | spawn/PATH/超时进程组杀/并发闸/maxBuffer（§3.1） |
| 环境探针 | `ade-bridge` 新命令 `gh_env_probe` | `envTokenInProcess` 语义 |
| remote URL | `ade-bridge` 新命令 `git_remote_urls` | 身份解析输入（§3.1） |
| gh 域 | `src/bridge/real/gh.ts`（新） | 只读方法映射（§3.2） |
| hosted review | `src/bridge/real/hosted-review.ts`（新） | 仅 `forBranch` GitHub 路径 |
| preflight | `src/bridge/real/preflight.ts`（改） | gh `{installed,authenticated}` |
| TS 编排 | `src/renderer/src/lib/github/*`（新，纯函数） | 解析/缓存/阶梯/分类（§3.3） |
| 接线 | `create-api.ts` RealDomains + parity | gh、hostedReview 入 RealDomains |

### 2.2 明确不做（防蔓延）

变更操作（创建 PR/评论/评审/merge/rerun/标题/状态/auto-merge）；工作项/issue/projects/labels/assignees；后台协调器（`enqueuePRRefresh` 维持 mock false、`onPRRefreshEvent` 空订阅、无队列/节流/事件发布）；远端 runtime 的 `github.*`（无本地 runtime host）；Windows/WSL 路由；SSH 主机别名展开（`ssh -G`）；check 失败 job 日志尾（`logTail`）；速率熔断器；`gh.viewer`、`prFileContents`、`checkOrcaStarred`/`starOrca`；GitLab/Bitbucket/Jira/Linear；`mobileDiffReview` 无关。

## 3. 架构

### 3.1 Rust 执行器（ade-bridge）

- **`gh_exec(args: string[], options: {cwd?: string, timeoutMs?: u32, maxBuffer?: u32}) -> {stdout, stderr, code}`**
  - `gh` 解析：继承进程 PATH；未命中时按序补 `/opt/homebrew/bin`、`/usr/local/bin`、`~/.local/bin`、`~/bin` 再解析；结果缓存于 `AppState`；仍失败返回 spawn 错误（TS 分类 `gh_unavailable`）。
  - spawn 环境：继承进程环境 + `GH_PROMPT_DISABLED=1`（已设置则不覆盖）。
  - 超时默认 30s，`ORCA_GH_EXEC_TIMEOUT_MS` 覆盖；超时对**进程组**发 SIGKILL（POSIX `setsid`/`process_group` 等价物），确保 shim 孙进程不残留。
  - maxBuffer 默认 10MiB（超限报错不截断）；stdout/stderr UTF-8。
  - 并发闸：`AppState` 全局 `Semaphore(4)`，FIFO。
  - 非零退出**不是**命令错误——返回 `{code, stdout, stderr}` 由 TS 分类；仅 spawn/超时/超限为 Err。
- **`gh_env_probe() -> {token: 'GH_TOKEN'|'GITHUB_TOKEN'|null}`**：读进程环境（`GH_TOKEN` 优先），与 gh 自报来源相互独立。
- **`git_remote_urls(repoPath) -> [{name, url}]`**：`git remote -v` 的 fetch URL；复用 ade-git runner 与既有路径守卫。

### 3.2 桥接接线（TS）

- `src/bridge/real/gh.ts` 新域（`GithubPullRequestApi` 只读子集）：`diagnoseAuth`、`repoSlug`、`repoUpstream`、`prForBranch`、`refreshPRNow`、`prChecks`、`prCheckDetails`、`rateLimit`；其余方法不提供（Proxy fallback 维持 unimplemented 语义）；`onPRRefreshEvent` 返回 no-op 退订。
- `src/bridge/real/hosted-review.ts`：`forBranch` → 身份 + PR 查询 → `HostedReviewInfo`；非 GitHub → null；其余方法维持 fallback。
- `src/bridge/real/preflight.ts`：新增 gh 探针（60s 缓存），git 逻辑不变。
- `create-api.ts`：`gh`、`hostedReview` 加入 `RealDomains`；parity/`create-api.test.ts` 同步更新（原「unported namespaces stay mock」断言调整）。

### 3.3 TS 编排模块（`src/renderer/src/lib/github/`，纯函数 + 可注入 executor）

- `gh-exec-client.ts`：`invoke('gh_exec')` 包装；只读重试（250ms/1s，瞬态 5xx/ECONNRESET/超时；429 带 Retry-After 不重试）；stderr 提取。
- `auth-diagnose.ts`：移植 `parseAuthStatus` 容错正则（多 host、`host:port`、登录行回退、env/keyring、scopes）+ 计算字段：`requiredScopes=['project','read:org','repo']`、host 过滤（trim+lowercase）、`missingScopes`、`hasKeyringFallback`（同 host 非 active keyring 账号）、`requiredHostAuthenticated`、`envTokenInProcess`（`gh_env_probe`）。永不抛。
- `repo-identity.ts`：`git_remote_urls` → `deriveGitRemoteIdentity`/`githubRepoIdentityKey` 归一化 → 候选 upstream→origin、head=origin；GHES host 鉴权门（auth inventory 60s 缓存；未鉴权 → null，**绝不**把未鉴权 host 传给 gh）；缓存 30s 正/5min 负（key = repoPath+remote 组合；`indeterminate` 不写负缓存）。
- `pr-for-branch.ts`：阶梯 = linkedPR number（`gh pr view N --json …`，失败降级 `gh api repos/O/R/pulls/N`）→ 分支（REST `pulls?head=OWNER%3Abranch&state=all&per_page=1`，失败降级 `gh pr list --repo O/R --head branch --state all --limit 1 --json …`）→ `fallbackPRNumber`；命中后 hydrate（`gh pr view N --json PR_LOOKUP_JSON_FIELDS`）；派生 `checksStatus`（rollup）、mergeable/conflict 摘要；输出 `PRInfo|null`（legacy）与 `PRRefreshOutcome{found|no-pr|upstream-error}`（`refreshPRNow`）。
- `gh-error-classification.ts`：移植参照固定顺序 `rate_limited → repo_unavailable → server_error/network（复用 api-availability）→ permission → gh_unavailable → auth → unknown` + `safePRRefreshErrorMessage` 稳定文案。
- `pr-checks.ts`：GraphQL rollup（`gh api graphql --cache 60s`，查询移植）→ 失败且有 headSha 时 REST 兜底（check-runs + status + check-suites，`action_required` suite 合成条目）→ `gh pr checks N --json name,state,link` 兜底（stderr `no checks reported` → `[]`）；映射 `PRCheckDetail`。
- `pr-check-details.ts`：`checkRunId` → `gh api repos/O/R/check-runs/<id>` + annotations（失败非致命）+ workflow jobs（失败非致命，按 checkName 过滤）；25s 超时（`gh_exec timeoutMs`）→ 精确文案 `Timed out loading check details.`；返回 `PRCheckRunDetails|null`（无 `logTail`）。
- `rate-limit.ts`：`gh api rate_limit` → `{core,search,graphql}` 快照 + `fetchedAt`；30s 缓存，`force` 绕过；失败返回 `{ok:false,error}`。

## 4. 数据流与契约

| 能力 | 流 | 输出 |
|---|---|---|
| diagnoseAuth(host?) | gh auth status（stdout+stderr 合并）→ parse → host 过滤 → 计算 + env probe | `GhAuthDiagnostic`（gh 缺失 → `ghAvailable:false`，永不抛） |
| repoSlug / repoUpstream | git_remote_urls → 候选 → GHES 门 →（upstream 缺失/相同）`gh repo view --json isFork,parent`（10s） | `{owner,repo,host}|null` |
| prForBranch | 阶梯 → hydrate | `PRInfo|null` |
| refreshPRNow | 阶梯 → outcome 组装 | `PRRefreshOutcome` |
| prChecks | GraphQL → REST → gh pr checks | `PRCheckDetail[]` |
| prCheckDetails | check-run + annotations + jobs | `PRCheckRunDetails|null` |
| rateLimit | gh api rate_limit | `{ok:true,snapshot}|{ok:false,error}` |
| hostedReview.forBranch | 身份 + PR 核心 | `HostedReviewInfo|null`（provider github） |
| preflight.check | gh 解析 + auth 活跃账号（60s 缓存） | `gh:{installed,authenticated}` |

契约约束：可选字段**省略**而非 null（对齐 structured-clone 语义）；`PRRefreshOutcome` 与 `PRCheckDetail` 形状以 `src/shared/github/*` 现有类型为准；错误分类文案稳定、不泄露 stderr 原文以外内容。

## 5. 错误处理与边界

| 场景 | 行为 |
|---|---|
| gh 不在 PATH/常见目录 | auth `ghAvailable:false`；PR 分类 `gh_unavailable`；preflight `installed:false` |
| 非零退出 | stderr 按固定顺序分类；不抛未分类异常 |
| 超时 | 进程组杀；归类 network/timeout；详情页精确文案 |
| maxBuffer 超限 | `gh_exec` Err → 分类 server/unknown（稳定文案） |
| 速率限制 | 分类 `rate_limited` + 文案；不预阻断（无熔断，披露） |
| GHES 未鉴权 | 视为非 GitHub → null；不传 host |
| 429 带 Retry-After | 不重试，直接分类返回 |
| `no checks reported` | `prChecks` → `[]` |
| annotations/jobs 失败 | 非致命，详情其余字段照常返回 |
| 并发 | Rust 闸 4；TS 重试不放大并发 |

## 6. 测试与门禁

### 6.1 Rust

- `gh_exec`：fake gh（临时 PATH）：成功/非零透传/超时**进程组杀**（脚本再 spawn 孙进程，断言全部退出）/maxBuffer/并发上限 4/`GH_PROMPT_DISABLED=1` 注入/PATH 补目录解析。
- `gh_env_probe`：设置/不设置 env 两态。
- `git_remote_urls`：真实临时仓库多 remote 集成。
- bindings 新鲜度 + 命令清单（3 个新命令）。

### 6.2 TS

- auth 解析 fixture：多 host/端口、env vs keyring、scopes 缺失、无 active、gh 输出为空、gh 缺失。
- 身份：候选顺序（upstream→origin）、host 归一化、GHES 门、缓存 TTL/负缓存/indeterminate 不缓存。
- PR 阶梯：linked 命中、REST 命中、`pr list` 兜底、fallback number、not-found、错误分类表（逐类一条）、outcome 形状。
- checks：GraphQL 成功、GraphQL 失败+REST、REST 空+`gh pr checks`、`no checks reported`、映射字段（checkRunId/workflowRunId/url）。
- 详情：check-run+annotations+jobs、annotations 失败非致命、25s 超时文案。
- rate limit：快照映射、30s 缓存、force。
- hosted review 映射；preflight gh 布尔；bridge parity（RealDomains/命令名）；mock 行为回归。
- 既有 store/组件测试（`github-pr-checks-fetch`、`hosted-review*`、`repo-slug-index`、`preflight`、`landing-preflight-issues` 等）保持全绿。

### 6.3 手工验收

1. Settings → Git & Source Control：速率预算面板显示三个 bucket 数值（真实 gh）。
2. Settings → repository → GitHub avatar：解析出 slug 并可刷新头像。
3. 选一个有开放 PR 的 worktree：卡片显示 PR pill；Source Control/Checks 面板显示 checks；详情可展开（无日志尾）。
4. Landing/Onboarding：gh 就绪状态正确（本机已装/已登录 → ready）。
5. 无 PR 分支：pill 不显示、无报错 toast。

### 6.4 门禁

`cargo test --workspace`、`pnpm typecheck && pnpm build:web`、`pnpm test`。

## 7. 风险与偏差备案

1. **无后台协调器**：`enqueuePRRefresh` 维持 false、无刷新事件；PR 状态更新依赖渲染层现有轮询/手动刷新与 `refreshPRNow`。后续切片（2D.2+）按需补。
2. **无速率熔断**：命中限制时分类报错不预阻断；高频轮询下可能重复触发限流文案。
3. **SSH 别名不展开**：`github-work:` 等别名 remote 视为非 GitHub；负结果**不缓存**（避免把 indeterminate 长缓存），每次重新解析。
4. **GHES 最小支持**：host/port 精确匹配 auth inventory；端口歧义（同 host 多端点）→ 视为未鉴权。
5. **无 check 日志尾**：详情含 jobs/steps 但无 `logTail`；失败详情的信息密度低于参照版。
6. **PATH 探测限常见目录**：不做登录 shell 探测（`zsh -lic`）；非常规安装位置会报 `gh_unavailable`。
7. **缓存位置**：身份/auth inventory/速率快照缓存在渲染层模块作用域（单窗口有效）；多窗口需上移。
8. **远端 runtime 路径未接**：`github.*` RPC 无本地 host；paired 环境仍走 web/mock 回退。
9. **`gh.viewer`/工作项/变更操作**维持现状；`refreshPRNow` 与 `prForBranch` 同时接真后，渲染层 truthiness 探测的首选臂行为正常。
10. **`gh_exec` 为通用执行器**：Rust 层不限制 argv（只读范围由 TS 调用面约束，与 `pty_spawn` 的既有能力面一致）；后续如需硬隔离可在命令层加只读子命令白名单。
