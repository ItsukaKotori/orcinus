# Phase 4 · Git Log 插件（增量 1：fixture UI）实施记录

日期：2026-10-18 · 分支：`phase4-git-log-graph`（自 `main@e11d9cda`）· 规格：`docs/superpowers/specs/2026-10-18-git-log-plugin-design.md` · 计划：`docs/superpowers/plans/2026-10-18-git-log-plugin.md`

## 交付内容
IDEA VcsLog 风格的**独立主区提交图谱**，以**插件**形态交付：`plugins/git-log/`（React 19 + Tailwind v4，vite 单入口全内联 `dist/tab.html`；右侧栏 `launcher.html` 调 `workspace.openTab`）。增量 1 数据来自内嵌确定性 fixture（31 提交，含 merge/fork/tag/remote/ahead-behind）。

## 隔离自证（spec §2.2 硬约束）
`git diff --name-only main...HEAD` 仅落在：`plugins/git-log/**`、`package.json`、`vitest.config.ts`、`.gitignore`、`docs/**`。
**未触碰**：插件能力表 `plugin-capability-gate(s).ts` / `plugin-capabilities.ts` / `plugin-host-api.ts`、Rust `ade-plugins` `host/methods.rs`·`service.rs`、现有 Source Control 组件、`src/shared/git-history*`。
→ **零 Rust 改动，`cargo test --workspace` 与 `main` 等价，故未重跑。**

## 门禁
- `pnpm typecheck` ✅（含 `plugins/git-log/tsconfig.json`）
- `pnpm build:web` ✅（渲染层产物正常）
- `pnpm build:git-log` ✅ → `dist/tab.html` 全内联（CSP：无外链/fetch/import，字体 data: URI）
- 插件测试 ✅（build-inline / theme-parity / snapshot-shape / git-graph-model / select-rows / format / launcher / git-log-view）

## 复用策略
- **纯逻辑复用**：`src/shared/git-history-graph.ts`（`buildGitHistoryViewModels`/`getGitHistoryItemLaneIndex`）只读复用。
- **几何/配色复用（最终审查修正 I-1）**：主视图 SVG 复用宿主 `git-history-graph-svg.tsx`（仅依赖 React + `src/shared`，且 `var(--git-graph-*)` 与插件 token 对齐），避免维护第二套（错误的）泳道几何；ref chips 复用 `@shared/git-history-ref-display` 的 `dedupeRemoteTrackingRefs`。**不 import 含 Radix/i18n 的 `GitHistoryRow`**。

## 遗留与增量归属（非静默缺口）
- **spec §3 未落地项**（增量 1 有意简化）：工具条分支选择器、RefPanel 折叠、Enter 打开提交。行内 ref chips、详情区 date/message 正文、`displayId ?? id` 已在最终审查补齐。Date/Path filter chips 顺延增量 2（`GitHistoryItem` 不带文件路径，Path 无真实数据可过滤）。
- **异步错误/加载态（M-2）**：fixture 恒不 reject，未加 try/catch；增量 2 接 `ChannelRepository` 前必补。
- **manifest 命名**：已对齐 phase3c updater 的规范名 **`ade-plugin.json`**（rebase 入新 main 时统一；Rust dev discovery 同时接受 legacy `orca-plugin.json`，二者皆可加载）。
- **构建 Node 版本**：`build:git-log` 已去掉 `--experimental-strip-types`，与 `build:updater` 同为纯 `node`（Node ≥ 22.18 默认启用类型擦除）。

## 与 phase3c 的合并协调（I-5）
- `package.json` 的 `typecheck` 行、`vitest.config.ts` 的 `include`/alias 两侧都被改写 → **是必然文本冲突，非"行级可并存"**。合并顺序建议：先 phase3c（updater 的 `build:updater` + worker/ui 两串 tsc），本分支 rebase 后把 `typecheck` 拼接为：`tsc -p tsconfig.json && tsc -p plugins/updater/tsconfig.worker.json && tsc -p plugins/updater/tsconfig.ui.json && tsc -p plugins/git-log/tsconfig.json`；`include` 合并 `plugins/**`；`build:git-log` 与 `build:updater` 并列。
- flaky：全量并行下 3 个与本改动无关的渲染层时序测试报 ~590s 超时（单跑 25 用例 <6s 全过），判定为环境争用；合并 commit 复跑记档。

## 手工验收（用户在 tauri dev 执行）
1. `pnpm build:git-log` → `pnpm dev`（真实桥）→ 设置→插件：**开 `pluginSystemEnabled` 总开关** → `devPluginPaths` 指 `plugins/git-log` → 插件中心启用（consent 仅 `workspace:open-tab`）。
2. 右侧栏 "Git Log" 启动器 → 点 → 主区 tab 打开图谱/refs/详情。
3. 搜索过滤、点 refs 过滤、↑/↓ 选提交、选中显示详情（date/message/files）；light/dark 与宿主同屏对照；≥30 行虚拟滚动流畅。
