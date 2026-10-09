# Phase 2 子项目 D.2：创建 PR 全链路 收尾记录

- 日期：2026-10-08（实施）～ 2026-10-09（终态自动门禁）
- 分支：`main`（spec 基线 `43282eea`；计划 `6d736b01`；特性提交 `28f2f01d`…`6ca82212`）
- 规格：`docs/superpowers/specs/2026-10-08-phase2d2-create-pr-design.md`
- 计划：`docs/superpowers/plans/2026-10-08-phase2d2-create-pr.md`（8 任务；Task 8 勾选全部复选框）
- 行为参照：`/Users/itsuka/CodeSpace/orca`（Electron oracle；`orca:` 前缀均为其内路径）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + 本记录）
- 手工验收状态：**待用户复核（2026-10-09 自动化门禁已全绿；6 项手工清单见 §4）**

## 1. 范围与验收

| 面 | 内容 | 主提交 |
|---|---|---|
| Rust `git_read` | 白名单只读五子命令（`config`/`rev-parse`/`symbolic-ref`/`show-ref`/`check-ref-format`）；`config` 仅读形式（精确只读旗标 allowlist，拒绝 `--file/-f/--blob*`、位置参数之后的选项与符号引用写形式）；`remote` 仅裸形/`-v`/`--verbose`；`require_authorized_worktree` 守卫；非零退出为结果（`{stdout, stderr, code}`） | 28f2f01d + 7bf84b64/f7733426/da6c21bc/c5b7cd42 |
| `gh_exec` stdin | `GhExecArgs.stdin?: string`；`Stdio::piped()` + 独立线程写入后关管；缺省不注入；bindings 重生成 | b2c91be5 |
| Rust `git_push` | argv 执行 + 防御校验（安全 remote 名/refspec 不以 `-` 开头）；`push [--force-with-lease] --set-upstream <remote> <refspec>`；缺省 `origin HEAD`；非零退出原始 stderr 进 `BridgeError` | 28bded63 |
| TS git 面 | `git-read-client.ts`（`GitReadError`/`createRunGit`/`defaultGitReadExecutor`）；`real/git.ts` `push`（pushTarget 校验 + `check-ref-format`；否则 `resolveConfiguredGitPushTarget`）；配置解析矩阵测试 | 2593c3ea + c5b7cd42 + 8b78dd26 |
| eligibility | `hosted-review-create.ts` 第一段：provider/review 查询、base 探测与归一化/后缀扫描/fail-open、auth 探测、12 序 blockers | 1974369d |
| create | preflight/模板 6 路径/argv/`--body-file -` stdin/JSON-URL-回退解析/分类表/blockers 映射/成功后 `reviewLookup.invalidate`；非幂等写 `retry:false` | cdac1adc + 9dbbf744 |
| 桥接接线 | `real/hosted-review.ts` creation 装配（client/identity/reviewLookup/makeRunGit/readStatus/readUpstream/readTemplate）；parity/create-api（creation `missing`→`explicit`，仅剩 `createStacked`） | 6ca82212 |

验收口径（spec §6.3）：真实 gh + 有远端仓库下，新分支 push → 作曲家创建 PR（普通/草稿/模板）成功并在 GitHub 可见；已有 PR 的分支显示 `existing_review` + 链接；dirty/no_upstream/needs_push 等 blockers 正确显示。自动门禁三组全绿（§3）。终态变更规模：**21 个文件，+3343 / −31**（4 个新增均为 TS：`git-read-client(.test).ts`、`hosted-review-create(.test).ts`；区间内另含计划补丁 `33fa4511`）。

## 2. 提交清单

`git log --oneline 6d736b01..HEAD`（spec 提交 `43282eea` 与计划提交 `6d736b01` 为区间下界，不列入；区间内 docs 提交 `33fa4511` 为计划补丁，列入；本记录由 Task 8 提交追加，SHA 见提交后 `git log`）：

| 提交 | 内容 |
|---|---|
| 33fa4511 | docs: 2D.2 计划补 gh-exec-client stdin 透传任务 |
| 28f2f01d | feat(bridge): git_read 白名单只读命令（Task 1） |
| 7bf84b64 | fix(bridge): git_read 拒绝 symbolic-ref 写形式（T1 fix 1） |
| f7733426 | fix(bridge): git_read symbolic-ref/config 白名单收紧（写形式与越权读拒绝）（T1 fix 2） |
| da6c21bc | fix(bridge): git_read config 拒绝位置参数之后的选项（写绕过）（T1 fix 3） |
| c5b7cd42 | fix(bridge): git_read 允许 remote -v（push 目标 URL 归一化）（T1 fix 4） |
| b2c91be5 | feat(bridge): gh_exec 支持 stdin（--body-file - 语义）（Task 2） |
| 28bded63 | feat(bridge): git_push 命令（argv 执行 + 防御校验）（Task 3） |
| 2593c3ea | feat(renderer): git_read 客户端与 git.push 目标解析接线（Task 4） |
| 8b78dd26 | test(renderer): push 目标配置解析矩阵（pushRemote/pushDefault/守卫）（Task 4） |
| 1974369d | feat(renderer): PR 创建 eligibility（blockers/base/auth）（Task 5） |
| cdac1adc | feat(renderer): PR 创建执行（preflight/模板/回退/分类）（Task 6） |
| 9dbbf744 | fix(renderer): PR 创建禁用瞬态重试（非幂等写）（Task 6 fix） |
| 6ca82212 | feat(bridge): hostedReview 创建面接线（eligibility/create）（Task 7） |

## 3. 门禁证据（自动化）

三组门禁在终态 HEAD（`6ca82212`，分支 `main`，`git status` 干净）顺序执行：

| 门禁 | 命令与工作目录 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace`（`src-tauri/`） | **exit 0**；41 个 suite（含 doc-tests）累计 **707 passed / 0 failed / 0 ignored**；`tests/git_commands.rs` **29 passed**（含 git_read 白名单矩阵与 `remote -v`）、`tests/gh_exec.rs` **12 passed**（含 stdin 注入/缺省两条）；`specta_export::tests::bindings_are_fresh` 与 `export_lists_every_command` 均 ok |
| 类型与构建 | `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（仓库根） | **exit 0**；`tsc --noEmit` 零错误；`✓ 11529 modules transformed`、`✓ built in 4.53s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；7 条信息性 `(!)`（6 条既有动态/静态双导入提示 + 1 条既有 >500kB chunk-size 警告），无本阶段新增文件 |
| TS 全量 | `pnpm test`（仓库根） | **exit 0，全绿**：`Test Files 3873 passed / 0 failed / 8 skipped (3881)`；`Tests 34592 passed / 0 failed / 122 skipped (34714)`；Duration **608.92s**。**本轮无性能抖动失败**（已知 `browser-history-match.performance.test.ts` 未触发，无隔离复跑需要） |

日志留存于本机临时目录（`cargo-test-phase2d2.log` / `build-web-phase2d2.log` / `pnpm-test-phase2d2.log`）。

## 4. 手工验收清单（待用户复核）

前置：`pnpm dev` 或打包产物均可；需本机已安装并登录 `gh`（`gh auth status` 有 active 账号）方可完整覆盖 1–6 项。

1. 新分支未推送 → 显示 needs_push → 推送成功 → 创建 PR 成功（GitHub 可见）—— **待用户复核**
2. 草稿勾选 → PR 为 Draft —— **待用户复核**
3. useTemplate 且正文空 → 正文来自仓库模板 —— **待用户复核**
4. 已有 PR 分支 → existing_review + 链接 —— **待用户复核**
5. dirty → dirty blocker；主分支 → default_branch —— **待用户复核**
6. 无 gh 登录 → auth_required —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

### 5.1 spec §7 九条（逐条备案）

1. **无 fetch/pull/fast-forward**：一键意图流的 `needs_sync`/快进分支报错；非快进 push 的自动 fetch 恢复不生效（推送错误本身仍正确显示）。
2. **无 stacked**：`createStacked` 维持 fallback、`stackedCreationSupported:false`（UI 开关隐藏）；顺延 2D.2.1。
3. **GHES 创建不支持**：非默认 host → `unsupported_provider`（`gh_exec` 无 host 参数，同 2D.1 边界）；身份解析仍可用。
4. **fork pushTarget 未物化**：`pushTarget.remoteUrl` 出现即报错；`resolvePrBase` 未接，故该路径当前无生产者。
5. **`git_read` 白名单**：新只读子命令需求出现时需扩展白名单（有意的能力面收敛）。
6. **读命令 120s 超时**：对齐参照读命令；本阶段无网络类变更命令。
7. **`publish` 与 `push` 同 argv**：参照语义（`--set-upstream` 恒有），沿用。
8. **`gh_exec` stdin**：能力面微扩（向子进程 stdin 写字符串）；仅创建路径使用。
9. **共享助手消费面**：`resolveConfiguredGitPushTarget`/`assertGitPushTargetShape` 本阶段获得首个 host 消费；`git-fork-sync`/`git-effective-upstream` 等仍无消费方，不清理（披露）。

### 5.2 实现中发现的新偏差（按任务分组）

1. **T1 fix 轮 1–4（`git_read` 白名单收紧）**：`symbolic-ref` 写形式最初仅拒字面 `-d/--delete`，随后改为「仅允许精确只读旗标 allowlist」以覆盖组合/缩写形式（`7bf84b64`→`f7733426`）；`config` 拒绝 `--file/-f/--blob*`（越权读 containment）；`config` 拒绝任何位置参数之后的选项（git `STOP_AT_NON_OPTION` 写绕过，`da6c21bc`）；`remote` 仅允许裸形/`-v`/`--verbose`（Task 4 URL→name 归一化所需，`c5b7cd42`）。
2. **T1 minor**：`--get-urlmatch` 未放行（无需求）；个别 exotic 旗标顺序会被保守误拒；评审报告条数曾出现计数滑误（不影响结论）。
3. **T2（`gh_exec` stdin）**：stdin 载荷用 owned 字符串实现（计划为 `&str`，借用检查适配）；`child.stdin.take()` 为 `None` 时仍有一个冗余 clone 分支（当前不可达）；无 >64KiB 长管道测试。
4. **T3（`git_push`）**：本地测试辅助与 `Vec<u8>` 适配既有 helper；「错误上抛契约」（spawn/超时 vs 非零退出结果）未直接测试；`is_safe_remote_name` 边界测试较薄。
5. **T4（TS git 面）**：首次派发因 SSE 超时中止（无代码落地，已重新派发）；无 target 兜底显式传 `origin`+`HEAD`（Rust `None` 同义，线上调用自描述）；URL 形态的已配置 remote 起初降级，经 `remote -v` 白名单修复；`resolveConfiguredGitPushTarget` 的正向守卫路径与 URL 形态 pushRemote/pushDefault 归一化未测试。
6. **T5（eligibility）**：`stackedCreationSupported` 未物化（spec 字面 false；对 `=== true` 消费方行为等价）；`defaultBaseRef`/`head` 显式物化 null（参照逐字）；happy-path-no-candidate 未测试。
7. **T6（create）**：非幂等创建用 `retry:false`（参照 `idempotent:false`）；单次尝试路径有一次 250ms 空转 sleep；GHES create → `unsupported_provider`；正文走 stdin（非临时 `--body-file` 文件）；`enforceBaseOnRemote` 仅存在于模块局部类型。
8. **T7（桥接接线）**：已知负载下性能 flake（`browser-history-match.performance.test.ts`），隔离复跑 4/4；adapter 层模板读取未测试（`readTemplate` 注入）；`TemplateFileContent` 手工重复定义。

## 6. 已知边界与后续

- **2D.2.1 stacked 创建**：`createStacked` 维持 fallback、`stackedCreationSupported:false`（UI 开关隐藏）。
- **fetch/pull/fast-forward**：`git_fetch`/`git_pull`/`git_fast_forward`/`syncFork`/`rebaseFromBase` 未接；`needs_sync`/快进分支与 push 自动 fetch 恢复不可用。
- **fork 物化与 `resolvePrBase`**：`pushTarget.remoteUrl` 出现即报错；pushTarget 自动恢复路径无生产者。
- **非 GitHub provider / GHES 创建**：非默认 host → `unsupported_provider`；身份解析仍可用。
- **PR 刷新协调器**：`enqueuePRRefresh` 仍 false、无事件发布/队列/节流；创建成功后的刷新依赖既有手动/轮询路径（`reviewLookup.invalidate` 已接）。
- **merge 生命周期**：merge/auto-merge/ready/close 未接；PR 评论/评审线程与 AI 字段生成（`generatePullRequestFields`）仍缺。
- **回滚策略**：将 `create-api.ts` 的 hostedReview creation（`getCreationEligibility`/`create`）与 git `push` RealDomains 接线还原为 mock 即可退回 Phase-0 语义；Rust 命令、TS 模块与 bindings 保留无副作用。
