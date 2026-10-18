# Phase 3C 插件 UI↔worker 通道与 updater 插件收尾记录

- 日期：2026-10-17（起始实施）～ 2026-10-18（实施完成 + 终态自动门禁）
- 分支：`phase3c-ui-worker-channel`（spec 基线 `4bfb142c` 前 `main=8d8bb849`；计划 `e11d9cda` + 预检修正 `1079c3ee`；特性/fix 提交 `8687acd1`…`a80a2555`）
- 规格：`docs/superpowers/specs/2026-10-17-plugin-ui-worker-channel-design.md`（可读；冲突裁决权威）
- 计划：`docs/superpowers/plans/2026-10-17-plugin-ui-worker-channel.md`（18 任务；A 部分 T1–T9 平台通道线性、B 部分 T10–T18 updater 插件线性；T12/T14/T16 各 1 轮 fix；Task 18 勾选全部复选框）
- 行为参照：`docs/phase3a-plugin-system-record.md`（Phase 3A 插件系统既有语义：会话注册表/host gate/桥预算/host-entry.mjs）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态四组门禁 + bindings 新鲜度 + 本记录）
- 手工验收状态：**待用户复核（2026-10-18 自动化门禁已全绿；11 项手工清单见 §4）**

## 1. 范围与验收

| 面 | 内容 | 主提交 |
|---|---|---|
| **A 部分 · 能力扩展** | capability 闭集 8→10（`worker:call`/`session:push`；仅声明者 consent 指纹受影响；consent 文案英文） | `8687acd1` + `fed1f087` |
| **A 部分 · 命令可见性** | `CommandContrib.visibility: "palette"｜"internal"`（缺省 palette，additive）；internal 校验（禁 `action` 别名、keybinding 引用 internal → manifest 校验失败、未知值拒绝）；列表投影携带 | `d657afba` |
| **A 部分 · host methods 14→16** | `worker.invokeCommand`（仅 panel_callable）/`session.post`（仅 worker）方法表 + 参数/结果双端校验 + 预算常量 + gate 矩阵（16×panel/worker） | `dafcf138` |
| **A 部分 · session.post 全链** | 帧 `session` 可选 → SDK `ctx.session` → service 推送预算（per-token 64KiB/30 条 10s）→ `plugins:panel-push` 事件发射 + bridge sink；token 校验（无效/他插件/非 UI kind → `unavailable`） | `4d78c50e` |
| **A 部分 · worker.invokeCommand 全链** | 帧/宿主 handler 的 `session` 透传、命令声明校验、结果 64KiB 上限、审计对齐、错误映射（未声明→`invalid_params`、errored→`unavailable`、崩溃/超时/抛错→`action_failed`） | `77a4fc14` |
| **A 部分 · visibility 贯通** | `PluginCommandDto` 增 `visibility` → bindings 重生成 → TS 类型/mock fixtures 同步 | `654248c0` |
| **A 部分 · TS 通道契约** | capability 联合 + spec 表（16 方法）+ panel action 集合（含 `worker.invokeCommand`）+ guest 消息 `orca-panel-push` + 订阅 `onPanelPush`（契约 25→26：23 命令 + 3 订阅） | `e32e647e` |
| **A 部分 · 渲染层推送转发** | 三个宿主组件共用 `use-plugin-panel-entry.ts`：按 token 匹配 → `contentWindow.postMessage({type:'orca-panel-push', …})`；web 端维持无插件面 | `fe200e52` |
| **A 部分 · internal 过滤** | 命令面板与 `plugin-quick-actions`（含 keybinding 消费面）过滤 internal 命令 | `8e652407` |
| **B 部分 · 脚手架与构建** | `plugins/updater/{ade-plugin.json, build.mjs, tsconfig*.json, src/**}`；`pnpm build:updater`（worker esbuild 单文件 `dist/main.mjs` + UI vite 单入口两趟构建，见 §5.2-1 → 单文件 `dist/tab.html`/`dist/panel.html`，Geist data: URI 内联）；`vitest` include `plugins/**`；`dist` 不入库 | `9fbf2fec` |
| **B 部分 · parse** | 纯函数 `parseNpmOutdated`（exit 1 = 有更新 / 空 `{}` = 无）、`parseBrewOutdated`（v2 `formulae`/`casks`）→ 统一条目 `{manager,kind,name,current,latest}` | `71377856` |
| **B 部分 · runner** | `runCommand`：spawn（继承 worker 环境）、stdout+stderr 行化、退出码、取消 SIGTERM→2s→SIGKILL、超时 | `aac11a61` + `3b068c86` |
| **B 部分 · managers** | 适配器注册表 `ManagerAdapter{id,label,kinds,detect,outdated,parse,upgrades}`；v1 注册 npm（`npm outdated -g --json` / `npm install -g <name>@latest`）与 brew（`brew outdated --json=v2` / `brew upgrade [--cask] <name>`）；check/update-all/cancel/UI 全注册表驱动 | `e0286d05` |
| **B 部分 · jobs** | 单飞任务状态机 `idle/checking/updating/done/failed`；最近报告缓存（`lastReport`+`checkedAt`）；管理开关持久化（storage `managers`）；会话簿记（token 按 kind，`unavailable` 剔除）；日志缓冲批量推送（≤2Hz、单行 4KB、单推 ≤64KiB） | `b7cc57b5` + `164cb292` |
| **B 部分 · main 接线** | `activate(orca)`：六命令（4 palette + 2 internal）注册、`ctx.session` 装配与会话簿记、宿主桥装配、错误兜底 | `a1225048` |
| **B 部分 · UI 基建** | `src/ui/{bridge,state,kit,theme.css}`：宿主桥（invokeCommand/push 订阅/连接状态）、reducer、本地 UI kit、`@theme inline` 与宿主逐字 parity（vitest 锁死） | `da841e55` + `c5af2bc2` |
| **B 部分 · UI 视图** | tab（统一表格 + 筛选 chips + 右侧 300px 常驻输出栏 + 四状态机）与 panel（管理器列表 + 状态点 + 显示开关 + Check now/Open tab）单文件入口 | `a80a2555` |
| **计划与过程** | 18 任务计划 + 预检修正（T4 测试借用绑定）；Ruling 与 minor/deferred 备案见 §5 | `e11d9cda` + `1079c3ee` |

验收口径（spec §5）：11 项手工验收（§4，待用户复核）；自动门禁四组全绿（§3）+ bindings 新鲜度 + `build:updater` 单文件断言。终态变更规模（净树差异，`git diff --shortstat 8d8bb849..HEAD`）：**71 个文件，+8313 / −81**（含计划 docs；本记录由 Task 18 提交追加，SHA 见提交后 `git log`）。churn（`git log --numstat` 求和，排除二进制）**+8402 / −170**（101 文件次）。

## 2. 提交清单

`git log --oneline --reverse 8d8bb849..HEAD`（分支起点 `8d8bb849`=Phase 3B 收尾，区间下界不列入；本记录由 Task 18 提交追加，SHA 见提交后 `git log`）：

| 提交 | 内容 |
|---|---|
| 4bfb142c | docs: Phase 3C 插件 UI↔worker 通道与 updater 插件设计规格 |
| 399a6016 | docs: Phase 3C 规格更新（UI 定稿与派生平台增量） |
| 2353bb09 | docs: 修正规格事实（契约 25→26、web composition 现状） |
| a651bd6b | docs: 规格修正（session.post 校验与 set-managers 结构化失败） |
| e11d9cda | docs: Phase 3C 实施计划（18 任务） |
| 1079c3ee | docs: 计划修正（T4 测试借用绑定）（预检） |
| 8687acd1 | feat(plugins): 新增 worker:call/session:push 能力（T1） |
| fed1f087 | fix(plugins): T1 consent 文案统一英文（T1 fix 1） |
| d657afba | feat(plugins): 命令可见性 visibility（manifest+投影）（T2） |
| dafcf138 | feat(plugins): host methods 扩至 16（worker.invokeCommand/session.post 表与门控）（T3） |
| 4d78c50e | feat(plugins): session.post 全链（推送预算+事件 plugins:panel-push）（T4） |
| 77a4fc14 | feat(plugins): worker.invokeCommand 全链与会话上下文透传（T5） |
| 654248c0 | feat(plugins): visibility 贯通 bridge DTO/bindings/TS 类型（T6） |
| e32e647e | feat(plugins): TS 侧通道契约（spec 表/推送消息/onPanelPush 26 成员）（T7） |
| fe200e52 | feat(plugins): 渲染层按会话 token 转发 panel-push（T8） |
| 8e652407 | feat(plugins): 命令面板与快捷键面过滤 internal 命令（T9） |
| 9fbf2fec | feat(updater): 插件脚手架与单文件构建管线（T10） |
| 71377856 | feat(updater): npm/brew outdated 解析层（T11） |
| aac11a61 | feat(updater): 进程执行层（行流/超时/取消）（T12） |
| 3b068c86 | fix(updater): T12 跨 chunk UTF-8 解码修复（T12 fix 1） |
| e0286d05 | feat(updater): 管理器适配器注册表（npm/brew）（T13） |
| b7cc57b5 | feat(updater): 单飞任务状态机与推送/预算控制（T14） |
| 164cb292 | fix(updater): T14 单飞守卫与剪枝测试修复（T14 fix 1） |
| a1225048 | feat(updater): 六命令接线与宿主桥装配（T15） |
| da841e55 | feat(updater): UI 基建（主题 parity/kit/桥/状态）（T16） |
| c5af2bc2 | fix(updater): T16 bridge ready 语义修复（T16 fix 1） |
| a80a2555 | feat(updater): tab 与 panel 视图接入单文件入口（T17） |
| （本提交） | docs: Phase 3C 实施记录与门禁证据（T18） |

## 3. 门禁证据（自动化）

四组门禁在终态 HEAD（`a80a2555`，分支 `phase3c-ui-worker-channel`，工作树干净）顺序执行：

| 门禁 | 命令与工作目录 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace`（`src-tauri/`） | **exit 0**；50 个 suite（40 test binary + 10 doc-tests）累计 **1232 passed / 0 failed / 0 ignored**；`ade-plugins` lib 356、`ade-bridge` lib 182 + 集成（plugins 11 / plugins_service 13 / host_entry 5 / host 6 / session 5 / stream 6 / supervisor 3 / staging 20 / status 16 / watch 5 / shell 3 …）、`orcinus-cli` cli_basics 8 / cli_integration 5；`specta_export::tests::bindings_are_fresh` ok、`command_inventory_and_bindings_are_fresh` ok |
| bindings 新鲜度 | `cargo run -p ade-bridge --bin export-bindings`（`src-tauri/`）→ `git diff --exit-code -- src/bridge/real/generated/tauri-bindings.ts`（仓库根） | **exit 0 / exit 0**；重生成 `len=106456` 后零差异 |
| 类型与构建 | `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web && pnpm build:updater`（仓库根） | **exit 0 / exit 0 / exit 0**；三段 `tsc --noEmit`（根 + `tsconfig.worker.json` + `tsconfig.ui.json`）零诊断；`✓ 10035 modules transformed`、`✓ built in 2.86s`；`externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；7 条信息性 `(!)`（既有动态/静态双导入与大 chunk 提示），无本阶段新增阻断；`updater plugin built -> plugins/updater/dist` |
| 单文件断言 | `node --check plugins/updater/dist/main.mjs` + node 单文件内联断言（T10/T17 一致）+ `grep -c "data:font"` | **exit 0**；`main.mjs syntax ok`、`single-file ok`（两产物均无外部 `<script src>`、无外部 `<link rel="stylesheet">`、含内联 `<style>`）；`data:font` 计数 tab/panel 各 1、`Geist` 计数 1（Geist 字体 data: URI 已内联） |
| TS 全量 | `pnpm test`（仓库根，vitest 4.1.11） | **exit 0，全绿**：`Test Files 2998 passed / 4 skipped (3002)`；`Tests 26944 passed / 51 skipped (26995)`；Duration **259.89s**（transform 94.21 / setup 55.00 / import 943.52 / tests 340.33 / environment 109.02）。node 存在（v22.21.0）故 node 依赖用例全部实跑，无 SKIP 计通过路径；插件面 `plugins/updater` 10 个测试文件 / 38 用例已随全量收集并通过（隔离复跑 `pnpm vitest run plugins/updater` 同结果）；4 files / 51 tests 跳过为既有条件跳过项，无失败 |

**已知环境问题记录（非 Phase 3C 引入）**：首轮 `cargo test --workspace` 在 `orcinus-cli --test cli_basics` 5 项失败（`help_and_version_flags_exist`、`open_human_launches_without_waiting_for_a_runtime`、`open_json_returns_the_launched_envelope`、`status_human_prints_metadata_state_lines`、`status_json_with_readable_metadata_reports_stale_bootstrap_and_id`）。定位为**过期构建产物**：`src-tauri/target/debug/orcinus` 是 2026-10-11 构建的 pre-trim 二进制（其 help 文案含 "become reachable"、错误码 `runtime_open_timeout` 在现源码中均已不存在），而 `orcinus-cli` 源码已在 2026-10-17 的 Phase 3B computer-use trim（`38bd6e8b`，Phase 3C 基线 `8d8bb849` 的祖先）改为不等待运行时；该 crate 在 Phase 3C 区间零改动（`git diff 8d8bb849..HEAD -- src-tauri/crates/orcinus-cli` 为空）。强制重建（`cargo build -p orcinus-cli --bin orcinus`）后隔离复跑 `cargo test -p orcinus-cli --test cli_basics` **8 passed / 0 failed**，随后整轮重跑 **exit 0**（上表数字取自该绿轮）。首轮失败日志留存 `/tmp/ade-phase3c-cargo-test-fail1-stale-bin.log`。

日志留存于本机临时目录（`/tmp/ade-phase3c-cargo-test.log` / `/tmp/ade-phase3c-export-bindings.log` / `/tmp/ade-phase3c-typecheck.log` / `/tmp/ade-phase3c-build-web.log` / `/tmp/ade-phase3c-build-updater.log` / `/tmp/ade-phase3c-pnpm-test.log`）。

## 4. 手工验收清单（待用户复核）

前置：`pnpm build:updater`；`pnpm dev`；系统 `node>=18`；macOS（npm + Homebrew）。

1. 设置 dev 路径 → 开发中 tab 出现 → 插件中心启用（consent 展示 `worker:call`/`session:push`/`workspace:open-tab`/`storage` 四能力）→ 确认 —— **待用户复核**
2. 「+」菜单打开 Updates tab → 自动检查（骨架→列表；统一表格 + chips + 右侧常驻输出栏）→ 与 `npm outdated -g` / `brew outdated` 命令行结果一致 —— **待用户复核**
3. Update all → 行级状态 + 右侧实时日志；完成后汇总条正确、列表减少；失败隔离核对：若真实场景出现单包失败，确认该项标 failed 且后续包继续（无失败场景时以插件单测覆盖为准） —— **待用户复核**
4. 运行中 Cancel → 当前包终止、剩余 skipped、汇总正确；再次 check 状态一致 —— **待用户复核**
5. 运行中关闭 tab → 重开 → 自动接上运行中任务进度（快照重推）；不产生重复任务 —— **待用户复核**
6. 右侧栏 panel：管理器列表（状态点/计数/开关）；未检查态 "—"；toggle 开关 → 已开 tab 的 chips/列表即时同步；重启 IDE 后开关保持（storage）；panel `Check now` 与 tab 状态一致；`Open tab` 打开/聚焦 —— **待用户复核**
7. light/dark 切换 → panel 与 tab 重挂载后配色/字体/圆角与宿主设置页同屏对照一致（Geist 生效） —— **待用户复核**
8. 命令面板 `Updates: Check/Update all/Open updater tab/Cancel` 可用；`snapshot`/`set-managers` **不出现**在命令面板；`open` 打开/聚焦既有 tab —— **待用户复核**
9. 禁用/卸载插件 → tab/panel 不可用态 → 数据目录/worker 清理；重装后可用 —— **待用户复核**
10. 模拟缺 npm 或缺 brew（PATH 裁剪）→ 对应管理器显示 "not detected"（tab 卡片 / panel 状态点），其他管理器正常；未安装管理器开关可切换 —— **待用户复核**
11. 面板消息洪峰（更新大包）→ UI 不卡死、无 `rate_limited` 误伤（插件节流生效） —— **待用户复核**

任一项失败：补记到本文件 §5 并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

逐条读 `.superpowers/sdd/2026-10-17-plugin-ui-worker-channel/progress.md`（执行台账），归纳 Ruling 与 minor/deferred 中属"规格偏差或已知边界"的条目。

### 5.1 spec §6 预留风险项（逐条备案）

1. **帧/SDK 兼容**：`invokeCommand` 帧的 `session` 字段为 additive 可选分支，旧插件 handler（只声明 `args`）零影响；host-entry 与 app 同版发布假设沿用既有约束（`/Users/itsuka/CodeSpace/ade/src-tauri/resources/plugins/host-entry.mjs` 随 app 一起发布，插件不感知帧细节）。
2. **命令可见性 additive**：`visibility` 缺省 `palette`，既有插件命令全部 palette 可见（默认行为不变由测试兜住）；internal 命令不出现在命令面板/快捷键面。
3. **推送预算与 watchdog**：host→guest 新预算（per-token 64KiB/30 条 10s）与既有 pong/data lane 独立记账，互不挤占；watchdog pong 保留 lane 语义不变。
4. **brew auto-update 时长**：检查耗时波动大；`check` 采用 started+流式推送模式已规避 30s invoke 超时，但慢场景需手工验收覆盖（§4-2/§4-3）。
5. **npm 全局权限**：EACCES 环境差异导致更新失败属预期（UI 展示错误 + 通用提示，不自动修）。
6. **视觉一致性**：parity 测试锁 `@theme inline` token 映射与字体栈逐字一致；组件级像素一致依赖手工同屏对照（§4-7）。

### 5.2 实施中新发现偏差（Ruling 裁决）

1. **build.mjs 三处必要偏差**（T10，Ruling 预评接受）：spec §3.8 要求"单文件产物"，实际采用（a）**单入口两趟构建**（多入口会引发 code-split 悬挂 import，破坏单文件内联）、（b）`$&`/`$1` 替换陷阱的修复（正则替换转义）、（c）CSS import 的 `types:["vite/client"]`（类型声明必需）。三处均为实现细节，不改变"单文件产物"这一被验收事实。若错：等价的其它内联实现需重做构建脚本（低频低成本）。
2. **`ready` 语义修正**（T16 fix，Ruling）：原计划 `bridge.ready` 等待 PONG 永久不 resolve（PONG 为 guest→host 单向，host 不回）；改为"监听器安装完成即 resolve"（guest 侧语义）。若错：ready 语义无消费者差异（T17 stub 已注）。
3. **T12 跨 chunk UTF-8 解码修复**（fix，Ruling）：`chunk.toString('utf8')` 逐 chunk 解码会把跨边界多字节字符变 U+FFFD；改用 `setEncoding('utf8')`（StringDecoder 边界安全）。属真实缺陷非风格问题，行为等价且更正确。
4. **T14 单飞守卫修复**（fix，Ruling）：`updateAll` 仅守卫 `this.running`，在 check 期/自身 recheck 期可重入并并发执行双循环（违 binding 约束"check/update 运行中拒绝重入"）；修法为 guard 加 `phase === 'checking' || 'updating'` 且 `runUpdate` 在 recheck 前先建 `running`（同时修复 cancel 在 recheck 期 no-op）。行为收紧，不破坏既定语义。同轮修复"剪枝测试空洞"（harness postTo 失败先于记录，剪枝逻辑删除测试仍绿；修法：记录 `postAttempts` 并断言第二轮只尝试 live）。
5. **深度 ≤64 仅适用 `args`**（T3，Ruling）：spec §3.1 表未对 handler 结果/payload 要求深度校验，只对 `args` 走 `check_json_bounds`；依据 spec 为权威且 bytes ≤64KiB 已限定规模。若错：后续需为 result/payload 补深度校验（低成本可加）。
6. **T16 import 路径笔误修正**（Ruling）：计划的 import 路径缺 `src/` 前缀（计划笔误），实现以 `'../../../../../src/shared/...'` 修正为准。
7. **`tsconfig.ui` 增 `node` types**（T16，Ruling 接受）：测试文件与 `src/ui` 同目录需 node types；真实 node 依赖由 vite 打包失败兜底。
8. **T17 panel 测试语义修正**（Ruling）：brief（=计划）的 panel 测试断言点第一个开关（npm）却期望 `['brew']`——`set-managers` 为**整集替换**语义，任何正确实现都不可能产出该断言；改为切换 Homebrew 开关并断言 `['npm','brew']`（保留既启用项、追加 brew，验证整体替换语义）。参考实现（spec §3.6/§3.7）正确，测试为计划笔误。
9. **`set-managers` 结构化失败措辞修正**（spec 修正 `a651bd6b`）：未知管理器 id → `{ok:false,error:"unknown_manager",manager}`（不抛错，UI 保留旧开关态）；storage 写入失败 → `action_failed`。
10. **host-entry 与 app 同版发布假设**：`host-entry.mjs` 的 `invokeCommand`/SDK 扩展随 app 发布（Phase 3A §3.11 现状）；插件只见 SDK 对象，不感知帧细节（§5.1-1）。
11. **plan-mandated minors**（多任务）：T3 旧 14 方法测试中 storage/openTab mutation 断言随重写丢失；T4 `PushAdmission` 与既有 `PanelAdmission` 逐行镜像；T4 stale-revision 会话仍可推（依赖既有生命周期失效路径，spec §3.3 语义）；T13 `upgradeArgs`/`upgrade` 命令数组字面重复；T10 `.gitignore` 条目冗余（全局 `dist/` 已覆盖）——均为计划明文的形态选择，非缺陷。
12. **brief 测试/模板补全**（多任务实现者修正）：T1 实现者改 `service.rs`/`gate.rs`/`methods.rs`（编译强制）且 consent 文案初为中文 → `fed1f087` 统一英文；T2 测试用例补 `main`、assert 片段改 `"hidden"`；T4 跳过 brief 中未用的 `panel_ctx` helper（避免死代码）；T6 额外纳入 6 个 fixture 测试文件（typecheck 强制）；T9 brief 模板无参调用与实际签名不符，按既有夹具改写；T14 brief 测试两处缺 `sessions` 已补。
13. **T1 gate.rs 过渡 skip**：T1 引入 `SessionPush|WorkerCall => None` 过渡 skip，由 T3 替换为真实方法映射（覆盖率缺口已闭环，无遗留）。
14. **契约成员 26**（T7）：23 命令 + 3 订阅（`onChanged`/`onOpenTab`/`onPanelPush`）；规格事实修正见 `2353bb09`（原 25 计数为笔误）。
15. **台账内部不一致**（文档性）：`progress.md` 任务清单表与进度记录段在 Task 8 上不一致——**表行停留「未开始」**（行 1 已记 ✅ 完成、行 2–18 停留「未开始」），而**进度记录段已记 `Task 8: complete`**；末尾残留行 `- Task 8: 待实现` 与表一致、与进度记录段冲突。台账为工作区临时文件（`.superpowers/` 已 gitignore），不影响代码与门禁结论。

### 5.3 minor/deferred（评审遗留，均已 deferred，不影响验收）

- **A 部分**：T1 gate.rs 过渡 skip 可加显式 `None` 断言、`service.rs` 两个新描述臂未按枚举字母序；T3 `gate.rs` `method_for` Option 形态死分支、`session_post_params` `unwrap_or(usize::MAX)` 防御性死代码、payload/result 未做深度 ≤64（§5.2-5）；T4 service 层 emit 测试只断言计数未断言内容；T5 handler 边界测试未断言 `invoke_calls` 内容、`(via_panel=false, session_token=Some)` 组合未单测、新 host_entry 测试未 pin `Ready.commands` 列表；T6 DTO enum 多 `Copy` derive（与邻居一致、无害）；T7 `plugin-capabilities.ts` 头注释仍写 "(8 total)"，应更新为 10；T8 drop 分支未断言 `postMessage` 未被调用、`window.api` 异步可用假设（与周边一致，实际不发生）；T9 新测试未在过滤后输出上复断言映射字段。
- **B 部分**：T10 build.mjs 正则转义可加固为全量 escapeRegExp、CSS 未转义 `</style>`、建议 build.mjs 末尾加产物自检断言、`.gitignore` 条目冗余；T11 `item?.name` 冗余可选链、非对象 JSON 运行时形状无校验、类型守卫空串路径未直接测试；T12 SIGKILL 升级分支与 signal 预中止路径无测试、`onLine` 抛出未隔离；T13 `upgradeArgs`/`upgrade` 命令数组字面重复、signal 杀死时错误文本 "exit null" 可附 signal、detect 吞所有 reject；T14 非零节流路径（coalesce/interval/4KB/48KB）无测试、coalescing 路径可能重复一次 state 推送（幂等）、广播载荷引用活状态（T15 装配下 host-entry `send` 同步 JSON.stringify，安全）、空 pending 早返保留旧 summary（表象不一致）、rows 排序无断言、`setManagers` 透传 storageSet rejection；T15 `open` 未断言 params/透传、`overrides` 合并顺序未测、`storageGet` 解包未直接断言、remember 仅 check 断言；T16 parity 正则对宿主未来嵌套 `\n}` 无防护、parity 为包含语义、state.test 未真正推满 500 行、bridge.test 未覆盖 onInit/ok:false/监听器拆除、Chip 无 `aria-pressed`；T17 `TabApp` 重复 `{report:null,logs:[]}` 字面（未用 `INITIAL_UI_STATE`）、panel 回滚路径无测试（serial-toggle 下正确）、空 chip 过滤态会显示 "All up to date"、`formatCheckedAt` 重复且相对时间不刷新、panel 对 payload 无运行时校验（受信通道）。

### 5.4 测试基建

- 缺 node 路径 SKIP；updater 插件单测跑 TS 源（parsers/jobs/runner/parity），无需先构建；theme parity 测试锁 `@theme inline` 映射与宿主逐字一致；`dist/` 产物不入库（CSP 单文件约束）；web 端维持无插件面（Phase 3B 已随远程客户端移除 web composition 测试，唯一现存在锁为 `useSettingsNavigationMetadata` 的 web 隐藏项，保持不动）。

## 6. 已知边界与后续项

（照抄 spec §7）

- updater 独立 git repo + 市场上架（dist 提交、marketplace 条目、git 源安装/更新/回滚链路 dogfooding）
- 方向 1（DB 工具）与方向 3（API 客户端）作为通道的下一批消费者；届时评估结果分块（>64KiB）与流式增强
- host 侧会话枚举/广播推送（现由插件自记 token 解决；插件数量增长后可平台化）
- 跨插件共享 UI kit 抽包（第二个数据驱动插件出现时）
- updater 增强：单包选择、wanted/latest 双列、greedy cask、定时检查、系统通知、项目依赖、更多包管理器适配器（pipx/cargo/mas 等）、输出栏拖拽调宽、表格排序、失败行点击定位日志

## 7. 终态全分支评审与 F1 修复波（追加）

- 终态评审（范围 `e11d9cda..2c09d2b2`，24 提交）结论：**With fixes** —— 2 条 Important：I1 插件推送节流上限（最坏 6/s）超宿主预算（30/10s）且生产节流路径零测试、终态 state 推送可能被静默丢弃；I2 TS 共享 `plugin-manifest.ts` 的 zod 未知键剥离 `visibility`（计划缺项）。
- F1 修复波（单次；提交 `7db4a651`）：
  - I1：默认节流 `STATE_COALESCE_INTERVAL_MS=600` / `LOG_FLUSH_INTERVAL_MS=1200`（稳态 ≈26/10s < 30 预算）；新增 `flushStateNow()` 在三个作业终态点立即冲刷（终态必达）；日志批字节统计改 `Buffer.byteLength(utf8)+1`；新增 `jobs.throttle.test.ts`（真实默认值 + fake timers：coalescing/日志批/200 行即刷/字节触发/终态必达/10s 窗口 ≤30）。
  - I2：`commandContributionSchema` 增 `visibility` 枚举（optional）；真实 manifest 解析测试（`plugins/updater/ade-plugin.json`：6 命令、snapshot/set-managers internal）暴露 schema 同缺 `contributes.tabs`（strict 拒绝）→ 伴随修复：schema 增 `tabs`（上限 64）、重复 id 校验、2 个渲染层 fixture 与 content-pack 默认期望补 `tabs: []`。
  - Minor 5：`plugin-capabilities.ts` 注释 "(8 total)" → "(10 total)"。
- F1 后复验：scoped re-review 全数 ADDRESSED、无新破坏；`pnpm test` **26952 passed / 51 skipped（3003 files）**（260.93s）、`pnpm typecheck` exit 0、`pnpm build:updater` exit 0 + 单文件断言通过；Rust 面未动（四组门禁中 Rust/bindings 两组仍以 §3 为准）。
- 残余（评为可后续）：日志批字节统计未含 JSON 包装开销（改善方向，未消除）；TS schema 未镜像 Rust 的 `internal+action` 组合拒绝；计划历史文本仍含 `LOG_FLUSH_INTERVAL_MS=500` 与 T17 panel 测试旧断言（以本记录 §5.2 为准）。

## 8. 上线后体验诊断（2026-10-18；按用户裁定：只留档，暂不修）

用户反馈"安装插件和使用插件的过程很卡"，细化后为三处：①切走再切回 Updates tab 需等待数秒才出列表；③"启用/consent 后"感觉卡；④Update all 运行时界面粘滞（用户下次观察后再定因）。

### 8.1 证据链

- **后端耗时实测（本机）**：`npm outdated -g --json` 0.73s；`brew outdated --json=v2` 0.51s；插件树 31 文件 / 1MB——检查与发现/哈希均为毫秒~秒级，非瓶颈。
- **构建产物实测**：`dist/tab.html` 453KB（内联 JS 346KB minified + CSS 107KB 含 91KB base64 字体）——每次 iframe 重挂载需完整重新解析。
- **应用数据事实**（`~/Library/Application Support/dev.itsuka.orcinus/`）：`plugins.lock.json` 显示实际安装源为 **local-path**（16:48:27），非 dev 路径；`plugin-data/itsuka.updater/audit.log`：install 16:48:27 → consent approved 16:49:15（48s 为人工操作间隔）；`settings.json` 的 `devPluginPaths` 及其两个备份均为 `[]`（dev 流程从未成功使用）；`disabledPlugins: []` → consent 批准后插件即视为已启用（故无 `plugins.set_enabled` 记录）。consent 状态持久化于 `settings.json` 的 `pluginConsents`。
- **代码路径**：`TabGroupPanel.tsx:298-307` 仅渲染激活 tab（切走即卸载 `PluginTabContent`）；`tab/App.tsx` 挂载流程只 `invokeCommand('check')`（worker 有 `snapshot` 缓存可用但未先取）；`jobs.ts` 的 `check` 在空闲态无条件全量重跑；`service.rs` 的 `install`/`consent`/`set_enabled` 命令均为毫秒级（list=discover+投影+状态读，无树哈希）。

### 8.2 根因判定

- **①（已定案）**：插件 tab 卸载-重挂载 + `TabApp` 状态清零 + 全量 `check` 重跑 + 骨架屏至检查完成（数秒）。放大器：每次重挂载重新解析 453KB 文档；首次使用叠加 worker 冷启动。
- **③（已定案≈①）**：安装/consent 命令本体无秒级任务（代码与审计时间戳双重证实）；"consent 后卡"即批准后首次打开使用界面所经历的同一条 ① 链路。
- **②（真实缺口，未触发）**：`devPluginPaths` 仅在应用启动装配时读取一次（`state.rs` 唯一读者；`service.start()` 从冻结 config 建 watcher；全仓无运行时更新命令），运行中添加/修改 dev 路径**需重启应用**才生效；设置页 Add path 后的 `plugins.refresh()` 仅用旧路径重列。属 Phase 3A 遗留（验收清单第 6 条未复核）。
- **④（待定因）**：用户下次观察需区分"整个应用 vs 仅插件面"、"连续 vs 阵发"。候选机制：(a) npm/brew 子进程的系统级 CPU/磁盘负载（环境因素）；(b) stale session token 扇出放大——每次 `load()`（挂载 + 每个 `plugins:changed`）签发新 token 且旧 token 不作废（registry 仅按 owner/plugin/scope/kind 失效；worker 侧仅 `unavailable` 剔除），更新期间每次推送按历史 token 数放大 IPC 往返——该累积本身是独立成立的卫生缺陷，与 ④ 归因无关。

### 8.3 建议修复（未实施，待定；用户裁定"暂不修，先记录"）

1. **①最小修复**：`TabApp` 挂载顺序改为 注册推送 → 先 `snapshot`（仅当 `state.checkedAt !== null` 应用，避免首开误导"全部最新"）→ 再 `check` 后台刷新。TDD：`tab/App.test.tsx` 断言调用序 `['snapshot','check']` 与缓存即时渲染；验证 = plugins 全测 + typecheck + build:updater。
2. **token 卫生（可选）**：`tab_entry`/`panel_entry_for` 重读同一 surface 时作废旧 token（sessions 注册表加按绑定失效查询），或渲染层加载后主动释放前一代 token。
3. **②修复（独立小项）**：`PluginService::set_dev_paths`（重建 DevWatcher + 重发现） + 设置保存钩子 + 测试；或在设置 UI 明示"重启后生效"。
4. **④**：先按 8.2 的观察要点取证；必要时在启用路径加时间戳仪表复现。

- 状态：**留档完成；未产生代码改动**（本节提交即唯一变更）。
