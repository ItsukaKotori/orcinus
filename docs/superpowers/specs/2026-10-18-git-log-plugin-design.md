# Phase 4：Git Log 插件（IDEA VcsLog 风格提交图谱）设计规格 · 增量 1（fixture UI 先行）

- 日期：2026-10-18
- 状态：brainstorming 输出（待用户 review 后进入 writing-plans）
- 基线：`main`；工作树 `phase4-git-log-graph`（自 `main@e11d9cda` 切出，位于 `/Users/itsuka/CodeSpace/ade-git-log`）。与并行开发中的 `phase3c-ui-worker-channel` **完全隔离**，不改 phase3c 正在动的能力表/方法表文件。
- 上游参照规格：
  - `docs/superpowers/specs/2026-10-11-plugin-system-design.md`（Phase 3A 插件系统）
  - `docs/superpowers/specs/2026-10-17-plugin-ui-worker-channel-design.md`（Phase 3C：UI↔worker 通道 + updater 构建规范。**本增量沿用其"构建型 TS 插件"与六项视觉一致约束；真实数据（增量 2）复用其通道**）
- 行为参照：`docs/phase3a-plugin-system-record.md`；IDEA 源码 `/Users/itsuka/CodeSpace/intellij-community-master/plugins/git4idea`（VcsLog 语义参照，**不移植代码**）。

## 1. 背景与目标

把 ade 的提交历史从"Source Control 内嵌折叠区块"升级为 **IDEA VcsLog 风格的独立主区 Git Log 大视图**（图谱 + 分支面板 + 过滤条 + 提交详情区），以**插件**形态交付：最大化隔离、**零改动现有 git 功能与插件宿主能力表**、可 crash 隔离、主区独立 tab（与用户此前两项取向一致）。

**本增量（1）只交付可直观迭代的 UI**（fixture 驱动）：用户可用 dev 插件热重载即时调 UI，且不触碰后端与能力表、与 phase3c 零冲突。真实数据留待增量 2 复用 phase3c 通道。

### 现状与差距（探查证据）
- ade 现有 git 公开面（bridge `commands/git.rs`）：status / diff / stage·unstage（含批量）/ discard（含批量）/ commit / push / upstream_status / conflict_operation / branch·commit compare / branch·commit diff / history / remote_urls / git_read。
- 现有提交历史 UI：`src/renderer/src/components/right-sidebar/source-control/sync/git-history*` —— Source Control 内一个折叠式 "Commits" 区块，仅查 `git log <HEAD> -n 50`（上限 200）、HEAD-scoped、无 `--all` / 过滤 / 分页 / 独立大视图 / 详情区。
- 现有纯图谱逻辑可复用、electron-free：`src/shared/git-history-graph.ts`（`buildGitHistoryViewModels` 泳道布局）、`src/shared/git-history*.ts`（types / log-parser / ref-display / boundary-rows），**无 `window.api` / `@tauri` / renderer 引用** → 可打进插件 bundle（只读复用，不改这些共享模块、不改现有 Source Control）。
- 插件面板硬约束（`src-tauri/crates/ade-plugins/src/panel_shell.rs`）：CSP `default-src 'none'; connect-src 'none'; script-src 'unsafe-inline'; img-src data:; font-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'`。→ **只能内联 `<script>/<style>`，禁外链/fetch/import**；数据只能走 postMessage host 方法。shell 自动注入 20 个 design-token（`PANEL_DESIGN_TOKEN_ALLOWLIST`）。
- 面板可调 host 方法（main 上 `PLUGIN_HOST_API_V0`）：仅 `workspace.readContext / workspace.openTab / terminal.sendText / notifications.show`（`gate.rs` 对其余 `panel_forbidden`；storage/secrets/settings/events 为 worker-only）。**无任何 git 数据方法。**
- 主区 plugin tab 在 main 即可用：`workspace.openTab` 方法、`PluginTabContent` 渲染、Rust `contributes.tabs`（limit 64）均已存在；phase3c 仅补 worker↔UI 通道。
- phase3c 正在给 `PLUGIN_CAPABILITY_KINDS`（8→10）与 host 方法表（14→16）加 `worker:call`/`session:push` + `worker.invokeCommand`/`session.post`。**本增量绝不改这两张表**，避免并行冲突。
- 已批准"构建型 TS 插件"规范（3C §3.5/3.8，updater）：React 19 + Tailwind v4 同宿主版本、复用根依赖；vite 多入口 + 自定义内联插件把 JS/CSS 单文件化；Geist woff2 内联为 data: URI；`@theme inline` 与宿主 `src/renderer/src/assets/main.css`（`@theme inline`，line 43+）**逐字相同**、由 vitest parity 锁死；v1 文案英文内嵌。
- 版本事实（根 `package.json`）：react / react-dom `^19.2.8`、`@tanstack/react-virtual` `^3.14.10`、tailwindcss `^4.2.4`（`@tailwindcss/vite`）、`@vitejs/plugin-react` `^5.2.0`、typescript `^7.0.2`、`esbuild` `^0.25.12`。根无 `plugins/` 目录（本增量新建 `plugins/git-log/`）。manifest 文件名：`orca-plugin.json`（`PLUGIN_MANIFEST_FILENAME`）。

## 2. 范围

### 2.1 实现面（增量 1；全部新建文件 + 少量根接线）

| 面 | 载体 | 说明 |
|---|---|---|
| 插件包 | `plugins/git-log/`（新） | `orca-plugin.json`、`build.mjs`、`tsconfig.json`、`src/ui/*`、`src/fixtures/*`、`src/graph/*`（复用/薄封装 shared 纯逻辑）；产物：`dist/tab.html`（vite 内联构建；**dist 不入库**）+ `launcher.html`（**手写入库**、最简内联，非构建产物——YAGNI：不为启动器开第二个 vite 入口）。 |
| manifest | `plugins/git-log/orca-plugin.json` | `contributes.tabs:[{id:"git-log",title:"Git Log",icon:"<宿主图标注册表键>",entry:"dist/tab.html"}]`（主区大视图）+ `contributes.panels:[{id:"launcher",title:"Git Log",entry:"launcher.html"}]`（右侧栏极简启动器，手写入库内联）；`capabilities:[{kind:"workspace:open-tab"}]`（**仅现有 kind，不新增**；launcher panel 内联脚本调 `workspace.openTab`）。**增量 1 不声明 `contributes.commands`、不含 worker**（命令触发需 worker handler，见「打开入口」行）。 |
| UI 框架 | React 19 + Tailwind v4 | 与宿主同版本，复用根依赖与 `@tailwindcss/vite`；沿用 3C 六项视觉一致约束（§5）。 |
| 数据缝 | `GitLogRepository` 接口 + `FixtureRepository` | UI 仅依赖接口；fixture 产出**对齐 `GitHistoryResult`/`GitHistoryItem` 形状**的快照，令增量 2 换真实数据零 UI 改动。`ChannelRepository` 仅留**类型占位**，不接、不声明 `worker:call`。 |
| 图谱逻辑 | import `src/shared/git-history-graph.ts` 等纯模块进 bundle | 只读复用泳道布局 / ref 显示 / boundary rows；**不修改**共享模块与现有 Source Control。 |
| 打开入口 | 右侧栏 **launcher panel** 内联脚本 postMessage `workspace.openTab({tabId:"git-log"})` → Rust dispatch（`host/methods.rs` `workspace.openTab` panel:true 已验证）→ bridge `plugins:open-tab` → `PluginTabContent` 渲染主区 tab | **机制已在 main 上核实**：`workspace.openTab` 在 `PLUGIN_HOST_API_V0` 中 `panel:true`，沙箱 panel 可直接调；`contributes.tabs`/`TabContrib`、dispatch、`plugins:open-tab` 事件、`PluginTabContent` 路由均存在于 main。增量 1 用 launcher panel 作零-worker 触发面（contributed command 经 ⌘J/键绑定触发的是 worker/built-in handler，无 worker 时不可用，故不选）。 |
| 根接线 | 根 `package.json` `build:git-log`；`vitest.config.ts` include 增 `plugins/**/*.test.ts`；`.gitignore` 排除 `plugins/**/dist` | 与 3C 对根接线的改动**同型、行级可并存**（低冲突面）。 |

### 2.2 不做（增量 1 明确排除）
- 不新增 `git:read` capability / `git.log` host 方法 / 任何 `PLUGIN_CAPABILITY_KINDS`·host 方法表改动（避让 phase3c）。
- 不接真实 git 数据、不 spawn git、不使用 worker 通道（→ 增量 2）。
- 不实现 `--all` 全分支 / 过滤**执行** / 分页 skip-limit（fixture 为固定数据集；过滤条/分支选择器为 UI 呈现 + 本地 fixture 交互，非真查询）。
- 不做 checkout / cherry-pick / revert / reset / squash / amend / stash（未选子系统）。
- 不做 i18n 本地化（v1 英文内嵌；宿主 i18n 不跨 opaque-origin 面板复用 —— 与用户"复用 i18n"取向的已知偏差，见 §7 风险）。
- 不做原生剪贴板 copy-hash（面板无该 host 方法；增量 1 作为无操作/不列）。
- **不改任何现有 git 功能 / Source Control 组件 / backend Rust git 命令** → `cargo test --workspace` 不受影响。

## 3. 视图分区（IDEA VcsLog 参照，增量 1 = fixture 驱动）

- **顶部工具条**：视图标题 + 当前分支标签（增量 1 取自 `GitLogSnapshot.currentRef`，**fixture 驱动、不调 `workspace.readContext`**，与「不碰后端/零额外 capability」一致）；搜索框；过滤 chips（Author / Branch / Date / Path，增量 1 对 fixture 本地过滤）；分支选择器；刷新（重载 fixture）。
- **左侧 分支/refs 面板**（可折叠）：Local / Remote / Tags 分组树（fixture）；点选按该 ref 过滤图谱列表。
- **中部 提交图谱列表**（`@tanstack/react-virtual` 虚拟化）：左列 lane SVG（复用 shared 泳道）→ commit dot → ref chips → subject → author → relative date；选中高亮。
- **底部 提交详情区**：选中提交的 短 hash / 完整 author / date / message 正文 / `+ -` 统计 / 变更文件列表（fixture；文件行"打开 diff"在增量 1 为占位）。
- **键盘**：↑/↓ 选提交、Enter 展开/聚焦详情（增量 1 基础）。
- **light/dark**：仅用注入的 20 token + `@theme inline`，与宿主同屏对照一致。

## 4. 数据缝与 fixture

- `GitLogRepository`：
  - `loadSnapshot(): Promise<GitLogSnapshot>`
  - `loadCommitFiles(oid: string): Promise<GitBranchChangeEntry[]>`
- `GitLogSnapshot` 形状**对齐** `GitHistoryResult`：`items: GitHistoryItem[]` + `currentRef/remoteRef/baseRef/mergeBase/hasIncomingChanges/hasOutgoingChanges/hasMore/limit`；`GitHistoryItem` 含 `id/parentIds/subject/message/author/authorEmail/timestamp/statistics/references`。使增量 2 直接换 `ade_git::history` 输出时 UI 与图谱零改动。
- fixture：确定性数据集，覆盖 线性 / 分叉 / merge 交叉 / 多 tag / 本地+远端 / ahead-behind，**≥30 提交**以验证虚拟滚动与多泳道布局。

## 5. 构建与视觉一致规范（沿用 3C §3.5/3.8）

- `plugins/git-log/build.mjs`：vite **单入口**产出 `dist/tab.html`（主视图）；launcher 为手写入库 `launcher.html`（不走构建）；自定义内联把 **JS/CSS 全部内联**（CSP 禁外链/import），并对内容中的 `</script>` 做提前闭合中和；Geist woff2（`src/renderer/src/assets/fonts/Geist-Variable.woff2`）内联为 `data:` URI。
- `@theme inline` 块与宿主 `src/renderer/src/assets/main.css`（`@theme inline`）**逐字相同**；vitest parity 测试读取两处文本比对锁死。
- 字体栈与宿主逐字一致；mono 用宿主同栈（系统字体）。
- 类型：`plugins/git-log/tsconfig.json`（UI 层，禁引 node types；增量 1 无 worker）。并入根 `typecheck`。
- dev 流程：`pnpm build:git-log` → 设置 `devPluginPaths` 指向插件根 → 插件中心启用（consent 仅 `workspace:open-tab`）→ 点右侧栏 launcher 打开 Git Log tab → 改源码 rebuild → dev watcher 热重载。

## 6. 测试与验收

- 自动化门禁：`pnpm typecheck && pnpm build:web && pnpm build:git-log && pnpm test`。**无 Rust 改动 → `cargo test --workspace` 不需重跑**（在记录中注明增量 1 不触后端）。
- 单测（vitest）：图谱泳道布局对 fixture 的断言/快照；`@theme inline` parity；`GitLogSnapshot` 形状合法性（类型守卫/zod）。
- 手工清单：dev 加载 → consent（仅现有能力）→ 开 tab → 图谱/分支面板/详情区渲染 fixture；light/dark 与宿主同屏对照；≥30 行虚拟化流畅；折叠分支面板 / 展开过滤 / 键盘选提交可用。

## 7. 风险与缓解决策

- **与 phase3c 抢改根接线**（package.json / vitest.config.ts / .gitignore）→ 均为**行级新增**、语义不重叠；对齐 3C 同型接线，merge 线性可解。核心能力表/方法表**本增量不碰**。
- **内联 bundle 体积**（React + 组件 + virtual 约 1–3MB）→ 单 tab 上限 10MiB 内；构建 minify；必要时按 tab 拆分入口。
- **增量 2 依赖 3C 通道并入 main** → 本增量把 `ChannelRepository` 仅留类型占位/接口，不接不声明 `worker:call`，确保增量 1 独立可合、可演示。
- **面板拿不到真实路径/数据** → 增量 1 全 fixture 不需路径；增量 2 经通道由 host 以会话绑定的 worktree（3C `SessionBinding.worktree_id`）解析路径并补强 readContext/`git` spawn。
- **"复用 i18n"无法在 opaque-origin 面板内实现** → 与 updater 一致：v1 英文内嵌；若后续需要，可在插件 bundle 内独立初始化 i18next 并复用共享资源（列为后续项，不阻塞增量 1）。
- design-token parity 漂移 → parity 测试硬门禁。

## 8. 分阶段路线

- **增量 1（本规格，main 独立）**：插件 UI 外壳 + 内联构建 + fixture + 视图分区 + parity/单测。零后端 / 零能力表改动。
- **增量 2（待 3C 通道入 main）**：`ChannelRepository` 经 `worker.invokeCommand`/`session.post`，worker `spawn git log`（复用现有只读语义）+ 补 worktree 路径透传；过滤/分页落地为真查询。
- **增量 3（后续可选）**：`--all` 全分支布局、分支 dashboard、blame 等，逐步参照 git4idea。
