# Phase 1 功能删减记录（Orcinus 瘦身）

- 分支：`phase1-trim-features`（分支点 `main` @ `0cf68df`）
- 日期：2026-09-17 启动，2026-09-19 收尾（macOS）
- 范围：从渲染层彻底删除 10 个功能域 —— Pet、Contextual Tours、Feature Tips、Feature Wall、Setup Guide、Dictation、Emulator Pane、Activity/Dashboard（含 Kanban/Agent Map/Popout）、Native Chat、Telemetry（含崩溃上报与 Feedback）；含生产代码、测试、mock bridge 域、preload 契约、store 注册/slice、设置项与导航/搜索条目、i18n 键、仅被删域独占的依赖
- 依据：`docs/superpowers/specs/2026-09-17-feature-trim-design.md`、`docs/superpowers/plans/2026-09-17-feature-trim.md`
- 后续 backlog：`docs/phase0-dead-code-inventory.md` 的「Phase 1 backlog」

## 方法与证据链

1. **生产可达性分析**（Task 0 工具 `orcinus-reach.mjs`，固化于 `.superpowers/sdd/2026-09-17-feature-trim/tools/`）：
   入口 = `src/renderer/index.html → main.tsx`、`popout.html → popout.tsx`（Task 5 前）、`web-index.html → web/main.tsx`，以及 `src/main`、`src/preload`、`src/bridge`、`src/types` 全量文件；解析相对导入、`@/`、`@renderer/` 别名与 lazy `import()` 字面量；测试文件（`*.test.ts(x)`/`*.spec.ts(x)`）不纳入可达性图（test-support 文件因此常驻不可达清单）。
2. **基线**：可达性基线 `baseline-reach.txt`（4983abb 时 697 行）；测试基线 `baseline-test-failures.txt`（db90d46 时 175 个 test 级失败名）。
3. **删除规则**：域内文件整域 `git rm`（R4：域内互引使分批门禁不可行，改为一次删除 + 一次门禁）；reach diff 中新增不可达文件逐个判定，仅「无生产/测试导入者」才删；`*.d.ts` 永不作为删除候选（R5，ambient declarations 无导入边）。
4. **门禁**：每次删除后 `pnpm typecheck && pnpm build:web` 必须 exit 0；自 Task 4 起 typecheck 一律先 `rm -f tsconfig.tsbuildinfo` 冷检（R7：增量缓存曾掩盖 10 个错误）。
5. **i18n**：`orcinus-domain-keys.sh` 从待删/待改文件收集候选键 → `orcinus-i18n-prune.mjs` 仅删「catalog 中存在 + 剩余源码无完整字面量 + 父路径无 `${}` 模板拼接」的键；Task 8 另做空壳与残留全量清键。
6. **提交策略**：每域一个提交（审查修复轮单独提交），全程不 merge/push，每域可独立 revert。最终 `git log --oneline phase1-trim-features ^main` = 3 个前置文档提交（spec/plan/gitignore）+ 12 个域提交（10 域，含 4 个审查修复轮）+ 1 个收尾 = 16 个提交（计划预期「9 个提交」按「spec + Tasks 1–7 + 收尾」计算，未计前置文档与修复轮）。

## 提交与门禁表

| 域 | 提交 | 删除文件数（git diff-filter=D，含修复轮） | 批次 | 门禁（typecheck + build:web，均 exit 0） |
|---|---|---|---|---|
| Pet 桌宠 | `0a863ec` + `9d458f0` | 32 | 整域 1 批（23）+ 解耦后 6 + 资源清理 3 | 冷门禁通过 |
| 引导/营销面（Tours/Tips/Wall/SetupGuide） | `db90d46` + `9fe6266` | 190 | 2 批（tours/tips/setup-guide；wall+shared） | 冷门禁通过 |
| Dictation | `2cf2b96` | 40 | 整域 1 批 | 冷门禁通过（首轮增量缓存掩盖错误后改冷检） |
| Emulator Pane | `59e7ee2` + `059b989` | 85 | 整域 1 批 | 冷门禁通过 |
| Activity/Dashboard/Kanban/Popout | `8f05dee` | 341 | 整域 1 批（精确清单） | 冷门禁通过 |
| Native Chat | `bb3e08d` + `d558770` | 572 | 终端 portal 解耦批 + 域内 2 批 | 冷门禁通过 |
| Telemetry 全链 | `6ecbb30` + `493e967` | 122 | 核心层批 + 编译器驱动多批 | 冷门禁通过 |
| 全局收尾（Task 8） | 收尾提交 | 1（`tab-view-mode.test.ts`） | — | 冷门禁通过 |

分支累计（不含收尾提交前的工作区）：**1383 个文件删除**（`git diff --diff-filter=D --name-only main`，含 Task 8 的 1 个测试文件）。

各域提交 stat：

```
0a863ec  55 files changed,    13 insertions,  3902 deletions
9d458f0   3 files changed,     0 insertions,     0 deletions
db90d46 317 files changed,   477 insertions, 32914 deletions
9fe6266  48 files changed,    25 insertions,   221 deletions
2cf2b96  95 files changed,    74 insertions,  6332 deletions
59e7ee2 244 files changed,   381 insertions, 13747 deletions
059b989   2 files changed,     0 insertions,    49 deletions
8f05dee 442 files changed,   390 insertions, 55497 deletions
bb3e08d 780 files changed,   537 insertions, 96428 deletions
d558770  11 files changed,   157 insertions,   155 deletions
6ecbb30 352 files changed,   616 insertions, 19780 deletions
493e967  12 files changed,    37 insertions,   298 deletions
```

## 全局可达性兜底（Task 8 Step 1）

- 最终：`reachable=6066 unreachable=623`（基线 4983abb：697 不可达）。
- 新增孤儿：**0**。基线不可达集合与最终集合做集合差（`LC_ALL=C comm -13`；macOS 默认 locale 的 collation 会把仅标点不同的路径误判为 DIFF，须加 `LC_ALL=C`）为空。
- 74 个基线不可达文件随本删减被删除（697 − 623）。
- 已知死树按计划**不扫**（非目标）：620+ 个残留不可达文件全部为基线既有死代码，分类见「残留表」。

## i18n 清键统计

各域 prune 的 kept/deleted 统计（`orcinus-i18n-prune.mjs` dry-run = apply）：

| 域 | notInCatalog | referenced（保留） | dynamic（模板引用保留） | deleted（域键） |
|---|---|---|---|---|
| Pet | 0 | 44 | 0 | 31 |
| 引导/营销面 | 58 | 294 | 0 | 333（回滚 10 个守卫专属键后净 323） |
| Dictation | 10 | 81 | 0 | 98 |
| Emulator | 68 | 101 | 0 | 157（+8 残留补删 = 165） |
| Activity/Dashboard | 18 | 122 | 0 | 235（+28 补充 sweep = 263） |
| Native Chat | 63 | 185 | 0 | 276 |
| Telemetry | 0 | 146 | 0 | 125 |

- 「referenced」= 剩余源码仍含完整键字面量（含保留域、测试、遥测事件名等）；「dynamic」= 父路径存在 `${}` 拼接；「notInCatalog」= 候选键不在任何 catalog。
- Task 8 全量残留清键：删除 43 个已删域死键（en 43 / 各 locale 37 / en-runtime-required 43）+ `auto.components.settings.VoicePane` 子树（2 键 × 7 catalog）；清除 **1185 个空对象壳**（叶子删除后遗留的 `{}` 父级，含级联），7 个 catalog 空壳归零。
- 各 catalog 叶子键净变化（4983abb → 收尾工作区，脚本按 key-path 集合对比）：

| catalog | 删除叶子 | 新增叶子 |
|---|---|---|
| en.json | 1326 | 0 |
| es.json | 1028 | 0 |
| fr.json | 1087 | 0 |
| ja.json | 1028 | 0 |
| ko.json | 1049 | 0 |
| zh.json | 1034 | 0 |
| en-runtime-required.json | 105 | 0 |
| 合计 | **6657** | **0** |

交叉校验：en 1326 = 域键合计 1281 + Task 8 死键 43 + VoicePane 2。

## 依赖清理（Task 8 Step 3）

逐项 grep（`grep -rl <dep> src --include='*.ts' --include='*.tsx' | grep -v '.test.'`），仅 0 命中才删除：

| 依赖 | 非测试命中 | 判定 |
|---|---|---|
| `@streamparser/json` | 0（全仓 0） | **删除**（dependencies） |
| `html-to-image` | 0（全仓 0） | **删除**（devDependencies） |
| `tldts` | 1（`tab-create-entry-url-classification.ts`） | 保留 |
| `react-grab` | 2（`main.tsx`、`react-devtools-commit-hook-shim.ts`） | 保留 |
| `@dnd-kit/core` / `@dnd-kit/sortable` | 10 / 4 | 保留（worktree 拖拽、tab 排序等保留功能） |
| `@tanstack/react-virtual` | 33 | 保留（虚拟列表） |
| `@sanity/diff-match-patch` | 1（`rich-markdown-source-reconcile.ts`） | 保留 |
| `emoji-picker-react` / `emojibase-data` | 3 / 3 | 保留（composer/编辑器 emoji 与仓库图标） |
| `react-colorful` | 1（`ui/color-picker.tsx`） | 保留 |
| `cmdk` | 22（命令面板） | 保留 |

锁文件更新：`pnpm install --lockfile-only`（仅移除两条 importer 与两条 package 记录）。清理后冷门禁 exit 0。

## ratchet 重基线（Task 8 Step 4）

| ratchet | 结果 |
|---|---|
| `lazy-use-ref-ratchet` / `lazy-modal-mount-state` / `renderer-node-builtin-boundary` | 通过 |
| `child-process-import-boundary` | **既有红，未动**：146 条 stale allowlist、pin 155 vs 实际 7（Phase 1 backlog 第 2 项） |
| `windows-console-visibility` | **既有红，未动**：58 条 stale、pin 65 vs 实际 7（同上） |
| `hover-reveal-touch-action-visibility` | 移除 3 个已删域 pin（`activity/ActivityPrototypePage.tsx`、`native-chat/NativeChatMessageRow.tsx`、`sidebar/WorkspaceKanbanStatusLane.tsx`）；仍红，唯一原因为既有 ENOENT `settings/MobilePairingQrSection.tsx`（Phase 0 已删） |
| `pane-agent-identity-inventory` | 移除 2 个已删的 `dashboard-popout/AgentMap*` pin；仍红，唯一原因为既有缺 `src/main/**`、`mobile/src/**` 树 |
| `pane-agent-identity-surface-inventory` | **未动**：缺失项全部为 `src/main/**` 既有缺口，本删减无 pin |
| `terminal-pane-store-subscription-budget` | 断言更新：随惰性 action 删除，绑定数 28→26，监听器削减常量仍为 34（注释已说明） |
| `tab-view-mode.test.ts` | 随 `setTabViewMode`/`toggleTabViewMode` 删除（无生产调用者） |

## 全量测试基线对比（Task 8 Step 5）

```
pnpm test 2>&1 | tee /tmp/orcinus-test-final.log
exit=1
Test Files  40 failed | 3813 passed | 8 skipped (3861)
     Tests  172 failed | 33902 passed | 123 skipped (34197)
    Errors  3 errors
  Duration  507.26s
```

对比 `baseline-test-failures.txt`（175 test 级失败名，按「去前导空白 + 去 collection `[ path ]` 后缀 + LC_ALL=C 排序」归一）：

- **新增失败：0**
- **修复：3**（`NativeChatPromptEditor.test.tsx` 的 3 个 skill editor 用例，随 native-chat 域删除）
- 172 = 175 − 3；`Errors 3` 对应 4 个 suite 因仓库缺文件收集失败（`web-session-tabs-sync-terminal-mirroring`、`web-session-terminal-orphan-recovery-{prior-removal,adoption-regressions}`、`windows-lane-tree-removal-boundary`），这 4 条均逐字出现在基线失败文件清单中，非本删减引入。

## 自动启动冒烟（人工冒烟的机器部分）

人工 GUI 冒烟无法由实施代理完成（R3），以有界自动启动检查替代，证据 `/tmp/orcinus-dev.log`：

```
pnpm dev   # tauri dev，60s 内启动，随后终止
ROLLDOWN-VITE v7.3.1  ready in 265 ms
➜  Local:   http://127.0.0.1:1420/
Finished `dev` profile [unoptimized + debuginfo] target(s) in 52.53s
Running `target/debug/orcinus-app`     # 进程存活确认后 kill，端口 1420 已释放
```

日志无编译错误、无 panic、无启动报错（"error" 命中均为 crate 名 `thiserror`）。

### 人工冒烟清单（交用户执行）

1. `pnpm dev` 启动，应用无报错；
2. 终端可开 tab 并执行命令；
3. 侧栏 worktree 列表可切换；
4. 设置页各分组可达；
5. 以下入口不存在：Pet 状态栏项、引导/功能墙弹窗、听写、移动仿真设置、Activity/看板入口、原生聊天入口、遥测/隐私遥测项、反馈入口；
6. 浏览器 tab 与 source control 面板仍可用。

## 残留表（未删的孤儿/残留与原因）

| 残留 | 数量/位置 | 原因 |
|---|---|---|
| rpc-contract 死树 | 75 个不可达文件 | 计划非目标；Phase 1 backlog 单独安排 |
| remote-runtime / relay 死树 | 46 | 被 live 测试支持与终端/web 代码牵住；先重构再删 |
| agent-hook-listener 死树 | 52 | 同上，基线即不可达 |
| CLI 死链（`node-cli-command-resolution` → `system-cli-install-dirs` 等） | ~5–21 | Phase 1 backlog |
| test-support/fixtures/harness | ~73 | 测试专用，无生产导入边；大量 kept 测试依赖 |
| 其他预置不可达文件 | ~374 | 基线既有死代码，本删减未新增孤儿 |
| `activity-terminal-portal.ts` 注册表 | 1 文件 + 7 个终端调用点 | 生产者已随 Activity 删除，现为 inert registry；按「不动终端/PTY」保留，待终端 parking 重构 |
| `data-workspace-board-preserve-open` / `preserveWorkspaceBoardOpen` | 侧栏 2 文件 | 仍被拖拽阻塞选择器读取（菜单打开时禁止行拖拽），属性名属历史残留 |
| ai-vault `resume-in-chat` UI plumbing | 5+ 文件 | eligibility 恒不可用、动作自动隐藏；拆除牵动 VirtualList/Row/Details 与测试，保留待后续 |
| `feature-education-telemetry.ts` / RPC `telemetrySource` / 启动 payload `telemetry` / `Tab.viewMode`、`structuredSessionId` | 多处 | 宿主兼容契约（外部宿主/旧会话仍可能读写），只删生产者不删字段 |
| `showMobileButton` 设置 | `AppearanceWindowSidebarSection.tsx` | Phase 0 backlog（「随移动按钮面统一移除」），不在本 10 域 |
| `components/dashboard/` 目录名 | 20 个通用 agent-row 模块 | 名称带 dashboard、实为共享呈现模块；未改名以控制爆炸半径 |
| `rpc-contract/client-ui-params.ts` 死导出 | `UnknownRecord`/`UnknownRecordArray` | 已知死树内部，随死树另行处理 |
| child-process / windows-console / feature-interactions 等既有红 ratchet | 见上表 | Phase 1 backlog，按指令未修 |
| 库级/预置死 CSS（`worktree-item`、`landing-content` 等） | main.css | 与本删减无关；本任务仅移除已删域专属 CSS（−604 行） |

## 推翻 Phase 0 判定的说明（Emulator）

`docs/phase0-dead-code-inventory.md` 保留项曾判定：「`MobileEmulatorSettingsPane` 及 `emulator-pane` 主体……**维持 Task 4 判定**：浏览器仿真能力，非移动端配对；Phase 0 保留」。

本次用户在删减设计（`2026-09-17-feature-trim-design.md` §2.4）明确决定**推翻该判定**，将 Emulator Pane 作为独立域彻底删除（含 `components/emulator-pane/`、`MobileEmulatorSettingsPane`、`MobileEmulatorAgentControlRow`、`MobileEmulatorAvailabilityDetails`、`mobile-emulator-search`、preload/mock 契约、pty 契约方法、shared 默认值/键位/feature-interaction 条目与 i18n 键）。Task 4 提交 `59e7ee2` 执行删除，`'simulator'` tab 类型与持久化 schema 同步移除（R8：旧会话该 tab 经 salvage 逐条丢弃，不整会话报错）。Phase 0 文档中的保留判定自此失效，以本记录为准。

## 验证命令与结果

| 命令 | 结果 |
|---|---|
| 各域 `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web` | 全部 exit 0（Task 7 收尾 `built in 3.39s`；Task 8 收尾 `built in 3.23s`） |
| Task 8 ratchet 组（child-process + lazy-* + boundary + hover/pane） | 9 failed / 116 passed；9 个失败全部逐字命中基线失败清单 |
| `pnpm test`（全量） | 见「全量测试基线对比」：0 新增失败 |
| `node orcinus-reach.mjs` final vs baseline | 新增孤儿 0 |
| `pnpm dev` 有界启动 | Vite ready + 应用启动，无报错 |
| `pnpm install --lockfile-only` | 锁文件仅 −2 importer / −2 package |
