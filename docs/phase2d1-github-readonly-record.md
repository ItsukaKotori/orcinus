# Phase 2 子项目 D.1：GitHub 连接与只读基础 收尾记录

- 日期：2026-10-08（实施）～ 2026-10-08（终态自动门禁）
- 分支：`main`（spec 基线 `466ef067`；计划 `17e69922`；特性提交 `0fc2ca31`…`65f7ab3a`）
- 规格：`docs/superpowers/specs/2026-10-08-phase2d1-github-readonly-design.md`
- 计划：`docs/superpowers/plans/2026-10-08-phase2d1-github-readonly.md`（9 任务；Task 9 勾选全部复选框）
- 行为参照：`/Users/itsuka/CodeSpace/orca`（Electron 只读 oracle；`orca:` 前缀均为其内路径）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + 本记录）
- 手工验收状态：**待用户复核（2026-10-08 自动化门禁已全绿；5 项手工清单见 §4）**

## 1. 范围与验收

| 面 | 内容 | 主提交 |
|---|---|---|
| Rust 执行器 | `gh_exec`（PATH 解析 + 附加目录 + 成功缓存 / 超时对进程组 SIGKILL / maxBuffer 10MiB / 全局并发闸 4 / `GH_PROMPT_DISABLED=1`）、`gh_env_probe`、`AppState.gh_gate` + `gh_path_cache`、bindings 重生成 | 0fc2ca31 |
| remote URL | `git_remote_urls`（`git remote -v` fetch URL，按名字首次出现去重；复用 ade-git runner + 路径守卫） | 60a879b4 |
| TS 客户端 | `gh-exec-client`（只读重试 250ms/1s；429 带 `Retry-After` 不重试）、`auth-diagnose`（多 host/端口/env/keyring/scopes，永不抛）、`gh-error-classification`（固定顺序 `rate_limited → repo_unavailable → server_error/network → permission → gh_unavailable → auth → unknown`） | e5eabaae |
| 仓库身份 | `repo-identity`（候选 upstream→origin、head=origin；GHES 鉴权门；正缓存 30s / 负缓存 5min；auth inventory 60s 缓存） | 3d53bb09 |
| PR 阶梯 | `pr-for-branch`（linked `gh pr view` → REST exact → REST head → `gh pr list` 兜底 → `fallbackPRNumber`；hydrate；merged-implicit 隐藏；`PRInfo\|null` + `PRRefreshOutcome`；`conflictSummary` 经可选注入依赖） | 072cd5ba + 78042fa1 |
| checks | `pr-checks`（GraphQL → REST（check-runs+status+suites）→ `gh pr checks`；`no checks reported` → `[]`）、`pr-check-details`（check-run + annotations + jobs；25s 死线；无 `logTail`） | 421ad1bb |
| 速率 / hosted review / 就绪 | `rate-limit`（30s 缓存 + single-flight + 失败缓存，`force` 绕过）、`hosted-review`（found/active-negative 60s、negative 15min；merged head-sensitive）、`preflight-gh`（60s 缓存探针） | 4520d8a2 |
| 桥接接线 | `real/gh.ts`（8 只读方法 + `onPRRefreshEvent` no-op）、`real/hosted-review.ts`（`forBranch`）、`real/preflight.ts` gh 探针、`create-api.ts` RealDomains + parity | 65f7ab3a |

验收口径（spec §1/§6）：真实 gh 下速率预算面板有值、仓库 avatar 解析、有 PR 的 worktree 显示 pill/checks、Landing/Onboarding 就绪正确、无 PR 分支安静；自动门禁三组全绿。终态变更规模：**40 个文件，+7277 / −13**（27 个新增：计划 1 + Rust 2 + TS 24）。

## 2. 提交清单

`git log --oneline 466ef067..HEAD`（spec 提交 `466ef067` 为区间下界，不列入；本记录由 Task 9 提交追加，SHA 见提交后 `git log`）：

| 提交 | 内容 |
|---|---|
| 17e69922 | docs: Phase 2D.1 GitHub 只读基础实施计划（9 任务，TDD） |
| 0fc2ca31 | feat(bridge): gh 执行器（PATH 解析/超时进程组杀/并发闸）与 env 探针 |
| 60a879b4 | feat(bridge): git_remote_urls 命令（身份解析输入） |
| e5eabaae | feat(renderer): gh 执行客户端、auth 诊断与 PR 错误分类 |
| 3d53bb09 | feat(renderer): GitHub 仓库身份解析与缓存（GHES 鉴权门） |
| 072cd5ba | feat(renderer): PR-for-branch 查询阶梯与 outcome 组装 |
| 78042fa1 | fix(renderer): headRepo 未知时分支查询对齐参照顺序（pr list 优先） |
| 421ad1bb | feat(renderer): PR checks 列表三阶降级与详情（无日志尾） |
| 4520d8a2 | feat(renderer): 速率快照、hosted review forBranch 与 gh 就绪探针 |
| 65f7ab3a | feat(bridge): gh/hostedReview 真实域接线与 preflight gh 探针 |
| 9cf5e89d | docs: Phase 2D.1 实施记录与门禁证据（GitHub 只读基础）——Task 9，含本文件与计划勾选 |
| ba435a07 | fix(renderer): 终审修复——merged fallback、诊断永不抛、GHES 边界与超时钳制（终审 5 项；§5.3 已更新） |

## 3. 门禁证据（自动化）

三组门禁在终态 HEAD（`65f7ab3a`，分支 `main`，`git status` 干净）顺序执行：

| 门禁 | 命令与工作目录 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace`（`src-tauri/`） | **exit 0**；41 个 suite（含 doc-tests）累计 **694 passed / 0 failed / 0 ignored**；新增 `tests/gh_exec.rs` **9 passed**（含超时进程组杀 1.89s）、`tests/git_commands.rs` **19 passed**（含 2 条 remote_urls）；`specta_export::tests::bindings_are_fresh` 与 `export_lists_every_command` 均 ok |
| 类型与构建 | `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（仓库根） | **exit 0**；`tsc --noEmit` 零错误；`✓ built in 3.15s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；仅有 7 条信息性 `(!)` 警告（6 条既有动态/静态双导入提示 + 1 条既有 >500kB chunk-size 警告），无本阶段新增文件 |
| TS 全量 | `pnpm test`（仓库根） | **exit 0，全绿**：`Test Files 3871 passed / 0 failed / 8 skipped (3879)`；`Tests 34471 passed / 0 failed / 122 skipped (34593)`；Duration **458.42s**。**本轮无性能抖动失败**（无需隔离复跑） |

过程说明：Task 8 提交前已跑过一次全量 `pnpm test`（3871 文件 / 34471 测试 / 0 失败），Task 9 在终态树上复跑结果一致，即终态证据与过程中证据无差异。日志留存于本机临时目录（`cargo-test-phase2d1.log` / `pnpm-test-phase2d1.log` / `build-web-phase2d1.log`）。

终审修复波次（`ba435a07`）后的最终树复跑：`pnpm test` **exit 0**（`Test Files 3871 passed / 0 failed / 8 skipped`；`Tests 34483 passed / 0 failed`，较上表 +12 条新增回归测试）；`cargo test -p ade-bridge --test gh_exec` 10/10（+1 超时钳制）；`pnpm typecheck` exit 0。终审结论：4 项 Important（merged fallback 隐藏、诊断可抛、GHES 查询边界、upstream 鉴权门）+ 1 项 Minor 加固（超时钳制）全部修复并经 scoped 复审确认；GHES 边界见 §5.3。

## 4. 手工验收清单（待用户复核）

前置：`pnpm dev` 或打包产物均可；需本机已安装并登录 `gh`（`gh auth status` 有 active 账号）方可完整覆盖 1–3 项。

1. Settings → Git & Source Control：速率预算面板显示 core/search/graphql 数值 —— **待用户复核**
2. Settings → repository → GitHub avatar：解析出 slug 并可刷新头像 —— **待用户复核**
3. 有开放 PR 的 worktree：卡片显示 PR pill；Checks 面板显示 checks；详情可展开（无日志尾）—— **待用户复核**
4. Landing/Onboarding：gh 就绪状态正确（已装/已登录 → ready）—— **待用户复核**
5. 无 PR 分支：无 pill、无报错 toast —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

### 5.1 spec §7 十条（逐条备案）

1. **无后台协调器**：`enqueuePRRefresh` 维持 false、无刷新事件；PR 状态更新依赖渲染层现有轮询/手动刷新与 `refreshPRNow`。后续切片（2D.2+）按需补。
2. **无速率熔断**：命中限制时分类报错不预阻断；高频轮询下可能重复触发限流文案。
3. **SSH 别名不展开**：`github-work:` 等别名 remote 视为非 GitHub；负结果不缓存（避免把 indeterminate 长缓存），每次重新解析。
4. **GHES 边界（终审定稿）**：host/port 精确匹配 auth inventory；端口歧义（同 host 多端点）→ 视为未鉴权。GHES 仅身份解析（`repoSlug`/`repoUpstream`）；PR/checks/review 查询在 2D.1 不支持，非默认 host 返回空/null，绝不按 github.com 查询；host 线程化留待后续切片。
5. **无 check 日志尾**：详情含 jobs/steps 但无 `logTail`；失败详情的信息密度低于参照版。
6. **PATH 探测限常见目录**：不做登录 shell 探测（`zsh -lic`）；非常规安装位置会报 `gh_unavailable`。
7. **缓存位置**：身份/auth inventory/速率快照缓存在渲染层模块作用域（单窗口有效）；多窗口需上移。
8. **远端 runtime 路径未接**：`github.*` RPC 无本地 host；paired 环境仍走 web/mock 回退。
9. **`gh.viewer`/工作项/变更操作**维持现状；`refreshPRNow` 与 `prForBranch` 同时接真后，渲染层 truthiness 探测的首选臂行为正常。
10. **`gh_exec` 为通用执行器**：Rust 层不限制 argv（只读范围由 TS 调用面约束，与 `pty_spawn` 的既有能力面一致）；后续如需硬隔离可在命令层加只读子命令白名单。

### 5.2 实现中发现的新偏差（按任务分组）

1. **T1（Rust 执行器）**：并发闸为近似 FIFO（`Condvar` 唤醒顺序不保证）；子进程退出后的管道排空阶段不设上限（若后代进程持有 stdout，可能长期占住 permit）；spawn 失败统一折叠为 `gh: command not found (spawn failed: …)` 文案；`#[cfg(not(unix))]` 回退分支在 POSIX-only 令下为死代码。
2. **T2（`git_remote_urls`）**：命令参数名为 `worktreePath`（spec 文本写作 repoPath，实际对齐既有 `GitWorktreeArgs.worktree_path`）；git 非零退出降级为 `Ok([])`（不抛）。
3. **T3（gh 执行客户端与分类）**：非瞬态 executor 异常不包装直接透传（分类器按 `Exception` 读取 message）；`Retry-After` 存在时永不重试。
4. **T4（仓库身份）**：GHES 测试 fixture 用 https（SCP + 端口为非法语法）；缓存住在 resolver 闭包内（模块内单窗口作用域）；一个与 origin 不同的 GHES upstream 原可在未鉴权情况下逃逸给调用方，终审已修复（`resolveCandidates` 候选门 + `getRepoUpstream` 上游门，未鉴权 → null，见 §5.3）；auth inventory 按 host 缓存。
5. **T5（PR-for-branch）**：candidates 为空的裸 `gh pr view` 路径被裁掉（无 repo cwd；全部调用显式携带 repo）；所有 gh 调用使用 `{}` options（无 cwd/host 固定）；`conflictSummary` 仅经可选注入依赖产出，默认省略（spec §3.3 裁剪）；headRepo 未知阶梯在 fix 轮 1（`78042fa1`）对齐 oracle（`gh pr list` 优先）；`isNoPullRequestError` 为死代码；hydrate 失败多花一次 REST 调用。**特别备案**：2D.1 仅实现 single-PR 阶梯，参照版的 stack / merge-queue 分支整体裁剪（随 §7.9 工作项面留待后续切片）。
6. **T6（checks）**：`identity` 依赖被接受但未使用；25s 竞速后落败方的工作仍在后台继续（`gh_exec` 无 `AbortSignal`）；JS 定时器与 Rust 超时之间存在理论竞态。
7. **T7（速率 / hosted review / preflight）**：速率探针未固定 host（`gh_exec` 无 host 参数，正是 spec §3.1 signature 所限）；hosted-review 缓存仅 TTL，无容量上限/single-flight/backoff/stale-on-error；未鉴权 GHES remote 原可经 `resolveCandidates` 触达默认 host 的 gh 查询（鉴权门只在 `getRepoSlug`/`getRepoUpstream`）——同样适用于 Task 5 的查询路径，终审已按 §5.3 的 GHES 边界全面拦截（未鉴权与已鉴权 GHES 均不再触达默认 host 查询）。
8. **T8（桥接接线）**：终审已修复（见 §5.3）：`diagnoseAuth` 永不抛（env 探针包裹；仅 spawn-class 错误置 `ghAvailable:false`，超时/IPC 失败保持 true）；`refreshPRNow` 从 `candidate.fallbackPRSource` 推导 `acceptMergedFallbackPR`（对齐 web 路径；merged fallback PR 不再被隐藏）；`prChecks`/`prCheckDetails` 对非默认 host 返回 `[]`/`null`，但仍会传播身份解析（`git_remote_urls`）拒绝。

### 5.3 终审（whole-branch review）修复备案（2026-10-08）

| # | 级别 | 修复 |
|---|---|---|
| F1 | Important | `refreshPRNow` 按 `linkedPRNumber == null && fallbackPRNumber != null && fallbackPRSource != null` 推导 `acceptMergedFallbackPR`（对齐 `web-github-api.ts` 既有推导）；merged fallback PR 在原生路径不再被隐藏。 |
| F2 | Important | `diagnoseAuth` 包裹 `gh_env_probe`（失败 → `envTokenInProcess:null`）；`gh auth status` 失败仅 spawn-class（`isGhMissingError`）置 `ghAvailable:false`，超时/IPC 失败保持 `true`（永不抛）。 |
| F3 | Important | GHES 边界收紧：`resolveCandidates` 对非默认 host 候选套用 auth 门（未鉴权丢弃）；`pr-for-branch` 跳过非默认 host 候选与 headRepo；`hosted-review` 首候选非默认 host → null；桥接 `prChecks`/`prCheckDetails` 非默认 host → `[]`/`null`；GHES 绝不按 github.com 查询。 |
| F4 | Important | `getRepoUpstream` 的 distinct upstream 在返回前套用 `ensureHostAuthenticated`（非默认 host 未鉴权 → null）。 |
| F5 | Minor | `read_timeout_ms` 钳制到 `[1, 600_000]` ms，防止 `Instant::now() + Duration` 溢出 panic。 |

## 6. 已知边界与后续

- **2D.2 创建 PR 全链路**：变更操作（创建 PR/评论/评审/merge/rerun/标题/状态/auto-merge）与工作项/issue/projects/labels/assignees 均未接；`gh.viewer`、`prFileContents`、`checkOrcaStarred`/`starOrca`、`mobileDiffReview` 维持现状。
- **后台刷新协调器**：`enqueuePRRefresh` 仍 false、无事件发布/队列/节流；`onPRRefreshEvent` 为 no-op 退订。
- **check 日志尾**：`logTail` 未接，失败详情信息密度低于参照版。
- **SSH 别名展开**：`ssh -G` 解析未接，别名 remote 视为非 GitHub。
- **Windows/WSL**：仅 macOS/POSIX 路径；非 unix 分支为死代码（§5.2.1）。
- **速率熔断器**：未接；限流仅分类文案，无预阻断/退避。
- **hosted-review 缓存加固**：补容量上限/single-flight/backoff/stale-on-error（当前 TTL-only，§5.2.7）。
- **回滚策略**：将 `create-api.ts` 的 `gh`/`hostedReview` RealDomains 接线还原为 mock 即可退回 Phase-0 语义；Rust 命令、TS 模块与 bindings 保留无副作用。
