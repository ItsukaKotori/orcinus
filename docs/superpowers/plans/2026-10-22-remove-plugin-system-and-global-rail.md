# 移除插件子系统 + 最左 GlobalActivityRail 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 物理删除整套插件/扩展子系统（含 `plugins/updater`、示例插件、插件贡献语言包）与最左 `GlobalActivityRail` 竖条，删除后应用不再有插件发现/加载/host/worker/面板/tab/命令/市场能力，也不再显示最左图标竖条。

**Architecture:** 方案 A —— 分层自底向上拆除。因 TS 程序由 `tsc -p tsconfig.json` 整体编译，删除顺序遵循「先删消费者（renderer/bridge），再删被引用模型（shared/preload）」的 DAG 方向；Rust 侧（`cargo test --workspace`）与 TS 侧门禁互相独立，单独成任务。核心枢纽文件采用外科手术式删分支，绝不整体删除被非插件代码共享的文件。

**Tech Stack:** Tauri 2 + Rust（`src-tauri/crates/*`，`tauri-specta` 命令登记）；React + TypeScript（renderer）；Zod 共享模型；Vitest；pnpm。

**Spec:** `docs/superpowers/specs/2026-10-22-remove-plugin-system-and-global-rail-design.md`

## Global Constraints

- **不做老数据迁移**（spec §2.7）：不写任何针对历史持久化数据（旧 `plugin` tab、旧 `activeView`、残留插件 settings 键）的迁移/scrub/回落代码。类型系统 + 既有 `isTopLevelView`/session schema 校验天然生效即可，脏数据被拒视为可接受。
- **保留红线（禁止触碰）**：原生 git-log（`contentType==='git-log'`、`tabs-git-log-actions.ts`、`git_log_query`/`git_refs`、`ade_git::history_with/list_refs`）；内置静态 locale `en/zh/ja/fr/ko/es`；`src/shared/skill-bundle-manifest.ts`（agent-skills 的 `plugin.json`）；第三方 `@tauri-apps/plugin-notification`、`@vitejs/plugin-react`；agent hooks（`ade-hooks`/`agent-hook-*`）、`ade-pty`、automations、GitHub 集成。
- **生成物不手改**：`src/bridge/real/generated/tauri-bindings.ts` 仅由 `cargo run -p ade-bridge --bin export-bindings`（在 `src-tauri/`）再生；新鲜度由测试 `specta_export::tests::bindings_are_fresh` 锁定。
- **门禁命令**：
  - TS-GATE（仓库根）：`pnpm typecheck && pnpm build:web && pnpm test`
  - RUST-GATE（`src-tauri/`）：`cargo test --workspace`
- **每任务末**：跑对应门禁直到全绿；再跑「悬空引用扫描」（见各任务）；一个可评审单元一次提交。
- **隔离**：实现前用 superpowers:using-git-worktrees 建立独立 worktree 分支，按任务提交，每任务为回退点。

**悬空引用扫描（通用，删除任务后用）：**
```bash
grep -rnE "shared/plugins|ade[-_]plugins|contentType === 'plugin'|activeView.*plugin|GlobalActivityRail|GlobalPluginHostView|pluginSystemEnabled|devPluginPaths|pluginConsents|disabledPlugins" src src-tauri/src src-tauri/crates/ade-bridge --include='*.ts' --include='*.tsx' --include='*.rs' | grep -v node_modules
```
命中应为空（`--include` 需引号，避免 zsh 展开通配符）。

---

## Task 1: 删除首方/示例插件与其构建引用

**Files:**
- Delete: `plugins/updater/**`（整目录）、`examples/plugins/**`（hello-orca、hostile-panel）
- Delete（本地未跟踪构建残留，可选）：`plugins/git-log/dist`
- Modify: `package.json:11`、`package.json:13`
- Modify: `vitest.config.ts`（删 `plugins/**/*.test.ts`、`plugins/**/*.test.tsx` include，约 L33–34）

- [ ] **Step 1: 删除 updater 与示例插件目录**
```bash
git rm -r plugins/updater examples/plugins
rm -rf plugins/git-log/dist   # 未跟踪，本地清理，不进 diff
```
- [ ] **Step 2: 改 `package.json`** — 删第 11 行整条 `"build:updater": "node plugins/updater/build.mjs",`；把第 13 行 `typecheck` 改为仅主 tsconfig：
```json
"typecheck": "tsc --noEmit -p tsconfig.json",
```
- [ ] **Step 3: 改 `vitest.config.ts`** — 删除 include 里的 `'plugins/**/*.test.ts',` 与 `'plugins/**/*.test.tsx',` 两行。
- [ ] **Step 4: 跑 TS-GATE** — `pnpm typecheck && pnpm build:web && pnpm test`。期望：全绿（updater 自带测试已随目录删除，不再被 include 收集）。
- [ ] **Step 5: 扫描 + 提交**
```bash
grep -rn "plugins/updater\|examples/plugins\|build:updater" package.json vitest.config.ts   # 期望空
git add -A package.json vitest.config.ts plugins examples
git commit -m "chore(remove-plugin): 删除 updater/示例插件与其构建测试引用"
```

---

## Task 2: 移除最左 GlobalActivityRail 竖条

复用现有导航（spec §7）：设置/帮助经 `sidebar/SidebarToolbar` 的 `SidebarSettingsHelpMenu`；离开设置经 `SettingsSidebar`「Back to app」+ Esc。竖条独占的「跨全页面一键切换」能力放弃。

**Files:**
- Delete: `src/renderer/src/components/global-rail/`（整目录：`GlobalActivityRail.tsx`+test、`global-rail-items.ts`、`GlobalPluginHostView.tsx`+test）
- Modify: `src/renderer/src/app-shell/AppWorkspaceShell.tsx`（删 L6–7 import、L113 `<GlobalActivityRail/>`、L74–76 `activeView.startsWith('plugin:')` → `<GlobalPluginHostView/>` 分支）

- [ ] **Step 1: 删 AppWorkspaceShell 的竖条接线** — 移除两行 import 与 `<GlobalActivityRail />`；移除 `ActivePage` 中：
```tsx
{activeView.startsWith('plugin:') ? (
  <GlobalPluginHostView pluginKey={activeView.slice('plugin:'.length)} />
) : null}
```
（`PluginCenterPage` 的 lazy import 与 `activeView === 'plugin-center'` 分支留到 Task 3。）
- [ ] **Step 2: 删整目录** `git rm -r src/renderer/src/components/global-rail`
- [ ] **Step 3: 跑 TS-GATE** — `pnpm typecheck && pnpm test`。若报 `GlobalActivityRail`/`GlobalPluginHostView` 未解析，说明仍有残留引用，回到 Step 1 清理。
- [ ] **Step 4: 扫描 + 提交**
```bash
grep -rn "global-rail\|GlobalActivityRail\|GlobalPluginHostView" src/renderer | grep -v node_modules   # 期望空
git add -A && git commit -m "chore(remove-plugin): 移除最左 GlobalActivityRail，复用现有导航"
```

---

## Task 3: 删除 renderer 插件 UI 面 + 核心枢纽插件分支

删所有「叶子」插件 UI，并同步删其在核心枢纽里的引用，使这些 UI 变为无引用后再删。

**Files（Delete，整叶）：**
- `src/renderer/src/components/plugin-center/**`
- `src/renderer/src/components/plugin-catalog/**`
- 右侧栏插件面：`src/renderer/src/components/right-sidebar/` 下 `PluginPanel.tsx`(+test)、`plugin-panel-*.ts(x)`、`use-plugin-panel-*.ts(x)`、`use-installed-plugin-route-reconciliation.ts`
- tab UI：`src/renderer/src/components/tabs/PluginTabContent.tsx`(+test)、`src/renderer/src/components/tab-bar/PluginTabLeadingIcon.tsx`、`TabBarPluginTabMenuItems.tsx`
- 设置 UI：`src/renderer/src/components/settings/` 下全部 `Plugin*.tsx`、`PluginMarketplace*`、`PluginConsent*`、`PluginInstall*`、`PluginRemove*`、`PluginSettings*`、`PluginVmRecipe*`、`PluginKeybinding*`、`PluginsSettingsSection*`，及 `plugin-install-source.ts`、`plugin-capability-presentation.ts`、`plugin-error-presentation.ts`、`plugins-search.ts`、`plugin-ref.ts`（含各自 test）
- 命令：`src/renderer/src/lib/plugin-command-execution.ts`(+test)、`plugin-command-keybindings.ts`(+test)
- cmd-j：`src/renderer/src/components/cmd-j/plugin-quick-actions.ts(x)`(+test)
- 精确枚举删除集：
```bash
git rm -r src/renderer/src/components/plugin-center src/renderer/src/components/plugin-catalog
git rm -r $(grep -rlE "plugin|Plugin" src/renderer/src/components/right-sidebar src/renderer/src/components/settings src/renderer/src/components/tabs src/renderer/src/components/tab-bar src/renderer/src/lib src/renderer/src/components/cmd-j --include='*Plugin*' --include='*plugin*' | grep -iE "plugin" | tr '\n' ' ')
```
> 注：删除命令先 `echo` 预览文件列表再执行，避免误删同名非插件文件。`settings/` 内只删 `Plugin*`/`plugin-*` 前缀文件，勿碰其它设置文件。

**Files（Modify，核心枢纽删分支）：**
- `src/renderer/src/app-shell/AppWorkspaceShell.tsx`：删 `PluginCenterPage` lazy import（L23–27）+ `activeView === 'plugin-center'` 分支（L73）。
- `src/renderer/src/store/index.ts`：删 `createPluginCenterSlice` import（L38）+ `...createPluginCenterSlice(...a)`（L89，注意收尾逗号）。
- `src/renderer/src/app-shell/use-global-keybindings.ts`：删 4 处 import（`executePluginCommand`、`findPluginCommandForKeybinding`、`usePluginCommands`、`PLUGIN_COMMAND_ALIAS_ACTION_IDS`）+ `pluginCommands` 选取/传参 + `findPluginCommandForKeybinding` 调用块（约 L220–232）。
- `src/renderer/src/components/settings/shortcut-definition-catalog.ts`：删 `ActivePluginCommand`/`buildPluginCommandKeybindingDefinitions` import + `pluginCommands` 选项 + `pluginDefinitions` 生成分支（约 L13–47）。
- `src/renderer/src/store/right-sidebar-route.ts`：删 `isPluginPanelTabKey` import + 插件 tab key 校验分支（约 L5、L17–35）。
- `src/renderer/src/components/right-sidebar/right-sidebar-panel-content.tsx`：删 `PluginPanel` import + 渲染分支。
- `src/renderer/src/components/tab-group/TabGroupPanel.tsx`：删插件 tab 渲染分支，并从末尾编辑器 fallback 的 `contentType !== 'plugin'` 条件里移除 `'plugin'`。

- [ ] **Step 1: 预览删除清单** — 先跑上面 `git rm` 的 `echo` 版（把 `git rm` 换成 `echo`），人工核对无「非插件同前缀」误伤后执行。
- [ ] **Step 2: 逐一编辑核心枢纽**（上方 Modify 清单），删净对已删 UI 的 import 与分支。
- [ ] **Step 3: 跑 TS-GATE** — `pnpm typecheck && pnpm build:web && pnpm test`。typecheck 会逐个报出仍引用已删文件的残留点，就地清理直到通过。
- [ ] **Step 4: 提交** `git add -A && git commit -m "chore(remove-plugin): 删除 renderer 插件 UI 面并清理核心枢纽分支"`

---

## Task 4: 移除插件贡献语言包通路（保留内置 6 语言）

**Files（Delete）：**
- `src/renderer/src/store/plugin-language-packs.ts`(+test)
- `src/renderer/src/i18n/plugin-chrome-allowlist.test.ts`

**Files（Modify）：**
- `src/renderer/src/i18n/i18n.ts`：删 `PluginLanguagePackRegistration` import、`pluginLanguagePacks` 状态、`setRendererPluginLanguagePacks` 导出、`resourceLanguage` 回落逻辑；保留内置 locale 选择/`Intl` 处理。
- `src/renderer/src/i18n/I18nProvider.tsx`：删 `usePluginLanguagePacks` 订阅与注入 effect（L8、L16–17、L26–28、L39）。
- `src/renderer/src/hooks/useMacosTccPromptNotice.ts` + `useMacTccAttributionSeveredNotice.ts`：删 `usePluginLanguagePackStore`/`isPluginUiLanguage` 相关等待条件（保留内置 locale 判断），连带各自 test 里的插件用例。
- `src/shared/ui-locale.ts` + `src/renderer/src/lib`? 内 `ui-language.ts`：删 `isPluginUiLanguage` 等插件语言辅助与其导出引用。
- `src/renderer/src/components/editor/rich-markdown-code-block-languages.test.ts`：删「plugin replaces resource bundle」用例（依赖 `pluginLanguageResourceId`）。

- [ ] **Step 1: 编辑上述 Modify 文件**，去除插件语言包注入；保留内置逻辑。
- [ ] **Step 2: 删除 Delete 文件**：`git rm src/renderer/src/store/plugin-language-packs.ts src/renderer/src/store/plugin-language-packs.test.ts src/renderer/src/i18n/plugin-chrome-allowlist.test.ts`
- [ ] **Step 3: 跑 TS-GATE** — `pnpm typecheck && pnpm test`；就地修残留引用直到全绿。
- [ ] **Step 4: 提交** `git add -A && git commit -m "chore(remove-plugin): 移除插件语言包注入通路，保留内置静态 locale"`

---

## Task 5: 删除 bridge 插件接线

**Files（Delete）：**
- `src/bridge/real/plugins.ts`(+test)、`src/bridge/mock/plugins-api.ts`(+test)

**Files（Modify，删插件成员）：**
- `src/bridge/create-api.ts`：删 plugins 装配。
- `src/bridge/create-api.test.ts`、`src/bridge/real/parity.test.ts`：删 plugins 相关断言/键集。
- `src/bridge/mock/fixtures.ts`、`src/bridge/mock/settings-api.ts`、`src/bridge/mock/worktrees-api.ts`：删插件字段/引用（若仅字段名撞车则保留，按 typecheck 报错定位）。

- [ ] **Step 1: 删两个插件 bridge 文件** `git rm src/bridge/real/plugins.ts src/bridge/real/plugins.test.ts src/bridge/mock/plugins-api.ts src/bridge/mock/plugins-api.test.ts`
- [ ] **Step 2: 清理 `create-api.ts`/`create-api.test.ts`/`parity.test.ts`/`fixtures.ts` 的插件成员**。
- [ ] **Step 3: 跑 TS-GATE** — `pnpm typecheck && pnpm test`。
- [ ] **Step 4: 提交** `git add -A && git commit -m "chore(remove-plugin): 删除 bridge 插件接线（real/mock）"`

---

## Task 6: 删除共享 TS 模型 + tab/视图/settings 联合类型变体（TS 侧收尾）

此时所有 TS 消费者已删净，可安全删除被引用模型与联合分支，达成 TS 全绿。

**Files（Delete）：**
- `src/shared/plugins/**`（整目录 45 文件）
- `src/shared/preload-api/api/plugin-host-api.ts`
- `src/renderer/src/store/plugin-panels.ts`(+test)
- `src/renderer/src/store/slices/plugin-center/**`
- `src/renderer/src/store/slices/tabs/tabs-plugin-actions.ts`(+test)
- 精确：`git rm -r src/shared/plugins src/renderer/src/store/slices/plugin-center`

**Files（Modify）：**
- `src/shared/preload-api/api/app-api.ts` + `src/shared/preload-api/api-types.ts` + `src/preload/api-types.ts`：删 `plugins` 命名空间成员与 `plugin-host-api` 引用。
- `src/shared/tab-types.ts`：删 `TabContentType` 的 `'plugin'` 变体、`PluginTabBinding` 类型、`Tab.pluginTab?` 字段、`toVisibleTabType` 内 `contentType === 'plugin'` 分支。
- `src/shared/workspace-session-tab-type-schema.ts`：`workspaceVisibleTabTypeSchema` 去 `'plugin'`。
- `src/shared/ui-chrome-types.ts`：`TopLevelView`/`StaticTopLevelView` 去 `'plugin-center'` 与 `plugin:` 形态。
- `src/shared/top-level-view.ts`：`TOP_LEVEL_VIEW_LOOKUP`（穷尽 Record）去 `'plugin-center'`；删 `PLUGIN_VIEW_RE` 及其在 `isTopLevelView` 的分支。
- `src/renderer/src/app-shell/use-app-chrome-layout.ts`：`isProjectView` 化简（移除对 `plugin-center`/`plugin:` 判定）。
- `src/shared/global-settings-types.ts` + `src/shared/default-global-settings.ts`：删 `pluginSystemEnabled`/`disabledPlugins`/`pluginConsents`/`devPluginPaths` 四字段（type + default）。
- tabs 内部：`tabs-slice-contract.ts`/`tabs-create-actions.ts`（`pluginTab` 出 `Pick` 白名单 + 逐字段复制）、`tabs-reconciliation.ts`（删 `contentType === 'plugin'` 分支，**保留 `'git-log'`**）、`create-tabs-slice.ts`（去 `tabs-plugin-actions` 装配）、`tabs-close-actions.ts`/`tabs-secondary-actions.ts`（去插件分支）。
- 关联 store/test 与 i18n 引用：`store/types.ts`、`recently-closed-tabs.ts`、`tab-number-shortcuts`/`workspace-tab-palette-*` 里对 `'plugin'` 的显式列举（非穷尽比较，按 typecheck 定位）。

- [ ] **Step 1: 删 Delete 文件/目录**。
- [ ] **Step 2: 编辑 Modify 文件**，删联合变体、插件字段、tabs 装配与分支。
- [ ] **Step 3: 跑 TS-GATE** — `pnpm typecheck && pnpm build:web && pnpm test`（**这是 TS 侧决定性全绿点**；typecheck 逐一暴露残留引用，就地清理直到全绿）。
- [ ] **Step 4: 提交** `git add -A && git commit -m "chore(remove-plugin): 删除共享 TS 模型与 plugin tab/视图/settings 联合变体"`

---

## Task 7: 删除 Rust 插件子系统（ade-plugins + ade-bridge 接线）

Rust 侧独立门禁。先删 ade-bridge 消费者，再删 crate。

**Files（Delete）：**
- `src-tauri/crates/ade-plugins/`（整 crate）
- `src-tauri/crates/ade-bridge/src/commands/plugins.rs`、`src-tauri/crates/ade-bridge/src/plugins_service.rs`
- `src-tauri/crates/ade-bridge/tests/plugins.rs`、`src-tauri/crates/ade-bridge/tests/plugins_service.rs`
- `src-tauri/resources/plugins/host-entry.mjs`（+ 空目录 `src-tauri/resources/plugins/`）

**Files（Modify）：**
- `src-tauri/crates/ade-bridge/src/commands/mod.rs`：删 `pub mod plugins;`
- `src-tauri/crates/ade-bridge/src/lib.rs`：删 `mod plugins_service;` + 插件 state/初始化/事件相关项
- `src-tauri/crates/ade-bridge/src/specta_export.rs`：删 16 条 `commands::plugins::*` 注册（L154–169）+ `export_lists_every_command`/命令 inventory 里的插件命令名
- `src-tauri/crates/ade-bridge/src/state.rs` + `events.rs`：删插件字段/`shutdown_plugins`/`plugins:changed`/`plugins:open-tab` 相关
- `src-tauri/crates/ade-bridge/Cargo.toml`：删对 `ade-plugins` 的依赖
- `src-tauri/Cargo.toml`：workspace members 去 `crates/ade-plugins`
- `src-tauri/src/lib.rs`：删 `state.shutdown_plugins();`（L76 + 其上注释）
- `src-tauri/tauri.conf.json`：删 `"resources/plugins/": "plugins/"`（L19）

- [ ] **Step 1: 编辑 ade-bridge 消费者**（mod/lib/specta_export/state/events/Cargo），移除对 `ade-plugins`/插件命令/`plugins_service` 的全部引用。
- [ ] **Step 2: 删文件/目录**
```bash
git rm src-tauri/crates/ade-bridge/src/commands/plugins.rs src-tauri/crates/ade-bridge/src/plugins_service.rs
git rm -r src-tauri/crates/ade-bridge/tests/plugins.rs src-tauri/crates/ade-bridge/tests/plugins_service.rs
git rm src-tauri/resources/plugins/host-entry.mjs
git rm -r src-tauri/crates/ade-plugins
```
- [ ] **Step 3: 编辑 `src-tauri/src/lib.rs` + `tauri.conf.json`**（上方 Modify 清单）。
- [ ] **Step 4: 再生绑定并跑 RUST-GATE**
```bash
cd src-tauri && cargo run -p ade-bridge --bin export-bindings && cargo test --workspace
```
期望：`bindings_are_fresh`、`export_lists_every_command` 通过，全 workspace 绿。
- [ ] **Step 5: 提交**
```bash
cd /Users/itsuka/CodeSpace/ade
git add -A && git commit -m "chore(remove-plugin): 删除 ade-plugins crate 与 ade-bridge 插件接线，再生 bindings"
```

---

## Task 8: 全量收尾校验 + locale 清理

- [ ] **Step 1: 全量 TS-GATE** — `pnpm typecheck && pnpm build:web && pnpm test`。
- [ ] **Step 2: 全量 RUST-GATE** — `cd src-tauri && cargo test --workspace`。
- [ ] **Step 3: 悬空引用扫描**（Global Constraints 的 grep 块）——命中为空。
- [ ] **Step 4: locale 清理（仅整洁，不影响门禁）** — 删除 en/zh/ja/fr/ko/es JSON 与 `config/scripts/locale-ko-key-overrides.json` 中仅被已删组件/竖条引用的插件 key（`auto.*.Plugin*`、`auto.*.plugin*`、竖条 `全局导航`/项目/插件中心 标签）。逐 key grep 确认无存活引用后再删；内置 6 语言与共享 key 保留。
```bash
grep -rn "\"plugin\|Plugin" src/renderer/src/i18n/locales/en.json | head
```
- [ ] **Step 5: 最终门禁**（若 Step 4 动了 JSON，重跑 `pnpm test` 确保 locale 测试绿）。
- [ ] **Step 6: 提交** `git add -A && git commit -m "chore(remove-plugin): 收尾校验与插件 locale 清理"`

---

## Self-Review（计划对照 spec）

- **spec §3.1 纳入** → Task 1–8 全覆盖：updater/示例(T1)、renderer UI(T3)、shared(T6)、bridge(T5)、Rust(T7)、竖条(T2)、语言包(T4)、构建/生成物(T1/T7)。
- **spec §3.2 保留红线** → 写进 Global Constraints，每任务门禁 + grep 兜底，`'git-log'` 分支在 T6 显式保留。
- **spec §6 穷尽 Record** → T6 `top-level-view.ts`/`ui-chrome-types.ts` 处理；T7 `specta_export` 命令清单处理。
- **spec §7 导航替代** → T2 显式说明依赖 `SidebarSettingsHelpMenu` + `SettingsSidebar` Back。
- **spec §9 门禁** → 每任务末 TS-GATE/RUST-GATE + grep；T8 全量。
- **无占位符**：删除清单、编辑点、命令、期望结果均具体；文件枚举以 `grep`/`typecheck` 现场定位，避免依赖不完整列表。
