# Phase 1 债务清偿收尾记录

- 分支：`phase1-debt-payoff`（base `main@7b6606d`）；本批次全程未 push、未 merge，合并/PR/保留由用户在合并前决定
- 日期：2026-09-20（macOS darwin-arm64；Node v22.21.0，pnpm 12.4.2）
- 计划与 spec：`docs/superpowers/plans/2026-09-20-phase1-debt-payoff.md`；`docs/superpowers/specs/2026-09-14-ade-design.md`（本批次回写风险 1 与新增 §10.1）
- 证据目录（gitignored，不随仓库分发）：`.superpowers/sdd/2026-09-20-phase1-debt-payoff/`
  - 终局冷门禁：`final-cold-gate.log`（`tsc --noEmit` + `vite build`，exit 0）
  - 终局全量测试：`final-test.log`（exit 0）
  - GUI 启动冒烟：`dev-smoke.log`
  - 基线：`baseline-test.log`、`baseline-failures.txt`（173 行）

## 1. 批次与提交序（Task 0–10）

| Task | 提交 | 主题 |
|---|---|---|
| 0 | `11109b3` | 计划提交（11 步）；创建基线工具与基线日志 |
| 0 | `c6487f2` | 控制器修正计划（allowlist 路径语义、基线提取、数量预期） |
| 0 | `7be0180` | 控制器修正 Task 9（失败清单 grep 前置、173 口径） |
| 1 | `d19c411` | 移植 xterm/node-pty 补丁族与再生维护链（16 文件，+6788/−25） |
| 1 | `d31b75a` | 补丁自测纳入测试套件并对齐文档（审查修复） |
| 2 | `b178e46` | 删除 rpc-contract 死树与连带契约模块（85 文件） |
| 2 | `32c604d` | 删除 CLI 解析死链与 test-only 孤儿模块（12 文件） |
| 2 | `4fac569` | 删除 remote-server-updates 整链（11 删除 + 3 修改） |
| 3 | `053e42d` | 清理 i18n 死键与设置面残留（19 文件，−2008 行；i18n −1605 叶子） |
| 3 | `235c83c` | 统一 providerAccountScope 文案并清 `menu.showMobileButton` 键（审查修复） |
| 4 | `804fbfe` | 清理 skill-share 契约、feature-interaction 死项与 ai-vault 残留 |
| 5 | `1c73df4` | 删除 relay/remote-runtime 死簇并解耦 live 测试（67 文件） |
| 5 | `30a6956` | 清理零引用残留（unpaired 契约、孤儿表单、死键）（审查修复，Ruling 10） |
| 5 | `4b3872a` | 恢复 live 模块单测并清理残留注释（审查修复，Ruling 11） |
| 6 | `46ac118` | mock 加固（方法级 fallback、返回值拷贝、doc-preview 响亮失败） |
| 6 | `eabba79` | 补齐 worktrees mock 返回值拷贝（审查修复） |
| 7 | `d77fb6c` | 抽取 preload 类型至 shared 并清除 electron 残留（100 文件，+4207/−5109） |
| 8 | `6450109` | 固化 PTY 宿主实现要求并启用 node-pty 构建 |
| 8 | `ea2bc23` | 控制器修正 Task 8（node-pty 源码构建脚本、CPR 尾部断言） |
| 8 | `2e61aea` | 固化 node-pty 源码构建脚本（审查修复） |
| 8 | `760b3f5` | node-pty 重建脚本跨平台化并补 PTY 文档（审查修复） |
| 9 | `9f691e9` | 测试套件重基线（ratchet、fixture、ENOENT、环境簇） |
| 10 | 本提交 | 债务清偿收尾（CEF 回写、勘误、记录） |

## 2. 删减与修复统计

| 指标 | 数量 | 证据 |
|---|---|---|
| 净删除源文件 | **188**（删除事件 192，其中 4 个 live 单测按 Ruling 11 恢复） | `git diff --diff-filter=D --name-only main...9f691e9` |
| 其中生产模块 / 测试 | 149 / 39 | 同上（按 `.test.`/`.spec.` 区分） |
| 分支 diff（截至 Task 9） | 393 文件，+18031 / −27032 | `git diff --shortstat main...9f691e9` |
| 死树（主要批次） | rpc-contract 76 文件 + 连带契约；relay/remote-runtime 41 生产 + 25 测试 + 1 test-support；CLI 解析死链 + 孤儿模块 12（含 5 测试与 1 fixture）；remote-server-updates 11（9 runtime + 2 store）；`src/main` 残留 7 + 1 测试 + electron shim 1 | 各任务提交 `--stat` |
| i18n 叶子删除 | **1605**：en 293 / es 248 / fr 287 / ja 248 / ko 248 / zh 248 / en-runtime-required 33 | `053e42d` + `235c83c` |
| 追加死键删除 | 7 键 × 7 catalog（−76/+7 行） | `30a6956` |
| ko overrides 移植 | 1693 键 / 5081 行，全文件 `Orca→Orcinus` | `9f691e9` |
| 新增 ratchet / 工具 | `src/shared/electron-import-boundary.test.ts`、`src/bridge/mock/clone-mock-value.ts`、`config/scripts/regenerate-xterm-patches{,.test}.mjs`、`config/scripts/xterm-patch-text.mjs`、`config/scripts/rebuild-node-pty.mjs` | 各任务提交 |
| node-pty 构建 | 启用源码构建 `pnpm run rebuild:node`（prebuilt spawn-helper 权限 644 且不编译补丁；脚本以 node wrapper 跨平台化） | Ruling 15、`760b3f5` |
| mock 加固 | 6 个命名空间级 fallback 改方法级；23 个 clone 位点 + `listRetiredNames` 补 clone | `46ac118` + `eabba79` |

## 3. 测试基线 → 全绿

Task 10 终局命令：`rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web && pnpm test > final-test.log 2>&1`（`final-cold-gate.log` + `final-test.log`，总 exit 0）。

| 指标 | 基线（Task 0 采样 `baseline-test.log`） | 终局（Task 10 `final-test.log`） | Δ |
|---|---|---|---|
| Test Files | 41 failed / 3813 passed / 8 skipped (3862) | **3825 passed / 8 skipped (3833)** | 失败 −41；通过 +12 |
| Tests | 173 failed / 33910 passed / 123 skipped (34206) | **33929 passed / 122 skipped (34051)** | 失败 **−173**；通过 +19 |
| Unhandled Errors | 3 | **0** | −3 |
| FAIL 行 | `baseline-failures.txt` = 173 行 | `grep -E '^\s*FAIL' final-test.log` = **0 行** | −173 |
| Exit code | 1 | **0** | |

- 173 项清零归属：Task 1（xterm/IME 簇）125、Task 4（feature-interactions）1、Task 5（relay/lifecycle）6、Task 8（pty-reply-echo）4、Task 9 **37**。
- 对比命令：`grep -E '^\s*FAIL' final-test.log`（前置 grep，避免 sed 扫描 1900+ 行噪音）；失败集合为空，无新增失败。
- 冷门禁：`tsc --noEmit` 无输出（exit 0）；`vite build` `✓ built in 4.84s`；仅既有 chunk-size 警告。
- 无 quarantine、无自定义 vitest exclude、无新增 skip；8 个 skipped 文件为仓库既有（平台条件/可选 live 环境），非本批次隔离。

## 4. 保留与延迟项

| 项 | 本批次处置 | 原因 | 跟踪 |
|---|---|---|---|
| `src/preload` 运行期树（80 个文件仍 `import ... from 'electron'`） | 保留（类型已抽至 `src/shared/preload-api/`） | R39 裁定 spec §8 三目录范围为准；运行期树随 Phase 2 Tauri bridge 删除 | spec §8 / Phase 2 |
| `orca-profiles` 整链 + 多 profile 管理死动作 | 整链 live，不删 | 仍被 store/preload/web/unexpected-signout/browser partition 消费；死动作留待后续 | `docs/phase0-dead-code-inventory.md` |
| mock 的 fake-success 插件流程 | 保留（仅 `doc-preview` 改响亮失败） | 插件真实数据未接入；mock 成功语义仍是 UI 可用的前提 | Phase 2 插件后端 |
| renderer relay harness / remote-runtime live 模块与测试 | 保留 | 被 live 终端/web 代码与测试支持牵住，先重构测试支持再清 | `docs/phase1-feature-trim-record.md` 残留表 |
| `activity-terminal-portal` registry | 保留 | 生产者已随 Activity 删除，现为 inert registry；待终端 parking 重构 | 同上 |
| host-compat 字段（`Tab.viewMode`、`telemetrySource`、launch telemetry） | 保留 | 外部宿主/旧会话仍可能读写，只删生产者不删字段 | 同上 |
| `src/renderer/src/electron-webview-globals.d.ts`（7 个成员） | 保留（新增文件） | renderer 生产代码约 107 处 `Electron.WebviewTag` 等引用；批量改写被禁 | Ruling 14；Phase 2 fork-owned 类型替换 |
| perf 预算 4ms（`browser-history-match-budget.ts`） | 由 2ms 抬至 4ms 并记录重测数据 | 孤立测量 p95 0.08–0.23ms，满负载 p95 2.245ms；4ms 仍有 >17× 余量 | Task 9 minor：Phase 2 复核 |
| `matchP95Ms` 未复测 | 保持 2ms | 两轮全量未失败 | Task 9 minor |
| windows-lane 次级覆盖 | `windows-lane-tree-removal-boundary.test.ts` 已删，未替换 | 该测试针对 ade 不存在的 Windows lane 树 | Task 9 minor / 见 §5 |
| agent-hook-listener / `data-workspace-board-preserve-open` / ai-vault resume UI 等 | 保留 | 见 `docs/phase1-feature-trim-record.md` 残留表 | 同上 |

## 5. Windows 验证单列跟踪

| 项 | 状态 | 说明 | 跟踪 |
|---|---|---|---|
| xterm/node-pty 补丁在 Windows 构建 | **未验证** | 补丁为源码级；`pnpm run rebuild:node` 已跨平台化（node wrapper + win32 `shell:true`），但未在 Windows 实跑 | Phase 2 / Windows lane |
| CEF 打包/签名/公证 | **未验证** | CEF spike 即因体积/启动/双平台构建（含 Windows 签名公证）未达标判 no-go；spec §10.1 已披露 | 若未来重启 CEF 方案 |
| Windows lane 次级覆盖 | **已删除未替换** | `windows-lane-tree-removal-boundary.test.ts` 针对 ade 不存在的 lane 树被删；未以 ade 等价覆盖替换 | Task 9 minor |
| Windows 上的 `pnpm test` 全量 | **未验证** | 本批次全量、冷门禁与 GUI 冒烟均在 macOS/darwin-arm64 完成 | Phase 2 |

## 6. GUI 复核结果

自动启动冒烟（本任务执行，`pnpm dev` 后台启动，观察到应用二进制运行后终止；证据 `dev-smoke.log`）：

```
ROLLDOWN-VITE v7.3.1  ready in 266 ms
➜  Local:   http://127.0.0.1:1420/
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.62s
Running `target/debug/orcinus-app`
```

- 观察到：Vite ready、Tauri dev 进程启动、`target/debug/orcinus-app` 存活运行；日志无编译错误、无 panic、无启动报错。终止后端口 1420 释放、无残留进程（已核对）。
- 需人眼交互、**待用户合并前复核**：
  1. 打开 `.ts` 文件（CombinedDiff/DiffViewer），确认 `ts.worker-*.js` 实际加载、无 worker 报错（Phase 0 遗留 `diagnostics_channel` 复核项，见 `docs/phase0-dead-code-inventory.md`「Phase 1 backlog」第 5 项）；
  2. 终端 pane 可开 tab 并执行命令（node-pty 源码构建后的运行期行为）；
  3. 设置页各分组可达；
  4. 三视图切换与重启持久化；
  5. 应用内 `git status` 展示无异常。

## 7. 隔离与豁免

- 无 quarantine、无自定义 `exclude`、无新增 skip、无静默规避。终局 `vitest` 的 8 个 skipped 文件为仓库既有条件跳过。

## 8. 控制器裁定（Rulings）

1. 隔离用功能分支 `phase1-debt-payoff`（非 worktree），沿用此前删减批次先例。
2. Task 3 步骤顺序调整为「先删代码/设置，再 prune i18n 键」。
3. 删 electron shim 时按需保留/拆分命名空间声明（Task 7 实际保留 `src/renderer/src/electron-webview-globals.d.ts`，7 个成员，因 renderer 生产代码约 107 处 `Electron.WebviewTag` 等引用）。
4. 补丁再生自测的运行方式按文件实际 runner 适配。
5. ko overrides 全文件品牌替换 `Orca→Orcinus`。
6. allowlist 工具兼容 repo-relative 与 src/-relative 两种条目。
7. 测试基线取 2026-09-20 实测（173 test 级失败），失败清单提取须 grep FAIL 前置。
8/9. 计划文本修正属控制器职责（提交 `c6487f2`、`7be0180`）。
10. 任务审查暴露的零引用残留（unpaired 契约、RuntimeHostAccessForm、死键）并入 Task 5 第二个提交。
11. Task 5 前缀规则误删的 4 个 live 模块单测从基线恢复（29 pass）。
13. Task 7 后删除 `src/main/startup` 3 个孤儿文件（serve-desktop-activation、serve-mode-argv、startup-diagnostics）。
15. node-pty 必须源码构建：`pnpm run rebuild:node`（prebuilt spawn-helper 权限 644 且不编译补丁）；脚本以 node wrapper 跨平台化。
16. GUI 复核：本任务做 `pnpm dev` 启动冒烟并记录；需要人眼的交互项（Editor 的 ts.worker 实际加载等）在记录中标注「待用户合并前复核」。

（Ruling 12、14 已并入上述条目表述。）

## 9. 文档回写与勘误（本提交）

| 文档 | 变更 |
|---|---|
| `docs/superpowers/specs/2026-09-14-ade-design.md` | 风险清单第 1 条改为 CEF no-go 结论；`§10` 后新增 `### 10.1 CEF no-go 披露` |
| `docs/phase0-dead-code-inventory.md` | preload 计数 88→80（基线 88，Phase 1 删减 −8）并注明 `src/preload` 运行期树保留至 Phase 2；`RemoteServerUpdateDialog` 改「已整链删除（原『启动链路仍在用』前提失效）」；`orca-profiles` 改「整链 live，不删；仅多 profile 管理死动作留待后续」；`MobileEmulatorSettingsPane` 标注「已被 Phase 1 功能删减 §2.4 推翻（`59e7ee2` 整域删除）」；composer 运行目标改「已决策：VM run-target 保留，端到端移除另立产品决策」 |
| `docs/phase0-acceptance.md` | 末条悬空引用 `task-12-remediation-report.md` 改指 `docs/phase0-dead-code-inventory.md`「Phase 1 backlog」第 5 项 |
| `docs/superpowers/plans/2026-09-14-ade-phase0-skeleton-ui.md` | `../orca-main/orca-main`（18 处）统一为 `/Users/itsuka/CodeSpace/orca` |

说明：`2026-09-14-ade-design.md` 内仍保留 3 处历史快照表述（基线 Windows 路径、`orca-main` 快照无 git 历史、方案 A 只读参照），属 2026-09-14 的成因/背景记录，非可执行路径，未改。仓库无 `AGENTS.md`。

## 10. 收尾状态

- 冷门禁 exit 0、全量测试 exit 0（3825 文件 / 33929 测试 / 0 failed / 0 unhandled）、GUI 启动冒烟通过。
- 本提交后 `git status` clean；分支未 push、未 merge。
- 交付选项（用户决定）：合并到 `main` / 开 PR / 保留分支。
