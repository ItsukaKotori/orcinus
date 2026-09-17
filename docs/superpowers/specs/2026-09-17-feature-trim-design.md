# Orcinus 功能删减设计（Phase 1 瘦身）

- 日期：2026-09-17
- 状态：brainstorming 输出，待用户审阅
- 基线：`main` @ `0cf68df`（Phase 0 收口后）
- 相关文档：`docs/superpowers/specs/2026-09-14-ade-design.md`、`docs/phase0-dead-code-inventory.md`
- 下游：本规格审阅通过后使用 writing-plans 技能编写实施计划

## 1. 背景与目标

渲染层整包 fork 自 Orca，约 161 万行 TS，包含大量本项目不需要的功能面。本次任务从代码库中**彻底删除**以下 10 个功能域（用户选定 9 项 + 追加 Setup Guide）：

Pet、Contextual Tours、Feature Tips、Feature Wall、Setup Guide、Dictation、Emulator Pane、Activity/Dashboard、Native Chat、Telemetry。

「彻底删除」定义：生产代码 + 测试 + mock bridge 域 + preload 契约 + store 注册/slice + 设置项与导航/搜索条目 + i18n 键 + 仅被删域使用的依赖。这与 `phase0-dead-code-inventory.md` 的「保留 preload 契约」惯例**不同**（用户明确选择含契约删除）。

目标：

- 删除后 `pnpm typecheck`、`pnpm build:web` 全绿，`pnpm dev` 可启动，终端/worktree/设置等保留功能可正常使用
- 每步可独立回滚；`pnpm test` 不新增失败（既有失败见 Phase 1 backlog）
- 设计文档同步更新，产出一份删减记录

非目标：不动终端/PTY、worktree/项目模型、agent 状态、编辑器、source control、设置页宿主、i18n 框架、mock bridge 主体；不清理 SSH/远程/Web 死树（Phase 1 backlog 另行安排）。

## 2. 删除范围与边界

### 2.1 Pet 桌宠

| 项 | 内容 |
|---|---|
| 组件 | `src/renderer/src/components/pet/`（约 22 文件） |
| 挂载点 | `components/status-bar/StatusBarSurface.tsx` 的 `PetStatusSegment` 懒加载与 `petEnabled` 分支（`PetStatusSegment.tsx` 一并删）；`app-shell/AppRootSurfaces.tsx` 的 `PetOverlay` 分支与 `shouldRenderPetOverlay` |
| store/设置 | `store/slices/ui/ui-slice-surface-actions.ts` 中 pet 导入/删除调用；settings 中 pet 相关字段（`petEnabled`/`petVisible` 等） |
| 契约 | `preload/api/pet-api.ts`、`preload/api/pet-bridge.ts`、`preload/index.ts`、`preload/api-types.ts`；bridge mock 中 pet 域（若存在；否则由 fallback 覆盖） |
| i18n | pet 相关键（6 locale + `en-runtime-required`） |

### 2.2 引导/营销面：Contextual Tours + Feature Tips + Feature Wall + Setup Guide

四项共享代码（`shared/feature-wall-setup-steps.ts`、`FeatureWallSetupChecklist`、`feature-wall-setup-progress`）随本组一并删除，无需重构。

| 项 | 内容 |
|---|---|
| 组件 | `components/contextual-tours/`（约 25 文件）、`components/feature-tips/`（约 21）、`components/feature-wall/`（约 90）、`components/setup-guide/`（约 11） |
| 设置/侧栏 | `settings/SettingsSetupGuidePane.tsx`、`settings/settings-setup-guide-progress.ts`、`sidebar/SetupGuideSidebarEntry.tsx`、设置导航与搜索条目 |
| 挂载 | `app-shell/use-onboarding-and-feature-tips.ts` 拆分：onboarding 部分保留，feature tips 部分删除；`AppRootSurfaces`/`AppBackgroundServices` 对应挂载点 |
| 共享 | `shared/feature-wall-setup-steps.ts`（随本组删除） |
| 其他 | `vite.config.ts` 的 `ORCA_FEATURE_WALL_ENABLED` define（当前无任何引用，直接删）；`lib/feature-education-telemetry.ts` 中 tours/tips/wall 调用点随组件删除（terminal 等保留调用点留到第 7 步） |
| i18n | 四组组件的键 |

保留：Onboarding 本体（用户未选删）、`setup-script-telemetry.ts`（若仅 telemetry 使用则第 7 步处理）。

### 2.3 Dictation 语音听写

| 项 | 内容 |
|---|---|
| 组件 | `components/dictation/`（约 21 文件） |
| 挂载 | `app-shell/AppRootSurfaces.tsx` 的 `DictationController` 懒加载、`shouldMountDictationController`、`dictationState`/`voiceEnabled` 相关选择器 |
| 设置 | 设置 voice 面中仅听写使用的项（`OpenAiTranscriptionKeyDialog` 等实施时判定） |
| 契约 | `preload/api/speech-api.ts`、`preload/api/speech-bridge.ts`、`preload/index.ts`、`preload/api-types.ts`；bridge mock 中 speech 域 |
| 其他 | 听写相关快捷键定义（`shared/keybindings/`）、store 切片/状态、hooks（`use-hold-dictation-gesture` 等） |
| i18n | 听写相关键 |

### 2.4 Emulator Pane 移动端仿真

**推翻 Phase 0 判定**：`phase0-dead-code-inventory.md` 曾以「浏览器仿真能力，非移动端配对」保留 `emulator-pane`；本次用户决定删除，删减记录中须注明该判定变更。

| 项 | 内容 |
|---|---|
| 组件 | `components/emulator-pane/`（约 54 文件） |
| 设置 | `settings/MobileEmulatorSettingsPane.tsx`、`MobileEmulatorAgentControlRow`、`MobileEmulatorAvailabilityDetails`、`mobile-emulator-search` 及 settings 渲染注册 |
| 契约 | `preload/api/emulator-api.ts`、`preload/api/emulator-bridge.ts`、`preload/index.ts`、`preload/api-types.ts`；`bridge/mock/emulator-api.ts`、`bridge/create-api.ts` |
| 关联 | `preload/api/pty-api.ts` 中 emulator 专用方法；store（`ui-slice-*` 中 emulator 引用）；`shared/default-global-settings.ts`、`shared/constants.ts`、`shared/keybindings/definitions-core-2.ts`、`shared/feature-interaction-*` 中 emulator 条目 |
| i18n | emulator 相关键 |

### 2.5 Activity / Dashboard / Kanban / Agent Map / Popout

| 项 | 内容 |
|---|---|
| 组件 | `components/activity/`（约 78 文件）、`components/dashboard/`（约 61）、`components/dashboard-popout/`（约 123）、sidebar Kanban 全套（`WorkspaceKanban*`、`workspace-kanban-*`、约 50 文件）、`sidebar/AgentDashboardSidebarEntry.tsx` |
| 入口 | `popout.html`、`popout.tsx`（当前未接入 vite `rollupOptions.input` 与 tauri windows 配置，属休眠入口，删除零配置改动） |
| 壳层 | `app-shell/AppWorkspaceShell.tsx` 的 activity 分支、`TitlebarMainStrip.tsx`、`use-app-chrome-layout.ts`；`shared/ui-chrome-types.ts` 的 `'activity'` 枚举项及持久化校验 |
| 解耦 | `ActivityThreadCollapseContext` 被 `sidebar/index.tsx` 引用；`use-workspace-board-task-status-sync`、worktree 卡片同步等交叉引用实施时逐一判定 |
| 设置 | `settings/AgentDashboardExperimentalSetting.tsx` 与实验搜索条目 |
| store | activity/dashboard/kanban 相关切片与选择器 |
| i18n | 相关键 |

### 2.6 Native Chat

| 项 | 内容 |
|---|---|
| 组件 | `components/native-chat/`（327 文件） |
| runtime | `runtime/structured-agent-session-*`、`web-agent-session-handoff.ts` |
| 终端互嵌（先解耦再删） | `terminal-pane/TerminalPaneNativeChatPortal.tsx`、`native-chat-covered-pane.ts`、`native-chat-leaf-title-agent.ts`、`use-terminal-pane-chat-state.ts`、`StructuredAgentSessionTerminalReturnButton.tsx` 等约 20 个文件的引用点 |
| tab 层 | `tab-bar/QuickLaunchButton.tsx`、`TabBarCreateEntry.tsx`、`tab-group` 中的引用点 |
| 契约 | `preload/api/native-chat-api.ts`、`preload/api/native-chat-bridge.ts`、`preload/index.ts`、`preload/api-types.ts`、`preload/api/runtime-bridge.ts` 的 `onNativeChatLaunchDraftResolved`、`preload/api/runtime-api.ts` 对应类型 |
| mock | bridge mock native chat 域 |
| shared | `shared/native-chat-*`（含 session options、slash commands、edit patch、stream unsubscribe 等） |
| 设置 | `settings/NativeChatExperimentalSetting.tsx` 与搜索条目 |
| web | `web/preload-api/web-native-chat-api.ts`、`web/web-preload-api.ts` 引用 |
| 其他 | `lib/native-chat-telemetry.ts`（若第 7 步未先删则随本步删） |
| i18n | native chat 键 |

### 2.7 Telemetry 全链（含崩溃上报与 Feedback）

用户选择全链铲除；TypeScript 编译器保证引用清零。

| 项 | 内容 |
|---|---|
| 契约 | `preload/api/telemetry-api.ts` + `preload/index.ts` + `preload/api-types.ts`；bridge mock 中 telemetry 域与 `settings-api` 中 telemetry 字段 |
| renderer 库 | `lib/telemetry.ts`（`track`/`setOptIn`/`getConsentState`/`acknowledgeBanner`/`PRIVACY_URL`） |
| shared | `shared/telemetry-*` 全部（events/consent/registry/classification/property schemas/feature-education/onboarding/repository/app/daemon 等约 15 文件） |
| UI 面 | `components/TelemetryFirstLaunchSurface.tsx`、`FirstLaunchBanner` 中遥测部分 |
| 设置 | `settings/PrivacyPane.tsx`、`PrivacyDiagnosticsSection.tsx`、`privacy-search.ts` 中遥测项；`shared/global-settings-types.ts`、`shared/persisted-state-types.ts` 中遥测字段 |
| Feedback | `sidebar/SidebarFeedbackDialog.tsx` 与 feedback preload/bridge/mock |
| 崩溃上报 | `shared/crash-reporting.ts`、renderer crash diagnostics、29 处 `recordRendererCrashBreadcrumb`/`installRendererCrashDiagnostics` 调用点（含 `main.tsx`；popout 的调用点随第 5 步删除） |
| 埋点 | 剩余约 80 个文件的 `track()` 调用点（其中一部分已随 1–6 步删除），删除 import 与调用 |
| 其他 | `lib/feature-education-telemetry.ts` 及残留调用点；`src/main` 中 telemetry/PostHog 模块（休眠 Electron 代码，一并删） |
| i18n | 遥测/反馈/崩溃上报相关键 |

### 2.8 全局收尾

- 全库可达性兜底扫描：按最终入口全量分析，未预期孤儿逐个判定（删除或记录残留理由）
- i18n 全量清键 + i18n 守卫测试更新
- 依赖清理：对每个 dependency/devDependency 逐个 grep；只删被删域独占的（候选：`@streamparser/json`、`tldts`、`react-grab`、`html-to-image`、`@dnd-kit/*`、`@tanstack/react-virtual` 等，实施时逐项确认）
- ratchet 重基线：`child-process-import-allowlist.txt`、`lazy-use-ref-ratchet`、`lazy-modal-mount-state` 等按删除后实际数量更新 pin
- `pnpm test` 全量对比基线：既有失败可保留，新增失败必须清零
- 文档：更新 `docs/superpowers/specs/2026-09-14-ade-design.md`（§2.1 保留清单/§2.2 删除清单/§6 UI 结构/Phase 2–3 等阶段内容剔除本次删除项）+ 新增 `docs/phase1-feature-trim-record.md`（沿死代码清单格式；含推翻 Phase 0 emulator 保留判定的说明）；README 不动

## 3. 执行顺序与理由

| 步 | 域 | 理由 |
|---|---|---|
| 1 | Pet | 最小、最自包含，用于验证 SOP |
| 2 | Tours + Tips + Wall + Setup Guide | 营销/引导面相互关联，且共享 `feature-wall-setup-steps`，同组删除 |
| 3 | Dictation | 独立，挂载点集中在 AppRootSurfaces |
| 4 | Emulator | 独立，但需动 pty 契约与 shared 默认值 |
| 5 | Activity/Dashboard | 含休眠 popout 入口与顶层视图枚举 |
| 6 | Native Chat | 最重，需先解终端 portal 互嵌 |
| 7 | Telemetry 全链 | 放后：先让 1–6 步各域带走自己的埋点，减少重复工作；剩余埋点集中在保留域（terminal/worktree/composer/sidebar/source-control/onboarding 等） |
| 8 | 全局收尾 | 兜底扫描、i18n、依赖、ratchet、文档 |

提交策略：分支 `phase1-trim-features`；第 1–7 步各一个提交，第 8 步一个收尾提交，共 8 个；每个提交可单独 revert。

## 4. 每域标准作业流程（SOP）

1. **可达性分析**（沿 Phase 0 Task 10 方法）：入口 = `src/renderer/index.html → main.tsx`、`popout.html → popout.tsx`、`web-index.html → web/main.tsx` + `src/main`、`src/preload`、`src/bridge` 全量文件；解析相对导入、`@/`、`@renderer/` 别名与 lazy `import()` 字面量。产出该域「独占文件清单」与「共享文件引用点清单」
2. **分批删除**：`git rm`，≤20 文件/批；每批后 `pnpm typecheck && pnpm build:web` 必须 exit 0
3. **清理关联面**：mock 域 → preload 契约（`api/*.ts` + `index.ts` + `api-types.ts`）→ store 注册/slice → 设置项/导航/搜索条目 → i18n 键（6 locale + `en-runtime-required`）→ ratchet/守卫测试 pin 与列表条目
4. **测试**：跑受影响 guard 测试（i18n、ratchet、boundary）；沿 `app-startup-routing.test.ts` 惯例加「入口不得回潮」反回归断言
5. **提交**：`chore: 移除 <域>（含契约与测试）`

## 5. 验收门禁

| 层级 | 门禁 |
|---|---|
| 每批删除后 | `pnpm typecheck` exit 0；`pnpm build:web` exit 0 |
| 每域完成后 | 受影响 guard/boundary 测试通过；`pnpm dev` 人工冒烟：启动、终端可用、worktree 列表可用、设置页可达、被删入口不存在 |
| 第 8 步 | 全局可达性扫描无未判定孤儿；`pnpm test` 对比基线无新增失败；i18n 守卫测试绿；依赖 grep 确认无未用残留 |

已知约束：`pnpm test` 基线存在既有失败（Phase 1 backlog 第 2 项：ratchet stale、缺 locale-ko override 等），本任务只保证不新增失败。

## 6. 风险与缓解

| 风险 | 缓解 |
|---|---|
| fork 耦合比预估深（Native Chat 与终端 portal、Emulator 与 pty 契约、Activity 与 sidebar 状态同步） | 每域先做可达性分析；Native Chat 排最后；编译门禁逐批拦截 |
| 可达性分析误判导致误删 live 代码 | 沿 Phase 0 方法（只删「仅剩测试或自身引用」的模块）；typecheck/build 门禁；每域独立提交可回滚 |
| ratchet 测试大量 pin 需重基线，易与既有失败混淆 | 第 8 步集中处理，先记录基线再对比 |
| i18n 清键触发守卫测试 | 每域同步更新守卫与 semantic 测试；键删除以引用扫描为准 |
| 依赖清理误删共享依赖 | 逐依赖 grep；仅删独占项；保守优先（不确定则保留并记录） |
| Telemetry 全链铲除面大（80 文件埋点 + 29 处 breadcrumb） | 编译器强制找全引用；排在第 7 步减少重复；逐批门禁 |

## 7. 后续

本规格审阅通过后，使用 writing-plans 技能编写实施计划（按本文件 §3 顺序拆分为任务，每任务含可达性分析、批次删除、契约/键清理、验证命令与提交）。
