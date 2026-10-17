# Phase 3 子项目 C：插件 UI↔worker 通道与 updater 插件设计规格

- 日期：2026-10-17
- 状态：brainstorming 输出（已批准：通道 = 命令 RPC + 会话推送；updater = npm 全局 + Homebrew、v1 仅全量更新、进入 tab 时检查、插件代码在本仓库 `plugins/updater`、worker 与 UI 均 TS + 构建、UI 与宿主完全一致六项约束；**UI 定稿**——右侧栏 panel（管理器列表 + 显示开关）+ 主区 tab（统一表格 + 筛选 chips + 右侧常驻输出栏），四状态稿（更新中/完成汇总/全部最新/panel 未检查）已确认；由 UI 需求派生的平台小增量：命令可见性 `visibility`、`storage` 能力消费、管理器适配器注册表；本规格为实现依据）
- 基线：`main@8d8bb849`（Phase 3B 收尾；工作树干净）
- UI 起草记录：`.superpowers/brainstorm/41490-1792247466/content/`（layout / layout-v2 / panel-and-output / final-states，light/dark 双态；`.superpowers/` 已在 .gitignore）
- 上游规格：`docs/superpowers/specs/2026-10-11-plugin-system-design.md`（Phase 3A 插件系统；本通道为其 additive 扩展，模式同 `workspace.openTab` 先例）
- 行为参照：Phase 3A 实施记录 `docs/phase3a-plugin-system-record.md`（会话注册表/host gate/桥预算/host-entry.mjs 的既有语义）
- 背景探索发现（2026-10-17，本会话）：
  - 插件 UI 为沙箱单向模型：面板/tab iframe `sandbox` + CSP `connect-src 'none'`（`panel_shell.rs`），UI 无法发网络请求/读 storage/settings/secrets/调用 worker
  - 面板可调 host 方法仅 4 个（`workspace.readContext / workspace.openTab / terminal.sendText / notifications.show`；`gate.rs` `panel_forbidden` 拒绝其余）
  - worker → UI 唯一数据通道是 `workspace.openTab(params)`（打开时快照；params 不同 = 新实例，每 worktree ≤8，无法刷新）
  - 官方插件市场（`stablyai/orca-plugins`）全部为 content-pack（主题/技能/翻译/图标/快捷键），无任何数据驱动 UI 插件——与该限制互为旁证
  - 结论：数据驱动插件（updater、后续 DB 工具/API 客户端）需要「UI↔worker 双向通道」这一共同缺失地基；本项目先建通道，updater 作为首个消费者验证

## 1. 背景与目标

在 Phase 3A 插件系统之上补全**插件 UI↔worker 数据传输通道**（命令 RPC + 会话推送），并以 **updater 插件**（npm 全局包 + Homebrew 更新管理，类似 `topgrade`/`brew outdated` 的图形化）作为首个真实消费者完成 dogfooding。

**验收（自动 + 手工）**：插件 tab 内可实时调用 worker 命令并接收流式推送（检查包更新→列表渲染→全量更新→行级状态/日志→可取消）；右侧栏 panel 可查看管理器状态并配置启用范围（持久化、与已开 tab 实时同步）；通道的 capability 门禁/consent/预算/错误码/命令可见性全链路生效；updater 在真实 npm/brew 环境可用且 UI 与宿主视觉完全一致（light/dark）。自动化门禁：`cargo test --workspace` 全绿 + bindings 新鲜度 + `pnpm typecheck && pnpm build:web` + `pnpm test`（含插件单测与 parity 测试）+ 插件构建脚本 exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| 通道能力 | `ade-plugins`（`manifest.rs`/`types.rs`） | capability 闭集 8→10：`worker:call`、`session:push`（§3.1） |
| 通道方法 | `ade-plugins`（`host/api.rs`/`host/methods.rs`/`host/gate.rs`） | host methods 14→16：`worker.invokeCommand`、`session.post`（§3.1） |
| 命令可见性 | `ade-plugins`（`manifest.rs`）+ 渲染层命令面板 | `CommandContrib.visibility: "palette" \| "internal"`（默认 palette）；palette 过滤 internal、keybinding 引用 internal 在 manifest 校验拒绝（§3.1） |
| 会话推送 | `ade-plugins`（`service.rs`/`sessions.rs` 复用） | 按 token 定位来源会话、校验、预算、事件发射（§3.3/§3.4） |
| worker 宿主 | `src-tauri/resources/plugins/host-entry.mjs` | `invokeCommand` 帧可选 `session` 字段；SDK `ctx.session`（§3.2） |
| bridge | `ade-bridge`（`events.rs`/`plugins_service.rs`） | 新事件 `plugins:panel-push`（发射装配仿 `plugins:open-tab`） |
| TS 契约 | `src/shared/plugins/*`、`src/shared/preload-api/api/plugin-host-api.ts` | capability 联合、panel action 集合、guest 消息 `orca-panel-push`、订阅 `onPanelPush`（契约 24→25）；mock/fixtures 同步 |
| 渲染层 | `PluginPanel.tsx`、`PluginTabContent.tsx`、`GlobalPluginHostView.tsx` | token→iframe 的推送路由 hook；web 端维持无插件面 |
| updater 插件 | `plugins/updater/`（新） | manifest（panel + tab + 命令）、worker（管理器适配器注册表→`dist/main.mjs`）、UI（React 19 + Tailwind v4→单文件 `dist/tab.html` + `dist/panel.html`）、构建脚本、测试（§3.5–3.8） |
| 工程接线 | 根 `package.json`、`vitest.config.ts`、`.gitignore` | `build:updater` 脚本；vitest include `plugins/**/*.test.ts`；dist 不入库（§3.8） |

### 2.2 明确不做（防蔓延）

通用双向 port 通道（UI↔worker 任意消息）；host 侧枚举/广播式推送（worker 可自行记住收到的会话 token 并推给同插件的其他会话——§3.6 会话簿记）；worker.invokeCommand 结果 >64KiB 的分块传输（报错并提示用 push）；面板访问 storage/settings/secrets 的扩权（panel 经 `worker.invokeCommand` 走 worker 代为存取）；面板网络能力；updater 的单包选择更新、wanted/latest 双列、greedy cask、定时/启动后台检查、系统通知、项目依赖（workspace 内 `package.json`）、输出栏拖拽调宽、表格排序列、失败行点击定位日志；npm/pnpm/yarn/bun 之外的 JS 包管理器与 pipx/cargo/mas 等其他管理器（管理器注册表架构支持，v1 仅实现 npm + brew 适配器）；跨插件共享 UI kit（第二个数据驱动插件出现时再抽）；updater 独立 git repo 与市场上架（后续项）；Windows/Linux 平台面（沿 Phase 3A 现状 POSIX-only）。

## 3. 架构

### 3.1 通道：能力与 host methods

**capability 闭集 8→10**（新 kind 仅出现在声明它的插件的 consent 指纹里；既有插件零影响）：

| kind | 语义 | consent 展示文案（草案） |
|---|---|---|
| `worker:call` | 允许插件界面（面板/tab/全局视图）调用该插件已声明的命令 | "允许插件界面调用插件后台命令" |
| `session:push` | 允许插件后台向发起调用的界面会话推送实时数据 | "允许插件后台向界面推送实时数据" |

**host methods 14→16**：

| method | 调用方 | params / result | 语义 |
|---|---|---|---|
| `worker.invokeCommand` | 仅面板/tab/全局视图（`panel_callable=true`） | `{commandId≤256, args?}`（args 序列化 ≤64KiB）/ handler 返回值（序列化 ≤64KiB） | 以会话绑定的插件身份调用其 `commands.register` 注册的处理器；`commandId` 必须是该插件在当前作用域声明过的命令（长度上限同 manifest `validate_command_id`）；沿用 supervisor `invoke 30s` 超时——**长任务 handler 必须立即返回 started，进度经 `session.post` 推送** |
| `session.post` | 仅 worker（`panel_callable=false`） | `{sessionToken, topic?≤64, payload}`（sessionToken 校验同 `panelSessionTokenSchema` ≤128；payload ≤64KiB）/ `{ok:true}` | 向指定 UI 会话推送消息；host 校验 token 有效 + `binding.plugin_key` 等于调用方插件 + kind ∈ {Panel, MainTab, GlobalView} |

- `worker.invokeCommand` 被 worker 自身调用（无 UI 会话上下文）→ `unavailable`；不新增 `worker_callable` 表标志（自调无语义，拒绝方式与"无会话"一致）。
- `session.post` 显式携带 `sessionToken`（而非帧级隐式上下文）：hostCall 帧无需扩展，token 本就是该插件自己的会话令符（worker 进程本就持有全部插件能力，无越权面）；传他插件 token 会因 `plugin_key` 不匹配被拒。
- 方法表（`host/api.rs`）为唯一 source of truth；TS 侧镜像（`shared/plugins/plugin-host-api.ts`）加 capability 联合与 panel action 集合成员；mock fixtures 同步。

**命令可见性（伴随扩展）**：`CommandContrib` 增可选 `visibility: "palette" | "internal"`（缺省 `"palette"`，additive）：

- `internal`：仅供插件 UI 经 `worker.invokeCommand` 调用；命令面板与快速动作过滤隐藏；keybinding 引用 internal 命令 → manifest 校验失败；internal 命令不允许 `action` 别名（其本就指向渲染层 built-in）。
- 动机：数据驱动插件的内部 RPC 命令（updater 的 `snapshot`/`set-managers`）不污染用户命令面；对既有插件零行为变化；commands 不在 consent 指令性身份内，指纹不受影响。
- 契约：插件列表投影的命令条目带 `visibility`（TS 类型 + mock fixtures 同步）；渲染层命令面板/`plugin-quick-actions` 按之过滤。

### 3.2 通道：帧协议与 SDK

- 父→子 `invokeCommand` 帧新增**可选** `session: {token: string}`：仅当调用来自 UI 桥时由 host 注入；命令面板/快捷键触发的调用无此字段。host-entry `parseParentFrame` 放宽该分支（校验 token 为字符串）。
- 子→父无新帧类型：`session.post` 走既有 `hostCall`，结果走既有 `hostResult`。
- SDK（host-entry.mjs `orca` 对象）：命令 handler 签名扩展为 `(args, ctx)`；`ctx.session` 仅在 UI 发起调用时存在，形如 `{ token, post(topic, payload) }`，`post` 为 `host.call('session.post', {sessionToken: ctx.session.token, topic, payload})` 的语法糖。对旧插件 handler（只声明 `args`）additive 兼容。
- 兼容性：host-entry.mjs 与 app 同版本发布（Phase 3A §3.11 现状）；插件只见 SDK 对象，不感知帧细节。

### 3.3 通道：会话归属与推送路由

- 会话注册表（`sessions.rs`）既有 `SessionBinding{plugin_key, kind, panel_id, tab_id, instance_id, scope, repo_id, worktree_id, manifest_revision, owner}` 直接复用；`session.post` 校验后按 token 定位。
- 推送路由：`service.session_post` → 注入式发射器 emit `plugins:panel-push`（载荷 `{sessionToken, topic?, payload}`；装配模式同 `open_tab_requested`/`plugins:open-tab`）→ 渲染层 real 域订阅 `onPanelPush`（契约 24→25）→ 三个宿主组件（`PluginPanel` / `PluginTabContent` / `GlobalPluginHostView`）各自以 token 匹配 → `contentWindow.postMessage({type:'orca-panel-push', topic?, payload}, '*')`（目标源策略与 `orca-panel-init` 相同）。
- guest 侧协议（`src/shared/plugins/plugin-panel-bridge.ts`）：新增 `orca-panel-push` 消息类型 + parse 函数（供插件 HTML 复用/示例）；插件 HTML 自行监听。
- 失效时序：插件禁用/卸载、项目关闭、宿主窗口关闭、dev 重载（revision 变更）→ 既有会话失效路径 → 后续 `session.post` 得 `unavailable`；渲染层 tab 重挂载取新 token，UI 载入时重新 invoke 即重新获得推送目标。

### 3.4 通道：预算、错误与审计

- **预算**：guest→host（panel_action）沿用既有 per-token 64KiB/30 条每 10s；新增 host→guest（push）per-token 64KiB/30 条每 10s，在 service 侧记账；两方向独立预算，互不挤占；watchdog pong 保留 lane 语义不变。
- **错误码**（全部沿用既有集合）：

| 场景 | 码 |
|---|---|
| worker.invokeCommand：命令未声明 | `invalid_params`（面板层翻译——服务层语义码 `unknown_command` 不在面板错误词汇表内，同 `workspace.openTab` 无效 tabId 先例） |
| worker.invokeCommand：结果 >64KiB | `invalid_params` |
| worker.invokeCommand：worker 未激活/errored | `unavailable`（既有 `ensure_active`/supervisor 语义） |
| worker.invokeCommand：崩溃早退 / 30s 超时 / handler 抛错 | `action_failed`（错误文本截断同帧上限；与既有 `invoke_command` 映射一致） |
| worker.invokeCommand：worker 自身调用（无会话上下文） | `unavailable` |
| session.post：token 无效 / 非本插件 / 非 UI kind | `unavailable` |
| session.post：payload/topic 超限 | `invalid_params` |
| session.post：推送预算超限（host 丢弃） | `rate_limited` |
| 面板调 `session.post` | `panel_forbidden` |
| capability 未授予 / consent 缺失 | `capability_denied` / `consent_required`（既有语义） |

- **审计**：`worker.invokeCommand` 对齐既有命令调用（palette `invoke_command`）的审计行为；`session.post` 为非 mutation 高频面，不记 audit，仅按需要计入 worker 日志。
- **consent 指纹**：新增 kind 进入 capabilities 规范化输入；未声明插件指纹不变。consent UI 的 capability 列表组件补两个新 kind 的展示文案。

### 3.5 updater 插件：manifest 与贡献

```json
{
  "manifestVersion": 1,
  "id": "updater",
  "publisher": "itsuka",
  "name": "Package Updater",
  "version": "0.1.0",
  "description": "Check and update global npm packages and Homebrew formulae/casks.",
  "engines": { "orca": ">=0.0.1" },
  "pluginApi": 1,
  "main": "dist/main.mjs",
  "contributes": {
    "tabs": [{ "id": "updates", "title": "Updates", "icon": "package", "entry": "dist/tab.html" }],
    "panels": [{ "id": "updater", "title": "Package Updater", "icon": "package", "entry": "dist/panel.html" }],
    "commands": [
      { "id": "check", "title": "Updates: Check for updates" },
      { "id": "update-all", "title": "Updates: Update all packages" },
      { "id": "cancel", "title": "Updates: Cancel running update" },
      { "id": "open", "title": "Updates: Open updater tab" },
      { "id": "snapshot", "title": "Updates: Get state", "visibility": "internal" },
      { "id": "set-managers", "title": "Updates: Set enabled managers", "visibility": "internal" }
    ]
  },
  "capabilities": [
    { "kind": "worker:call" },
    { "kind": "session:push" },
    { "kind": "workspace:open-tab" },
    { "kind": "storage" }
  ]
}
```

- `icon: "package"` 需与 tab/panel icon curated 映射核对，缺失则取 curated 内最近值（不新增图标资产）。
- `workspace:open-tab` 仅供 `open` 命令；`storage` 供管理器开关持久化（panel 经 worker 命令写入，不经 panel 直调）。
- 命令四个为用户面（palette），两个为 internal（仅 UI RPC，§3.1 命令可见性）。

### 3.6 updater 插件：worker 设计（`src/worker/*.ts` → `dist/main.mjs`）

| 模块 | 职责 |
|---|---|
| `main.ts` | `activate(orca)`：注册命令；`ctx.session` 装配与会话簿记；错误兜底 |
| `parse.ts` | 纯函数：`parseNpmOutdated(stdout)`（`{pkg:{current,wanted,latest,…}}`；注意 exit 1 = 有更新、空 `{}` = 无）、`parseBrewOutdated(stdout)`（v2 `formulae`/`casks`）；输出统一条目 `{manager:'npm'\|'brew', kind:'global'\|'formula'\|'cask', name, current, latest}` |
| `runner.ts` | `runCommand(cmd, args, {onLine, signal})`：spawn（继承 worker 环境，PATH 已由 host 白名单转发）、stdout+stderr 行化、退出码、取消 SIGTERM→2s→SIGKILL |
| `managers.ts` | **管理器适配器注册表**：`ManagerAdapter{id, label, kinds, detect(), outdated(), parse(stdout), upgrades(entries)}`；v1 注册 npm（`npm outdated -g --json` / `npm install -g <name>@latest`）与 brew（`brew outdated --json=v2`，brew auto-update 保留默认 / `brew upgrade <name>`、`brew upgrade --cask <name>`）两个适配器；新管理器 = 新增适配器 + 注册（check/update-all/cancel/UI 全部注册表驱动，不写死管理器） |
| `jobs.ts` | 单飞任务状态机 `idle/checking/updating/done/failed`；最近报告缓存（`lastReport` + `checkedAt`）；管理器开关配置（storage 键 `managers`，缺省=已检测到的全部启用）；会话簿记（记录收到的 session token 按 kind，状态变化时推给全部已知 token，`unavailable` 即剔除）；事件发射经注入接口（便于单测）；日志行缓冲批量推送（≤2Hz 刷、单行截断 4KB、单推 ≤64KiB、总量控制在预算内） |

**命令语义**：

| 命令 | 可见性 | 行为 |
|---|---|---|
| `check` | palette | 任务在跑 → 返回 `{running:true}` 并向调用会话推当前快照；否则启动异步检查并立即返回 `{started:true}`；结果经 `topic:"state"` 推送（每管理器状态、条目、`checkedAt`、错误） |
| `update-all` | palette | 无最新报告先隐式 check；**逐包**执行（注册表驱动的升级命令），每包状态迁移（pending/running/ok/failed）+ 日志推送；单包失败不阻断后续；结束推汇总 `{updated,failed,skipped}`（`topic:"state"`） |
| `cancel` | palette | 有运行中任务 → 杀当前子进程、剩余标 skipped、推取消汇总，返回 `{canceled:true}`；无 → `{canceled:false}` |
| `open` | palette | `workspace.openTab({tabId:"updates"})` |
| `snapshot` | internal | 只读：返回 `{job:{state,progress?}, report:lastReport|null, config:{managers, enabled[], detected[]}}`（不触发网络）；panel/tab 载入时取初始状态 |
| `set-managers` | internal | `{managers:string[]}`：与注册表校验（未知 id → `invalid_params`）→ storage 持久化 → 向全部已知会话推更新后的状态（panel 开关即时同步到已开 tab） |

- 逐包执行的选择理由：包级进度可见、可取消粒度清晰、单包失败隔离；代价（npm 每包一次进程启动）v1 接受。
- 会话同步模型：worker 记住各会话 token（每次 invoke 的 `ctx.session` 更新簿记；含 panel 与 tab）；任何状态变化（检查完成、单包状态迁移、配置变更）推给全部已知 token，令牌失效（`unavailable`）即剔除——满足"panel 开关 → 已开 tab 同步"而无需 host 侧广播（§2.2）。
- 推送 `topic` 约定：`"state"`（报告/汇总/行状态/配置）与 `"log"`（输出行）。

### 3.7 updater 插件：UI 设计（`src/ui/*.tsx` → 单文件 `dist/tab.html` + `dist/panel.html`）

**与宿主完全一致六项约束（定型；两个入口共用同一主题/组件基建）**：

1. 框架：React 19 + Tailwind v4（与宿主同版本，构建复用根仓库依赖）。
2. 主题 token：只消费 shell 注入的 20 个 CSS var（`--background`…`--radius`，`PANEL_DESIGN_TOKEN_ALLOWLIST`）；插件 `@theme inline` 映射块与宿主 `src/renderer/src/assets/main.css` **逐字相同**，由 vitest parity 测试锁死。
3. 暗色机制：逐字复制 `@custom-variant dark (&:is(.dark *))`；shell 已设置 `html class="light|dark"`；宿主主题切换经 `usePluginPanelThemeRevision` remount 机制自动携带新 token，插件不自行实现主题逻辑。
4. 字体：构建时把宿主 `src/renderer/src/assets/fonts/Geist-Variable.woff2`（68KB）内联为 data: URI 进插件 CSS（CSP 仅放行 `font-src data:`）；font-sans 栈与宿主逐字一致（`'Geist', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif`）；mono 用宿主同栈（系统字体）。
5. 组件：插件内做小型 UI kit（Button/Badge/Card/行/表头/开关），class 配方逐字抄宿主 `components/ui/*` 等价物；不引入 daisyUI（宿主没有）。
6. 验收：手工清单含 light/dark 双态与宿主同屏视觉对照。

**Panel（右侧栏，`dist/panel.html`）**：

- 头部：标题 "Package Updater" + 上次检查时间（未检查显示 "not checked"）。
- 动作：`Check now`（invoke `check`）、`Open tab`（invoke `open`）。
- 管理器列表：每个注册表管理器一行——状态点（detected / not installed）、名称、摘要（如 "global · 4 updates" / "formula 3 · cask 2"）、**显示开关**（写 `set-managers` 持久化）。未安装管理器仍可开关；未检查时计数显示 "—"。
- 载入时 invoke `snapshot` 取初始状态；此后由 `state` 推送驱动。底部提示 "No recent check. The tab checks automatically when opened."

**Tab（主区，`dist/tab.html`）**：

- 头部：标题 "Updates" + 上次检查时间 + `Refresh` + `Update all (N)`；更新中 `Refresh` 禁用、主按钮切换为 `Cancel`（destructive 描边）。
- 筛选 chips：`All N` + 每个**启用**管理器（带计数）；点击单管理器过滤表格（panel 开关变更实时同步 chips）。
- 表格：统一表（列 `package / source / version / state`）；source 列显示 npm / brew formula / brew cask 等注册表标签；version `current → latest`（tabular-nums，latest 加粗）；行状态 chip：`✓ updated` / `running…` / `✗ failed` / `pending` / `skipped`（宿主 status 绿/琥珀/红）。
- **右侧常驻输出栏**：约 300px 固定宽（v1 不可拖拽），`Live output` 标题 + 实时日志（等宽字体）；无输出时显示提示文案；失败原因在输出栏可见。
- 完成汇总条：`✓ N updated · ✗ M failed · K skipped · 耗时`，持续显示到下次刷新。
- 状态机：`not-checked → checking（骨架屏）→ report（列表+计数）→ updating（行级状态 + 日志）→ done（汇总条）`；另有 empty（"All up to date"）、error（检查失败 banner + 原始输出尾部；npm EACCES 等附通用提示）、manager-missing（"not detected" 卡片，不影响其他管理器）。
- 不做（后续项）：输出栏拖拽调宽、表格排序、失败行点击定位日志。
- 文案：v1 英文内嵌（与插件生态一致）。

**代码结构**：

- `src/ui/bridge.ts`：宿主桥封装——`invokeCommand(commandId, args)`、push 订阅（`orca-panel-push`）、连接状态（等待 pong 后可用）。
- `src/ui/state.ts`：reducer——连接/检查/更新/配置状态归并；push 的 `state`/`log` 分流。
- `src/ui/kit/*`：本地 UI kit；`src/ui/tab/*` 与 `src/ui/panel/*` 两个入口视图（共享 kit/theme/bridge）。

### 3.8 updater 插件：构建与开发流程

- 源码布局：`plugins/updater/{ade-plugin.json, build.mjs, tsconfig.json, src/worker/*, src/ui/*, test fixtures}`；产物 `dist/{main.mjs, tab.html, panel.html}`（dist 不入库，`.gitignore`）。
- 构建：`plugins/updater/build.mjs`（根 `package.json` 增 `"build:updater"`）——worker 用 esbuild（bundle / platform node / format esm / target node18+）；UI 用 vite 多入口（`tab.html` + `panel.html`，复用根依赖与 `@tailwindcss/vite`；自定义内联插件把 JS/CSS 单文件化进各自 HTML；Geist woff2 内联为 data: URI）。
- 类型：`plugins/updater/tsconfig.json`（worker/UI 分层，禁 UI 引 node types、禁 worker 引 DOM）；根 `typecheck` 脚本增补对该 tsconfig 的 `tsc --noEmit`。
- 测试：根 `vitest.config.ts` include 增 `plugins/**/*.test.ts`；插件单测跑 TS 源（parsers/jobs/runner/parity），不需先构建。
- dev 流程：`pnpm build:updater` → 设置 dev 路径 → 开发中 tab 出现 → 改源码 rebuild → dev watcher（通知+批处理+内容哈希）热重载；dist 内文件数少，无哈希上限压力。
- 独立 repo/市场上架（dist 提交、marketplace 条目、git 源安装链路）为后续项；本项目只保证 dist 可由构建重现。

### 3.9 数据流

**check**：
1. 用户经「+」菜单或命令面板打开 Updates tab → tab 载入 → `invokeCommand('check')`
2. host：token 校验（会话有效）→ gate（`worker:call` + 声明校验）→ ensure worker ready → invoke 帧带 `session{token}`
3. handler 立即返回 `{started:true}`（<30s 约束安全）→ 异步按注册表探测/执行各管理器 outdated
4. 各步 `ctx.session.post("state"|"log", …)` → host 校验/预算 → `plugins:panel-push` → 匹配 iframe → `postMessage`；推送目标 = 全部已知会话 token（panel + 已开 tab）
5. 完成 → 最终 `state` 报告；UI 渲染列表

**panel 载入 / 配置变更**：
1. panel 载入 → invoke `snapshot`（只读）→ 渲染管理器列表与开关；此后由 `state` 推送驱动
2. 开关 toggle → invoke `set-managers` → storage 持久化 → 向全部已知会话推 `state`（已开 tab 的 chips/列表即时同步）
3. panel `Check now` 与 tab 共用同一单飞任务：任务在跑时返回 running 快照，不重复起任务

**update-all / cancel**：同链路；逐包状态迁移与日志高频推送经缓冲节流；cancel 杀子进程并推汇总；tab 关闭后推送 `unavailable` 被静默忽略且 token 剔除，任务继续，新 tab 载入 check 获 running 快照。

**重启/退出**：IDE 退出经既有 supervisor 进程组终止 worker 及其子进程；重启后不自动恢复中断的更新（v1）；管理器开关经 storage 持久化（跨重启保持）。

### 3.10 错误处理与边界（汇总）

- 通道见 §3.4；updater 见 §3.7/3.9；另：畸形 JSON/超时检查→失败 banner + 原始尾部；brew auto-update 慢→started 立即返回不误报失败；日志洪峰→缓冲节流；超长行→截断。
- 会话并发：同 tab 重挂载/多 tab 同开 → 单飞任务 + 快照重推（后调用者获得当前状态，不重复起任务）；已知 token 的 `unavailable` 推送失败即剔除（不重试、不阻断任务）。
- 配置：`set-managers` 未知管理器 id → `invalid_params`（UI 保留旧开关态）；storage 写入失败 → 返回 `action_failed`，panel 显示错误并保留旧值。

## 4. 测试与门禁

**Rust（`ade-plugins` / `ade-bridge`）**：
- `api.rs` 方法表：新增 2 方法的 name/params/result/panel_callable/capability 断言
- gate 矩阵：2 新方法 × panel/worker × 有/无 capability × 有/无 consent（复用 Phase 3A 矩阵测试模式）
- `worker.invokeCommand`：声明校验、无会话上下文拒绝、结果 64KiB 上限、审计对齐、错误映射
- `session.post`：token 校验（无效/他插件/Worker kind）、topic/payload 上限、per-token 限流、事件发射（注入式发射器捕获断言）
- `visibility`：缺省 palette、internal 校验（action 别名拒绝、keybinding 引用拒绝、未知值拒绝）、列表投影携带
- host-entry：`invokeCommand` 带/不带 `session` 的帧解析与 SDK `ctx.session` 行为（沿用 host_entry 集成测试模式）
- 既有 22 命令与非插件面零回归；bindings 新鲜度

**TS（共享层/渲染层）**：
- `plugin-host-api`：capability 联合、panel action 集合（含 `worker.invokeCommand`）、错误码不变、命令条目 `visibility` 类型
- 命令面板/`plugin-quick-actions`：internal 命令不出现（含 keybinding 面不消费）
- `plugin-panel-bridge`：`orca-panel-push` schema/parse 测试
- 桥 host：token→iframe 推送路由测试（沿用 `plugin-panel-bridge-host` 模式）；订阅 `onPanelPush` real 域 parity（24→25）
- web 端维持无插件面（composition 测试不变）

**插件（vitest，跑 TS 源）**：
- `parse`：npm（正常/空 `{}/exit1/畸形）、brew（formulae/casks/空/畸形）夹具
- `managers`：注册表（注册/未知 id 拒绝/检测拼接命令）
- `runner`：假子进程（`node -e`）行流/退出码/取消/超时
- `jobs`：单飞、快照重推、失败隔离、取消置 skipped、会话簿记（更新/剔除）、`set-managers` 持久化（fake storage）、`snapshot` 只读不触发网络
- theme parity：插件主题文件与宿主 `main.css` 的 `@theme inline` 映射、dark variant、字体栈逐字一致

**门禁（终态一次全绿）**：
1. `cargo test --workspace`（`src-tauri/`）
2. bindings 新鲜度（`export-bindings` 后零差异；插件列表 DTO 增 `visibility` 字段需随代码重生成；事件不进 bindings）
3. `rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web`（含插件 tsconfig）
4. `pnpm test`（含插件单测与 parity）
5. `pnpm build:updater` exit 0 + `dist/tab.html` 与 `dist/panel.html` 单文件断言（无外链 script/link、Geist 已内联）

## 5. 手工验收清单

前置：`pnpm build:updater`；`pnpm dev`；系统 `node>=18`；macOS（npm + Homebrew）。

1. 设置 dev 路径 → 开发中 tab 出现 → 插件中心启用（consent 展示 `worker:call`/`session:push`/`workspace:open-tab`/`storage` 四能力）→ 确认
2. 「+」菜单打开 Updates tab → 自动检查（骨架→列表；统一表格 + chips + 右侧常驻输出栏）→ 与 `npm outdated -g` / `brew outdated` 命令行结果一致
3. Update all → 行级状态 + 右侧实时日志；完成后汇总条正确、列表减少；失败隔离核对：若真实场景出现单包失败，确认该项标 failed 且后续包继续（无失败场景时以插件单测覆盖为准）
4. 运行中 Cancel → 当前包终止、剩余 skipped、汇总正确；再次 check 状态一致
5. 运行中关闭 tab → 重开 → 自动接上运行中任务进度（快照重推）；不产生重复任务
6. 右侧栏 panel：管理器列表（状态点/计数/开关）；未检查态 "—"；toggle 开关 → 已开 tab 的 chips/列表即时同步；重启 IDE 后开关保持（storage）；panel `Check now` 与 tab 状态一致；`Open tab` 打开/聚焦
7. light/dark 切换 → panel 与 tab 重挂载后配色/字体/圆角与宿主设置页同屏对照一致（Geist 生效）
8. 命令面板 `Updates: Check/Update all/Open updater tab/Cancel` 可用；`snapshot`/`set-managers` **不出现**在命令面板；`open` 打开/聚焦既有 tab
9. 禁用/卸载插件 → tab/panel 不可用态 → 数据目录/worker 清理；重装后可用
10. 模拟缺 npm 或缺 brew（PATH 裁剪）→ 对应管理器显示 "not detected"（tab 卡片 / panel 状态点），其他管理器正常；未安装管理器开关可切换
11. 面板消息洪峰（更新大包）→ UI 不卡死、无 `rate_limited` 误伤（插件节流生效）

任一项失败：补记到实施记录并修复后重跑对应门禁。

## 6. 风险与偏差备案（预留）

- **帧/SDK 兼容**：`invokeCommand` 帧与 handler 签名的 additive 假设需测试覆盖；host-entry 与 app 同版发布依赖既有约束。
- **命令可见性 additive**：`visibility` 字段与 palette 过滤是新面向；需测试兜住默认行为不变（既有插件命令全部 palette 可见）。
- **推送预算与 watchdog**：host→guest 新预算与既有 pong/data lane 的交互需在测试中固定；实施中发现偏差记录于此。
- **brew auto-update 时长**：检查耗时波动大；started+流式模式已规避 30s 超时，但手工验收需覆盖慢场景。
- **npm 全局权限**：EACCES 环境差异导致更新失败属预期（提示不自动修）。
- **视觉一致性**：parity 测试锁 token 映射，但组件级像素一致依赖手工对照；如有偏差记录于此。
- **实现中新发现偏差**：格式同 Phase 3A 记录 §5.2（实施期间逐条追加）。

## 7. 后续项

- updater 独立 git repo + 市场上架（dist 提交、marketplace 条目、git 源安装/更新/回滚链路 dogfooding）
- 方向 1（DB 工具）与方向 3（API 客户端）作为通道的下一批消费者；届时评估结果分块（>64KiB）与流式增强
- host 侧会话枚举/广播推送（现由插件自记 token 解决；插件数量增长后可平台化）
- 跨插件共享 UI kit 抽包（第二个数据驱动插件出现时）
- updater 增强：单包选择、wanted/latest 双列、greedy cask、定时检查、系统通知、项目依赖、更多包管理器适配器（pipx/cargo/mas 等）、输出栏拖拽调宽、表格排序、失败行点击定位日志
