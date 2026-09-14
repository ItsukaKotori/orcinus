# Phase 0 验收记录（Task 11）

- 日期：2026-09-14（macOS）
- 分支：`phase0-skeleton-ui`；验收基线：`d982d62`（Task 10 完成后；Task 11 仅新增本文件与 README，无源码改动）
- 环境：macOS 26.6.2（Build 25G83）arm64，Apple Silicon 8 核 / 16GB；Node v22.21.0；pnpm 12.3.4；rustc/cargo 1.95.0（Homebrew）；Xcode CLT（Apple clang 21）
- 规格验收条款：`docs/superpowers/specs/2026-09-14-ade-design.md` §8 Phase 0
- 证据目录：`.superpowers/sdd/2026-09-14-ade-phase0-skeleton-ui/task-11-evidence/`（未入库）与 `/tmp/ade-*.log`（本机临时）

## 结论摘要

| Spec §8 验收项 | 判定 | 说明 |
|---|---|---|
| `pnpm dev` 启动 Tauri 窗口 | **不通过（功能性）** | 窗口能启动（标题 `ade`，1440×900），但应用 shell 渲染失败：dev 显示 Orca 渲染错误边界，生产二进制白屏；详见 §2 |
| 三视图可切换且状态可持久化 | 逻辑证据通过；GUI 未执行（需人工） | 组件/store 测试 6 文件 59 用例全过；交互切换与重启恢复被 §2 阻塞，标未执行 |
| 插件中心可浏览（scope 徽标/启停） | 逻辑证据通过；GUI 未执行（需人工） | scope 徽标有单测；启停开关仅代码证据，点击未执行 |
| 右侧栏只出现项目级插件 | 通过（逻辑） | `filterRightSidebarPluginEntries` 单测 + 接线复核 |
| 设置页为简化分组 | 逻辑证据通过；GUI 未执行（需人工） | 简化分组渲染测试 4 用例通过；页面交互未执行 |
| 构建无 Electron 依赖 | **部分通过** | 生产 bundle 无 electron 模块；`grep -rn "from 'electron'" src/renderer src/shared src/bridge` 仍有 1 处 type-only 测试导入（§7） |
| 两个 spike 有书面结论 | 通过 | PTY（Windows 实测/macOS 未验证）、CEF（macOS 实测/no-go/Windows 未验证）；§8 |

**总判定：Phase 0 验收未完全通过。** 构建链与逻辑层达标；`pnpm dev` 应用 shell 在 Tauri/WebKit 下无法渲染，属阻断性缺陷，需修复后重跑本清单的 §2–§6 人工项。全量测试 194 项失败中另发现 1 处 Task 5 引入的回归（§9.2），其余为 fork 遗留与环境问题。

## 1. 全量验证命令与结果

| 命令 | 退出码 | 结果 |
|---|---|---|
| `pnpm typecheck` | 0 | 无输出（tsc --noEmit 通过） |
| `pnpm build:web` | 0 | `✓ built in 4.85s`；仅 chunk >500kB 提示（既有） |
| `pnpm test`（= `vitest run`，R4 取代计划中的 `pnpm test -- --run`） | 1 | **44 failed files / 194 failed tests / 3 unhandled errors**；4332 passed files / 38584 passed tests / 123 skipped；456.10s（见 §9 清单） |
| `cargo check --manifest-path src-tauri/Cargo.toml` | 0 | `Finished dev profile … in 50.10s` |
| `pnpm tauri build --debug --no-bundle` | 0 | `Built application at: src-tauri/target/debug/ade-app`（40,032,392 B，无 `.exe`，符合 R31） |

命令实测尾行摘录：

```
$ pnpm typecheck
$ tsc --noEmit -p tsconfig.json          # exit 0

$ pnpm build:web
✓ built in 4.85s                          # exit 0

$ pnpm test
 Test Files  44 failed | 4332 passed | 8 skipped (4384)
      Tests  194 failed | 38584 passed | 123 skipped (38901)
     Errors  3 errors
  Duration  456.10s (transform 153.83s, setup 85.35s, import 1629.38s, tests 665.70s, environment 216.84s)

$ cargo check --manifest-path src-tauri/Cargo.toml
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 50.10s

$ pnpm tauri build --debug --no-bundle
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 32.07s
       Built application at: /Users/itsuka/CodeSpace/ade/src-tauri/target/debug/ade-app
```

## 2. `pnpm dev` 窗口检查（未通过）

- 环境：macOS 26.6.2 arm64，本机有显示器；`pnpm dev` 分离启动（vite 124ms ready，Rust 复用 debug 产物 14.72s）。
- 结果：Tauri 窗口出现（标题 `ade`，1440×900，菜单栏显示 `ade`），但 WebView 渲染的是 Orca 的「遇到渲染器错误 / 应用 shell 无法完成渲染」错误边界。
- 截图：`/tmp/ade-dev-try1.png`、`/tmp/ade-dev-window2.png`（窗口 + 错误边界），副本见证据目录；前一张独立运行截图 `/tmp/ade-dev-window.png` 因游戏窗口抢占前台只拍到游戏，不作为证据。
- 交叉验证（区分引擎问题与 bundle 问题）：
  1. 直接运行 `pnpm tauri build --debug --no-bundle` 产物（生产资源内嵌）：窗口白屏，无错误边界 UI（`/tmp/ade-prod-window2.png`）。
  2. 用 `dist/` 静态服务 + headless Chromium（CDP）加载同一生产 bundle：shell **能**渲染（DOM 正文出现「搜索/入门清单/任务/自动化/项目/No workspaces found/添加项目…」），但 15s 内产生 **148,893 次未处理 Promise 拒绝**：
     `UnimplementedBridgeError: ade bridge method not implemented yet: crashReports.recordBreadcrumb`
     （另有 `platform.get` 240 次等一次性未实现调用）。
- 已定位的机制（确定性）：`src/renderer/src/lib/crash-diagnostics.ts:84` 的 `unhandledrejection` 监听器调用 `recordRendererCrashBreadcrumb` → mock bridge 对 `crashReports.recordBreadcrumb` 返回 reject → 该 reject 再次触发监听器，形成自激循环。原 Electron preload 实现了该方法，Phase 0 mock 未实现（`src/bridge` 的 unimplemented fallback）。
- 未定位：WebKit 专有的渲染异常本体（错误边界文案不携带底层错误；已打开 Web Inspector（截图 `/tmp/ade-devtools.png`），但受前台应用抢占未能读到 Console 内容）。
- 建议修复（交控制器决策，本任务未改代码）：Phase 0 让 mock bridge 对 fire-and-forget 方法（至少 `crashReports.recordBreadcrumb`）返回 resolved no-op，并在 `recordRendererCrashBreadcrumb` 处兜底 `catch`；随后重跑本检查。修复前 §3–§6 的交互项无法进行。

## 3. 三视图切换与持久化

逻辑证据（`pnpm test <6 文件>` → 6 files / 59 tests 全过；日志 `ade-evidence-tests.log`）：

- `src/renderer/src/components/global-rail/GlobalActivityRail.test.tsx`：点击「插件中心」→ `activeView === 'plugin-center'`；全局插件条目渲染；底部「设置/帮助」入口。
- `src/shared/top-level-view.test.ts`：`plugin-center` 与 `plugin:<key>` 校验通过，`skills`/`mobile` 被拒。
- `src/renderer/src/store/slices/ui-hydration-view-layout.test.ts:130`：持久化的 `plugin-center` 视图在 hydration 后恢复（重启恢复的逻辑证据）。
- `src/renderer/src/lib/right-sidebar-visibility.test.ts`：`plugin-center`/`plugin:` 视图下右侧栏抑制。

GUI 交互（实际点击三视图、重启进程验证回到上次视图）：**未执行（需人工）**，被 §2 阻断；逻辑证据如上。

## 4. 插件中心可浏览

- scope 徽标：`PluginCenterEntryRow.test.tsx` 通过（「Database Manager」+「全局」徽标）；实现 `PluginCenterEntryRow.tsx:64-66`（`全局`/`当前项目`）。
- 启停：`PluginCenterEntryRow.tsx:87-91` 为 `Switch`（aria-label 启用/禁用），页面数据来自 `plugin-center-slice.ts`（mock），但**点击启停未执行（需人工）**。
- 市场/开发中 tab 为 mock/空态（Task 6 范围）。

## 5. 右侧栏只出现项目级插件

- `src/renderer/src/components/right-sidebar/plugin-panel-scope-filter.test.ts` 通过：保留 `repo-notes`（项目）与 `legacy`（scope 缺失按项目），丢弃全局。
- 接线：`use-right-sidebar-activity-items.ts:54` 对 `pluginCenterEntries` 套用 `filterRightSidebarPluginEntries`。

## 6. 设置页简化分组

- `Settings.load-performance.test.ts` 通过（4 用例，渲染设置页）。
- 分组注册见 `settings-navigation-foundations.ts:10`；`grep` 复核设置目录与 `AppWorkspaceShell.tsx` 中已无 `SshPassphraseDialog`/`MobilePage`/`SkillsPage`/`AddRemoteHost`/`EphemeralVmSettings`/账号用量分组入口（无输出）。
- 交互可用性（打开各分组、deep link）：**未执行（需人工）**。

## 7. 构建链无 Electron 依赖（部分通过）

- 验收命令原文：`grep -rn "from 'electron'" src/renderer src/shared src/bridge` → **1 处命中**（非空）：
  - `src/renderer/src/lib/serve-desktop-promotion-session-continuity.test.ts:17`：`import type { App, BrowserWindow } from 'electron'`（**type-only**，编译期擦除；该测试还引用 `src/main` 的 Electron 主进程模块，属 fork 未裁的 main 侧测试簇）。
- `src/shared`、`src/bridge`：0 处命中。
- 生产 bundle 复核：`pnpm build:web` 产物 `dist/assets` 中无 `from "electron"` / `require("electron")`（正则检索无匹配）→ 构建链本身无 Electron 运行时依赖。
- 结论：规格语义（构建无 Electron）达标；字面 grep 因 1 处 type-only 测试导入未达「无输出」。建议 Phase 1 shim 清理时把该测试一并处理（vitest 已用 `electron-vitest-stub` 别名兜底，见 `vitest.config.ts:16`）。
- 范围说明：`src/preload` 仍有 Electron 导入，规格验收 grep 不含该目录，未列入本项判定。

## 8. 两个 spike 书面结论

| spike | 文档 | 结论 | 平台覆盖 |
|---|---|---|---|
| PTY 吞吐 | `docs/spikes/2026-09-14-pty-throughput.md` | 8 MiB ≈ 21–24 MB/s < 50 MB/s 阈值 → **Phase 1 终端数据通道走本地 socket**，Tauri Channel 仅控制消息；附 ConPTY CPR(`ESC[6n`) 启动阻塞修复与 +10% 字节改写 | Windows 实测；**macOS 未验证** |
| CEF 打包 | `docs/spikes/2026-09-14-cef-packaging.md` | 体积 334.5 MB > 300 MB 阈值 → **no-go**，Phase 3 维持系统 WebView 降级；冷构建 80s、进程隔离成立 | macOS arm64 实测；**Windows 未验证**；签名/公证未测 |

- R31 说明：原计划方向为 Windows 主测、macOS 记录未验证；迁移后反向（上表已如实标注各半）。
- R34：CEF 结论的规格 §10.1 回写建议文本见 `docs/spikes/2026-09-14-cef-packaging.md:138-143`；spec 文件本轮有意未改，**no-go 属 Phase 3 决策，需用户确认**。

## 9. 全量测试失败清单（不得隐藏）

全量 `pnpm test`：**40 个失败文件 / 194 个失败用例**，另有 **4 个 Failed Suites**（收集失败）与 **3 个 unhandled errors**。未修复任何失败（Task 11 无源码改动）；隔离复现样本（6 文件）计数与全量一致，确认确定性（`ade-isolation-sample.log`）。

### 9.1 按已知/遗留分组

- **已知且未修复（Task 4/10 记录）**：`ko-ui-semantic-mistranslations.test.ts`（缺 `config/scripts/locale-ko-key-overrides.json`，1 项）；`child-process-import-boundary.test.ts`（stale allowlist 146 条 + `DIRECT_IMPORTER_PIN=155` vs offenders=7，2 项）。隔离复现与 Task 10 记录一致，无变化。
- **Task 2 记录的 excluded 测试**：`src/renderer/src/app-shell/workspace-view-cross-client-sync.test.tsx` 仍被 `vitest.config.ts:36` 排除，未参与全量。
- **fork 遗留（G/I/远程面删除后缺文件/目录）**：web-session 3 个 suite、`windows-lane-tree-removal-boundary`（缺 `.github/workflows/pr.yml`）、`hover-reveal-touch-action-visibility`（缺 `MobilePairingQrSection.tsx`）、`agent-hook-listener-relay-dependency`、`pane-agent-identity-*`、`remote-runtime-shared-control-boundary`、`workspace-cleanup-scanned-host-confirmation-removal`、`plugins/plugin-*-fixture`（缺 `examples/plugins/*`）。
- **清单/ratchet 漂移**：`useIpcEvents-lifecycle`（97→96，已删 `ui.onOpenSkillShare`）、`windows-console-visibility`（58 条 stale）、`feature-interactions`、`cli-runtime-pairing-boundary`。
- **环境/引擎相关（确定性命中）**：xterm IME/搜索/装饰断言簇（happy-dom/Node）；`pty-reply-echo-shapes.node-pty`（`posix_spawnp failed.`）；tiptap 编辑器簇（`there is no window object available`）。

### 9.2 新发现：Task 5 引入的回归（19 项，建议单独修复）

- 现象：`github-refresh-sweep.test.ts`（14）、`github-worktree-refresh-if-stale.test.ts`（4）、`github-pr-refresh-states-leak.test.ts`（1）全部以 `TypeError: Cannot read properties of undefined (reading 'startsWith')` 失败，栈顶 `src/renderer/src/lib/right-sidebar-visibility.ts:20`。
- 机制：Task 5 为 `plugin:<key>` 视图新增 `isPluginHostedView(activeView) => activeView.startsWith(...)`；这些 GitHub slice 测试用部分 store（无 `activeView`）驱动 `refresh-sweep-actions.ts:58` → `rightSidebarShowsPullRequestData(state)`，原 fork 实现（`../orca` 同路径文件）只做 `Set.has(activeView)`，不会崩。生产态 `activeView` 恒有值，实际影响主要在测试面，但属真实回归。
- 建议修复：`activeView?.startsWith(PLUGIN_HOSTED_VIEW_PREFIX) ?? false`（或 store 测试补 `activeView: 'terminal'`），另行提交。

### 9.3 失败文件明细（40 文件 / 194 用例）

| # | 测试文件 | 失败数 | 根因 |
|---|---|---|---|
| 1 | `src/renderer/src/i18n/ko-ui-semantic-mistranslations.test.ts` | 1 | 缺 `config/scripts/locale-ko-key-overrides.json`（**Task 4 已知**） |
| 2 | `src/renderer/src/hooks/useIpcEvents-lifecycle.test.ts` | 1 | 期望表 97 项 vs 实际 96 项（已删 `ui.onOpenSkillShare` 残留） |
| 3 | `src/renderer/src/components/hover-reveal-touch-action-visibility.test.ts` | 1 | 缺 `settings/MobilePairingQrSection.tsx`（移动端移除遗留） |
| 4 | `src/renderer/src/components/terminal-scrollback-decoration-eviction.test.ts` | 7 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 5 | `src/renderer/src/components/terminal-search-decoration-leak.test.ts` | 6 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 6 | `src/renderer/src/components/terminal-search-long-wrapped-line.test.ts` | 15 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 7 | `src/shared/agent-hook-listener-relay-dependency.test.ts` | 1 | ENOENT `src/relay/agent-hook-server.ts`（relay 移除遗留） |
| 8 | `src/shared/cli-runtime-pairing-boundary.test.ts` | 2 | CLI 允许清单期望 2 条解析结果，实际 [] |
| 9 | `src/shared/feature-interactions.test.ts` | 1 | feature 目录仍含已移除项的 wiring（期望 [] vs 5 条） |
| 10 | `src/shared/pane-agent-identity-inventory.test.ts` | 2 | ENOENT `src/main/runtime/orchestration/mailbox-pointer-stage.ts` + 清单计数漂移（117→105） |
| 11 | `src/shared/pane-agent-identity-surface-inventory.test.ts` | 2 | ENOENT `src/main/runtime/orchestration/groups.ts`（orchestration 移除遗留） |
| 12 | `src/shared/pty-reply-echo-shapes.node-pty.test.ts` | 4 | `posix_spawnp failed.`（node-pty/测试环境 spawn 失败） |
| 13 | `src/shared/remote-runtime-shared-control-boundary.test.ts` | 2 | ENOENT `src/main/ssh`、`src/relay` 目录（远程面移除遗留） |
| 14 | `src/renderer/src/components/editor/markdown-dirty-state.test.ts` | 1 | tiptap「there is no window object available」（测试缺 happy-dom 环境） |
| 15 | `src/renderer/src/components/editor/markdown-round-trip.test.ts` | 12 | tiptap「there is no window object available」（测试缺 happy-dom 环境） |
| 16 | `src/renderer/src/components/editor/rich-markdown-html-superscript-link.test.ts` | 3 | tiptap「there is no window object available」（测试缺 happy-dom 环境） |
| 17 | `src/renderer/src/components/editor/rich-markdown-list-tokenizers.test.ts` | 2 | tiptap「there is no window object available」（测试缺 happy-dom 环境） |
| 18 | `src/renderer/src/components/editor/rich-markdown-table-keyboard.test.ts` | 1 | tiptap「there is no window object available」（测试缺 happy-dom 环境） |
| 19 | `src/renderer/src/components/native-chat/NativeChatPromptEditor.test.tsx` | 3 | tiptap「there is no window object available」（测试缺 happy-dom 环境） |
| 20 | `src/renderer/src/components/workspace-cleanup/workspace-cleanup-scanned-host-confirmation-removal.test.tsx` | 1 | 缺 `src/main/ipc/workspace-cleanup-scan` 模块 |
| 21 | `src/renderer/src/store/slices/github-pr-refresh-states-leak.test.ts` | 1 | **Task 5 回归**：`right-sidebar-visibility.ts:20` `activeView.startsWith` 未防 undefined → TypeError |
| 22 | `src/renderer/src/store/slices/github-refresh-sweep.test.ts` | 14 | **Task 5 回归**：同上 |
| 23 | `src/renderer/src/store/slices/github-worktree-refresh-if-stale.test.ts` | 4 | **Task 5 回归**：同上 |
| 24 | `src/shared/child-process/child-process-import-boundary.test.ts` | 2 | ratchet：146 条 stale allowlist + pin 下界（**Task 10 已知**） |
| 25 | `src/shared/child-process/windows-console-visibility.test.ts` | 2 | ratchet：58 条 stale 条目未清 |
| 26 | `src/renderer/src/components/terminal-pane/terminal-ime-composer-placeholder-mask.test.ts` | 5 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 27 | `src/renderer/src/components/terminal-pane/terminal-ime-hangul-syllable-flush.test.ts` | 2 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 28 | `src/renderer/src/components/terminal-pane/terminal-ime-won-composition-order.test.ts` | 6 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 29 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-adversarial.test.ts` | 6 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 30 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-cancelled-preedit-visibility.test.ts` | 2 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 31 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-composition-cancel.test.ts` | 1 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 32 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-composition-deduplication.test.ts` | 35 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 33 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-consumed-key-commit.test.ts` | 2 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 34 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-linux-native-trace-replay.test.ts` | 2 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 35 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-midline-preedit-tail.test.ts` | 19 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 36 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-resumed-preedit-visibility.test.ts` | 3 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 37 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-trailing-preedit-occlusion.test.ts` | 4 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 38 | `src/renderer/src/components/terminal-pane/terminal-ime-xterm-transaction-events.test.ts` | 14 | xterm IME/装饰断言在 happy-dom/Node 环境下失败（确定性） |
| 39 | `src/shared/plugins/plugin-demo-fixture.test.ts` | 1 | 缺 `examples/plugins/*` 夹具文件（未随 fork 迁移） |
| 40 | `src/shared/plugins/plugin-hostile-fixture.test.ts` | 1 | 缺 `examples/plugins/*` 夹具文件（未随 fork 迁移） |

### 9.4 Failed Suites（4）与 Unhandled Errors（3）

| suite | 原因 |
|---|---|
| `src/renderer/src/runtime/web-session-tabs-sync-terminal-mirroring.test.ts` | fixture `web-session-terminal-host-finalization.ts:18` 导入已移除的 `main/runtime/runtime-mobile-session-result-finalization`（0 test） |
| `src/renderer/src/runtime/web-session-terminal-orphan-recovery-adoption-regressions.test.ts` | 同上 |
| `src/renderer/src/runtime/web-session-terminal-orphan-recovery-prior-removal.test.ts` | 同上 |
| `src/shared/windows-lane-tree-removal-boundary.test.ts` | 读取缺失的 `.github/workflows/pr.yml`（0 test） |

Unhandled errors：3 次 `TypeError: Cannot read properties of undefined (reading 'dimensions')`，来自 xterm 6.1.0-beta 的 `updateCompositionElements` 定时器（originating: `terminal-ios-hangul-preedit.test.ts`、`terminal-ime-hangul-syllable-flush.test.ts`）。

## 10. 后续动作（建议）

1. **P0 / 阻断**：修 §2 mock bridge 未实现方法的 reject 循环（至少 `crashReports.recordBreadcrumb`），复跑 `pnpm dev` 确认 shell 渲染。
2. **P0 / 回归**：修 §9.2 `right-sidebar-visibility.ts` 的 undefined 防护，消除 19 项 GitHub 测试失败。
3. **Phase 1**：按 Task 10 清单继续清理 fork 遗留失败簇（rpc-contract/remote-runtime/CLI、ratchet 重基线、i18n 清键、excluded 测试重指向）；`examples/plugins` 夹具与 CI 工作流文件随对应功能补。
4. **Phase 3**：CEF no-go 结论（§8）需用户确认后回写 spec §10.1。
5. 人工复跑 §3–§6 GUI 交互项（三视图切换/重启持久化、插件启停、设置页）——修复 §2 后执行。

## 11. 证据文件

- 全量测试：`/tmp/ade-full-test.log`（副本 `ade-full-test.log`）
- 隔离复现（6 文件）：`/tmp/ade-isolation-sample.log`（副本 `ade-isolation-sample.log`）
- 逻辑证据测试：`/tmp/ade-evidence-tests.log`（6 passed / 59 tests）、`/tmp/ade-settings-tests.log`（4 passed）
- 构建：`/tmp/ade-build-web.log`、`/tmp/ade-cargo-check.log`、`/tmp/ade-tauri-debug-build.log`
- GUI：`/tmp/ade-dev-try1.png`（dev 错误边界）、`/tmp/ade-prod-window2.png`（prod 白屏）、`/tmp/ade-devtools.png`（Web Inspector）；Chromium CDP 探针原始输出 `/tmp/ade-cdp-probe.json`（127MB，含 148,893 次 rejection 事件）
