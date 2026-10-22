# 设计：移除插件子系统 + 最左 GlobalActivityRail

- 日期：2026-10-22
- 状态：待用户 review
- 落点：`/Users/itsuka/CodeSpace/ade`（实现应在独立 worktree 分支进行，见 §9）

## 1. 背景与目标

Orcinus（ade）内置一套「插件/扩展子系统」：表驱动的 manifest + capability 模型、out-of-process Node worker、沙箱面板 iframe、市场/安装、插件贡献的 tab/命令/快捷键/语言包，由 Rust crate `ade-plugins` + `ade-bridge` 接线 + TS `src/shared/plugins/**` + renderer 插件面 + bridge 组成。总开关 `pluginSystemEnabled` 默认关闭。

同时最左侧有一条 ~40px 的常驻图标竖条 `GlobalActivityRail`，其入口为：项目 / 插件中心 / 每个全局插件 / 底部 设置·帮助。

**目标**：物理删除整套插件子系统，并删除最左 `GlobalActivityRail` 竖条。删除后应用不再有任何插件发现/加载/host/worker/面板/tab/命令/市场能力，也不再显示最左图标竖条。

## 2. 关键决策记录（已与用户确认）

1. **拆除深度**：全量物理拆除（Rust + 共享 TS + bridge + renderer UI + 示例/首方插件）。
2. **`plugins/updater`**：它是「npm/brew 包更新器」（*非* 应用自更新器），**直接删除、不迁原生**。
3. **原生功能**：**保留**（原生 git-log tab + 原生 git 命令）。
4. **插件贡献的语言包**：**一并移除**动态注入通路；内置静态 locale 保留。
5. **实施顺序**：方案 A —— 分层自底向上，每层末以门禁为安全网。
6. **`GlobalActivityRail`**：**整条竖条直接删除**，不新建常驻导航；复用现有入口（§7）。
7. **老数据平滑降级**：**无需考虑**。不新增任何历史持久化数据（会话/settings/右侧栏路由）的迁移、scrub 或回落代码；仅依赖类型系统与既有校验天然生效。

## 3. 范围

### 3.1 纳入删除（In scope）
- Rust：`ade-plugins` 整 crate；`ade-bridge` 插件接线；`src-tauri/src/lib.rs` 收尾调用；`tauri.conf.json` resources 映射；`resources/plugins/host-entry.mjs`。
- 共享 TS：`src/shared/plugins/**`；preload-api 插件成员；tab 系统的 `'plugin'` 变体；视图模型的 `'plugin-center'`/`plugin:`；`isPluginUiLanguage` 等插件语言包辅助；4 个插件 settings 字段。
- Bridge：`real/plugins.ts`、`mock/plugins-api.ts` 及其在 `create-api.ts`/`fixtures.ts`/parity 测试中的接线。
- Renderer：插件中心/目录/设置里全部 `Plugin*` UI、右侧栏插件面板、插件 tab、插件命令/快捷键、插件 store 切片、插件语言包 store、i18n 插件语言包通路。
- `GlobalActivityRail` 及其子组件/测试。
- 首方/示例插件：`plugins/updater/**`、`examples/plugins/**`。（`plugins/git-log/dist` 为 git 未跟踪的构建残留，本地清理即可，非仓库删除项）
- 构建/配置：`package.json`、`vitest.config.ts`、生成的 tauri 绑定（根 `tsconfig.json`/`config/tsconfig.web.json` 已核实无插件引用）。

### 3.2 明确保留（Out of scope / 防误伤）
- 原生 git-log tab（`contentType==='git-log'`）与原生 git 命令（`git_log_query`/`git_refs`、`ade_git::history_with/list_refs`）。
- 内置静态 locale：`en/zh/ja/fr/ko/es`（仅移除「插件可注入 locale」通路）。
- `src/shared/skill-bundle-manifest.ts`：其 `plugin.json`/`agent-plugins.org` 属 **agent-skills**，与插件子系统无关（命名撞车，勿动）。
- 第三方依赖：`@tauri-apps/plugin-notification`、`@vitejs/plugin-react`（仅名字含 "plugin"）。
- agent hooks（`ade-hooks`/`agent-hook-*`）、terminal/pty（`ade-pty`）、automations、GitHub 集成等非插件能力。

## 4. 拆除清单（方案 A：按层，自底向上）

> 每层结束应达成一个可编译绿点；破坏性删除前先 grep 确认无其它调用方。

**层① 首方/示例插件**
- `plugins/updater/**`（含 `ade-plugin.json`、`build.mjs`、`src/**`、`tsconfig.*`）
- `examples/plugins/**`（hello-orca、hostile-panel）
- （本地 `plugins/git-log/dist` 为未跟踪构建残留，随手 `rm` 清理，不进 diff）

**层② renderer 插件面**
- 目录：`components/plugin-center/**`、`components/plugin-catalog/**`、`components/global-rail/**`（并入层②/§7）
- 右侧栏：`components/right-sidebar/PluginPanel.tsx`、`plugin-panel-*.ts(x)`、`use-plugin-panel-*.ts`、`use-installed-plugin-route-reconciliation.ts`、`right-sidebar-panel-content.tsx` 插件分支
- tab：`components/tabs/PluginTabContent.tsx`、`components/tab-bar/PluginTabLeadingIcon.tsx`、`TabBarPluginTabMenuItems.tsx`
- 设置：`components/settings/Plugin*`（Marketplace/Install/Consent/Remove/Row/Overview/Section/Dialog 等）、`plugin-install-source.ts`、`plugin-capability-presentation.ts`、`plugin-error-presentation.ts`、`plugins-search.ts`、`plugin-ref.ts`
- 命令/快捷键：`lib/plugin-command-execution.ts`、`lib/plugin-command-keybindings.ts`
- cmd-j：`components/cmd-j/plugin-quick-actions.ts(x)`
- store：`store/plugin-panels.ts`、`store/plugin-language-packs.ts`、`store/slices/plugin-center/**`

**层③ 共享 TS 模型 + preload**
- `src/shared/plugins/**`（45 文件）
- `src/shared/preload-api/api/plugin-host-api.ts`；`app-api.ts`/`api-types.ts` 插件成员

**层④ bridge**
- `src/bridge/real/plugins.ts`、`src/bridge/mock/plugins-api.ts`
- `src/bridge/create-api.ts`、`mock/fixtures.ts`、`real/parity.test.ts`、`create-api.test.ts` 的插件接线

**层⑤ Rust**
- `src-tauri/crates/ade-plugins/`（整 crate：`host/`、`install/`、`marketplace/`、`worker/`、`tests/` 等）
- `ade-bridge`：`commands/plugins.rs`、`plugins_service.rs`（整文件）；`commands/mod.rs` 模块声明；`specta_export.rs` 16 条 `commands::plugins::*` 注册；`state.rs`/`events.rs`/`lib.rs` 插件成员（含 `shutdown_plugins`）；`Cargo.toml` 对 `ade-plugins` 依赖；`tests/plugins.rs`、`tests/plugins_service.rs`
- `src-tauri/src/lib.rs`：`state.shutdown_plugins()` 调用
- `src-tauri/tauri.conf.json`：`"resources/plugins/": "plugins/"` 映射
- `src-tauri/resources/plugins/host-entry.mjs`

**层⑥ 构建/配置/生成物**
- `package.json`：删 `build:updater`；`typecheck` 去掉两条 updater tsconfig；核对其它引用
- `vitest.config.ts`：去 `plugins/**/*.test.ts(x)` include
- 根 `tsconfig.json` / `config/tsconfig.web.json`：已核实无插件目录引用，无需改动
- 再生 `src/bridge/real/generated/tauri-bindings.ts`（删除后重跑既有生成流程，勿手改）

## 5. 核心枢纽「外科手术」（删分支/变体，不整体删文件）

- **`src/shared/tab-types.ts`**：删 `TabContentType` 的 `'plugin'` 变体、`PluginTabBinding` 类型、`Tab.pluginTab?` 字段、`toVisibleTabType` 内 `contentType === 'plugin'` 分支。
- **`src/shared/workspace-session-tab-type-schema.ts`**：`workspaceVisibleTabTypeSchema` 去 `'plugin'`（`tabContentTypeSchema` 本就不含，无需动）。
- **`tabs-reconciliation.ts`**：删 `tab.contentType === 'plugin'` 分支（保留 `'git-log'` 分支）。
- **`tabs-slice-contract.ts` / `tabs-create-actions.ts`**：`pluginTab` 从 `Pick<Tab,...>` 白名单与逐字段复制中移除。
- **`create-tabs-slice.ts` / `tabs-plugin-actions.ts` / `tabs-close-actions.ts` / `tabs-secondary-actions.ts`**：去插件 tab action 装配与分支。
- **`store/index.ts`**：删 `createPluginCenterSlice` 的 import 与 spread。
- **`components/tab-group/TabGroupPanel.tsx`**：删插件渲染分支，并把渲染分派末尾编辑器 fallback 的 `contentType !== 'plugin'` 条件移除。
- **`store/right-sidebar-route.ts`**：删 `isPluginPanelTabKey` 校验与插件 tab key 分支。
- **`app-shell/use-global-keybindings.ts` / `components/settings/shortcut-definition-catalog.ts`**：删插件命令查找/定义生成分支。
- **`app-shell/AppWorkspaceShell.tsx`**：删 `<GlobalActivityRail/>` 渲染 + `GlobalActivityRail`/`GlobalPluginHostView` import；删 `ActivePage` 内 `activeView==='plugin-center'`、`activeView.startsWith('plugin:')` 分支。

## 6. 视图模型穷尽点（会触发编译错误，需逐处收敛）

- **`src/shared/ui-chrome-types.ts`**：`TopLevelView`/`StaticTopLevelView` 去 `'plugin-center'` 与动态 `plugin:` 形态。
- **`src/shared/top-level-view.ts`**：`TOP_LEVEL_VIEW_LOOKUP: Record<StaticTopLevelView, true>` 是**穷尽 Record**（删项即编译报错）→ 去 `'plugin-center'`；删 `PLUGIN_VIEW_RE` 与其在 `isTopLevelView` 的分支。
- **`app-shell/use-app-chrome-layout.ts`**：`isProjectView`（原 `!(activeView==='plugin-center' || activeView.startsWith('plugin:'))`）化简为常量语义并移除相关引用。
- **activeView 持久化/IPC 校验**：依赖 `isTopLevelView` 天然拒绝已删值即可（§2.7 不写额外回落）。

## 7. GlobalActivityRail 移除与导航替代

竖条删除后，其入口去向：
- **插件中心 / 全局插件图标**：随子系统消失，无需替代。
- **设置 / 帮助**：项目视图仍由 `components/sidebar/SidebarToolbar.tsx` → `SidebarSettingsHelpMenu`（常驻「设置/帮助」按钮）进入。
- **返回项目 / 离开设置**：Settings 页自带 `SettingsSidebar`「Back to app」按钮 + Esc（`use-settings-page-effects.ts`）；`space` 视图由 `ui-slice-view-actions` 的 `previousViewBeforeSpace` 自行进出。
- **取舍**：放弃「在全页面视图之间直接一键切换」的竖条独占能力（用户改为「先退出再进入」）。

删除文件：`components/global-rail/GlobalActivityRail.tsx`(+test)、`global-rail-items.ts`、`GlobalPluginHostView.tsx`(+test)。竖条 i18n 文案（含 `全局导航`）随删。

## 8. i18n 语言包通路移除

移除「插件贡献 locale」的动态注入，保留内置 6 语言：
- `src/renderer/src/i18n/i18n.ts`：删 `pluginLanguagePacks` 状态、`setRendererPluginLanguagePacks`、按 `resourceLanguage` 回落逻辑。
- `src/renderer/src/i18n/I18nProvider.tsx`：删 `usePluginLanguagePacks` 订阅与注入 effect。
- `hooks/useMacosTccPromptNotice.ts` / `hooks/useMacTccAttributionSeveredNotice.ts`：删 `usePluginLanguagePackStore`/`isPluginUiLanguage` 相关等待条件（保留内置 locale 判断）。
- `src/shared/ui-locale.ts` / `ui-language.ts`：删 `isPluginUiLanguage` 等插件语言辅助。
- `store/plugin-language-packs.ts`：整文件删（层②）。
- 相关测试：`i18n/plugin-chrome-allowlist.test.ts`、`i18n/lazy-locale.test.ts`、`i18n/intl-locale.test.ts`、`components/editor/rich-markdown-code-block-languages.test.ts`（其插件语言包用例随删）、`store/plugin-language-packs.test.ts`。
- locale JSON（en/zh/ja/fr/ko/es）与 `config/scripts/locale-ko-key-overrides.json`：删插件 key（`auto.*.Plugin*`、`plugin.*` 命名空间、竖条 key）。

## 9. 实施与验证

- **隔离**：在独立 worktree 分支实施（遵循本仓 worktree 增量惯例），按层提交，每层为回退点。
- **门禁（每层末全绿后方可继续）**：
  - `pnpm typecheck`
  - `pnpm build:web`
  - `cargo test --workspace`（EXIT 0）
  - 全量 `pnpm test`（0 失败）
  - grep 确认全仓零悬空引用：`shared/plugins`、`ade-plugins`/`ade_plugins`、`contentType === 'plugin'`、`activeView.*plugin`、`GlobalActivityRail`、`pluginSystemEnabled` 等。

## 10. 风险与注意事项

- **最高风险面：i18n 插件语言包与核心 locale 耦合**（§8）——须确保移除动态注入后内置 6 语言与 `Intl` 回落仍正确。
- **穷尽 `Record<StaticTopLevelView, true>`**（§6）是少数会硬编译失败的点，需与其它 `TopLevelView` 消费点一并对齐。
- **生成绑定** `tauri-bindings.ts` 必须走既有生成流程重生成，不手改。
- **误伤红线**：`skill-bundle-manifest.ts`（agent-skills `plugin.json`）、第三方 `@tauri-apps/plugin-notification` / `@vitejs/plugin-react` 不得动。
- **不做老数据迁移**（§2.7）：历史脏数据（旧插件 tab/旧 activeView/残留插件键）不写专门处理代码；若天然校验导致旧值被拒即按现状回落，视为可接受。

## 11. 交付

本设计经用户 review 通过后，进入 `writing-plans` 产出可执行的实现计划（逐任务 + 每层门禁/回退点 + grep 核对表），再按计划实施。
