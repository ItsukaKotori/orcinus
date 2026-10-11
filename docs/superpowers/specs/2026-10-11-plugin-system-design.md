# Phase 3 子项目 A：插件系统全链路（双作用域 + 主区插件 Tabs）设计规格

- 日期：2026-10-11
- 状态：brainstorming 输出（范围 = 全量含市场+开发中 tab；secrets = macOS Keychain；kill list = 跳过；**主内容区自定义 tab = 纳入**——均已批准；本规格为实现依据）
- 基线：`main@f14764b3`（Phase 2G 记录）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§6.1–6.3、§7 插件系统、§8 Phase 3）
- 前置事实（2026-10-11 本会话三探索代理）：
  - **oracle 面（`orca:src/main/plugins/` ~13.8k 行，110 文件）**：manifest v1（zod，`orca-plugin.json`，≤1MiB）、capabilities 闭集 7 种、sha256 树哈希（`orca-plugin-tree-v1\0` + 长度帧 + 排序路径）、staging→provenance(0600)→指针→lockfile 原子写/回滚、consent 指纹（capabilities+worker+教学内容身份）、kill list（远程）、marketplace（git 源 + 官方种子 `stablyai/orca-plugins#main` + preview/install/rollback + official trust 三重校验）、dev watcher、Node 子进程 worker（fork + ELECTRON_RUN_AS_NODE + `plugin-host-entry.js`；帧协议 zod 双端；supervisor 3 重启 backoff；ready 10s/invoke 30s/event 5min/idle 5min/max 5 workers/grace 2s）、iframe panel bridge（shell CSP、session token 32B、预算 64KiB/30msgs/10s、host 竞争 13 methods、panel 可调仅前 3）、storage/settings/secrets（Electron safeStorage）/audit JSONL。
  - **oracle 无项目作用域**：仅 userData 全局；fork §7.1 新增项目级（`<repo>/.ade/plugins/<key>`、`<repo>/.ade/plugin-data/<key>`、提交 `<repo>/.ade/plugins.lock.json`）与双作用域遮罩/独立 consent。
  - **fork 渲染层已全量移植**（插件中心三 tab/设置段/consent 对话框/安装对话框/市场浏览与源管理/快捷键面板/语言包/面板 shell `PluginPanel.tsx` 全桥实现/右侧栏 activity 项 + scope 过滤/命令与快捷键消费/cmd-j 快速动作）；`plugins` 域仍 mock（`create-api.ts` 固定 mock；fixtures 里 `PluginScope` 为临时 hack）；**无任何 lockfile 提示 UI、安装对话框无作用域选择、无主区插件 tab**；`GlobalPluginHostView` 为占位。
  - **主区 tab 模型**：`src/shared/tab-types.ts` `TabContentType`（terminal/editor/diff/conflict-review/check-details/agent-session/browser）+ `WorkspaceVisibleTabType` 折叠、`Tab{id,entityId,groupId,worktreeId,contentType,label,isPinned,...}`、tabs slice 模块化（分组/拖拽/MRU/最近关闭/水合）、tab bar 创建入口 `TabBarCreateEntry.tsx`。
  - **Rust 侧**：无 `ade-plugins`；data_dir = Tauri app data（`ORCINUS_USER_DATA_PATH` 覆盖）；缺 git clone 能力（ade-git 仅 porcelain，runner 模式可参照复制）、缺 sha2/hex 直接依赖（lock 内可启用）、无 keychain、无 reqwest 直依赖（市场为 git 无需 HTTP；kill list 跳过也不需要）；`precheck.rs` 进程组 SIGTERM→SIGKILL 为子进程监管先例；`hydrate_login_path`（preflight）可复用于找 `node`。
  - **Electron 耦合处**（需替换）：`safeStorage`→Keychain、`ipcMain plugins:*`→Tauri 命令、`app.getPath('userData')`/`isPackaged`→Tauri 路径、`webContents.send('plugins:changed')`→事件、`fork`→系统 node、导航守卫/owner 生命周期→Tauri 窗口事件、parcel utility process watcher→notify。

## 1. 背景与目标

Phase 3 先做插件子系统（浏览器降级版另立项）。交付**插件从安装到面板/主区 tab 运行的全链路**，含 fork 特有的双作用域、项目 lockfile 一键安装与**主内容区自定义 tab**（插件可声明并在 UI 中打开自己的 tab，如数据库插件打开查询控制台）。

**验收（自动 + 手工）**：local/git/市场三源安装 → consent → 启用 → worker 激活 → 右侧栏面板可用且 host 调用（readContext/terminal.sendText/notifications/storage/secrets/settings）真实生效；项目级安装写入提交 lockfile、`.ade` 自动 deny、新克隆按 lockfile 一键安装；插件面板按钮可打开主区 tab（多实例/参数透传）；市场浏览/安装/回滚可用；开发中 tab 热重载。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| 内核 crate | `ade-plugins`（新） | manifest/hash/发现/staging/发布/lockfile/consent/启用/完整性/worker/host methods/gate/面板会话/storage/settings/secrets/audit/市场/watcher（§3.1–3.8） |
| Worker 宿主 | `src-tauri/resources/plugins/host-entry.mjs`（新，资源） | 移植 oracle 纯 Node entry+runtime；系统 `node` 运行（§3.2） |
| Tauri 命令 | `ade-bridge` ~22 命令 + `plugins:changed` 事件 | 对齐 preload 名 + 项目 lockfile 面（§3.9） |
| TS 域 | `real/plugins.ts` 21 方法 + create-api/parity | web 端维持无 `plugins`（composition 测试不变） |
| 渲染层 | scope 真数据/安装作用域/ lockfile 提示/市场/开发中/主区 Tabs | 既有面板 shell 复用（§3.10） |
| 主区 Tabs | tab 模型 + `PluginTabContent` + 新建菜单 | fork 扩展（§3.6） |
| 打包 | `resources/plugins/host-entry.mjs` 入包 | 无外部二进制（§3.11） |

### 2.2 明确不做（防蔓延）

远程 kill list（记录：consent/哈希/惰性完整性三重防线仍在；kill 入列阻止语义不做）；bundled 插件（schema 仍识别 `bundled` 源）；vmRecipes 执行与 agent-profiles 消费（schema/artifact/指纹保留，作为后续项）；插件命令 alias 打开 tab（仅 UI 菜单 + host method）；`workspace.closeTab/setTabTitle` 等后续方法；SSH/远端/overlay/relay 插件安装；菜单重建与 i18n 控制面扩展（语言包注册列表已由渲染层消费）；市场私有认证；Windows；`plugin-host-conformance` 的 relay 半边。

## 3. 架构

### 3.1 `ade-plugins` crate（纯 Rust，不依赖 tauri）

模块（每模块独立测试）：

- **`manifest.rs`**：`ade-plugin.json`（主）+ `orca-plugin.json`（兼容读）；≤1MiB 流式上限；字段/交叉校验照 `orca:src/shared/plugins/plugin-manifest*.ts`（id 保留名、semver、engines 仅 `>=x.y.z`、contributes 上限、artifact 相对路径安全、capabilities 闭集+去重规范化）；**增 `contributes.tabs`（≤64，`{id,title,icon?,entry}`）**；`qualifiedPluginKey = publisher.id`；`is_plugin_git_url`（HTTPS/SSH、无 user/password）。
- **`content_hash.rs`**：sha256 `orca-plugin-tree-v1\0` + 8 字节 BE 长度帧 + 排序相对路径；symlink 拒绝；跳过根 `.git`；≤2000 文件 / 50MiB。deps：sha2/hex/walkdir。
- **`discovery.rs`**：全局安装目录 + 项目安装目录 + dev 路径（`devPluginPaths` 设置）；dev 遮罩 installed；同 scope 同 key 不得重复；无效项投影不含 dev 绝对路径。
- **`artifact.rs`**：声明产物校验（realpath containment + 单类上限：worker 50MiB/panel 10MiB/tab 10MiB/icon 2MiB/languagePack 5MiB/vmRecipe 256KiB/agent 1MiB）。
- **`consent.rs`**：指纹 = `sha256-<base64>`（capabilities 规范化 + `\0trusted-node-worker`（有 main）+ 教学内容身份（有 keybindings/vmRecipes/agents 时））逐字移植；**tabs 与 panels 同为沙箱 UI 贡献、不入教学内容身份**；激活态 `disabled > pending(mismatch) > approved`；fork consent 键按 `global:<key>` / `project:<repoId>:<key>`。
- **`install/`（子模块）**：staging 管线（manifest→引擎门→artifact→hash→trust→原子 rename→rehash/reparse/revalidate）、`publication.rs`（provenance 0600→指针→lockfile；失败回滚指针）、`pointer.rs`（current 指针 ≤128B）、`lockfile.rs`（v1 条目 `{pluginKey,version,source,resolvedCommit,contentHash,consentFingerprint,installedAt}`，legacy `capabilityHash` 回读；损坏→空不致命；全局 `<app_data_dir>/plugins.lock.json` + 项目 `<repo>/.ade/plugins.lock.json`）、`provenance.rs`、`atomic_write.rs`（tmp+rename+Windows 重试照抄）、回滚（当前+1 保留）。
- **`enablement.rs`**：approve 校验 `reviewedFingerprint == 当前指纹`；enable 不绕过 consent；kill 摘要跳过。
- **`integrity.rs`**：惰性重验（worker 启动/面板与 tab 读取前）。
- **`worker/`（子模块）**：`spawn.rs`（系统 node + 参数/环境白名单 + cwd=插件根）、`protocol.rs`（帧 serde 双端模型，照 `orca:src/shared/plugins/plugin-host-protocol.ts`）、`supervisor.rs`（3 重启 backoff [500,2000,5000]ms、状态机、idle reap 5min、max 5、grace 2s、manifestRevision 热重载）、`manager.rs`（一插件一 worker、FIFO slot pool、lazy 激活）。
- **`host/`（子模块）**：`gate.rs`（capability×panel 纯函数，错误码 `unknown_method/invalid_params/capability_denied/consent_required/panel_forbidden/rate_limited/unavailable/action_failed`）、`methods.rs`（**14 methods**：oracle 13 + `workspace.openTab`；参数/结果校验与限额照 spec 表）、`services.rs`（由 bridge 注入：活动 worktree 上下文、终端列表/发送、通知、storage/settings/secrets、事件订阅、tab 打开回调）、`audit.rs`（JSONL，10MiB 轮转，mutation attempt→ok/error）。
- **`storage.rs` / `settings_store.rs`**：per-plugin JSON（0600；key 保留名；值 256KiB/总 5MiB/1024 keys）。
- **`secrets.rs`**：**macOS Keychain**（service `dev.itsuka.orcinus.plugin-secrets`，account `<pluginKey>:<key>`）；不可用 fail-closed（对齐 oracle safeStorage 语义）；删除空值语义照 oracle。
- **`sessions.rs`**：面板/tab 会话 token（32B base64url）绑定 `{pluginKey, sessionKind: panel|tab|global-view, panelId?/tabId?/instanceId?, scope, worktreeId?, manifestRevision}`；owner 生命周期失效。
- **`marketplace/`（子模块）**：`store.rs`（sources.json + snapshots/<sourceId>.json）、`service.rs`（add/remove/refresh/list；官方种子 `https://github.com/stablyai/orca-plugins.git#main`）、`installer.rs`（preview=checkout 固定 commit + inspect 返回 `{resolvedCommit,contentHash,consentFingerprint,manifest}`；install 要求 commit 未变否则拒绝）、official trust（URL+owner `stablyai`+`stablyai.orca-*` 身份+URL 前缀三重校验）、rollback。
- **`git.rs`**：自建 runner（argv execFile、超时/取消 10ms poll、kill+reap，仿 `ade-git/src/runner.rs`）；`clone --depth 1 [--branch ref]` / `init+fetch --depth 1 <sha>+checkout FETCH_HEAD` / `rev-parse HEAD` / `ls-remote`。
- **`watcher.rs`**：notify（仿 ade-fs 批处理）监视 dev 路径 → 重发现 + revision 变化 → 热重载 + 事件。
- **`events.rs`**：插件事件源投影（`worktree.created/removed`、`agent.status.changed`，payload 照 `orca:src/shared/plugins/plugin-events.ts` 有界投影）→ 订阅者 worker 投递（动态订阅仅对运行中 worker）。
- **`service.rs`**：总编排（发现→状态投影→激活协调→事件→退出清理）；`ServiceEventSink` 仿 automations。

### 3.2 Worker 宿主与协议（fork 适配）

- 资源文件 `host-entry.mjs`（移植 `orca:src/main/plugins/plugin-host-entry.ts` + `plugin-host-runtime.ts` 语义：file-URL import 插件 main、`activate(orca)`/`deactivate`、SDK `{commands.register, events.on, host.call, grantedCapabilities, log}`、未捕获异常→fatal+exit(1)）。
- Rust 以系统 `node` spawn：`node <resource>/plugins/host-entry.mjs`，环境白名单（PATH/HOME/LANG/TZ/TMP*），**stdio NDJSON 帧**（偏离 oracle 的 Node `fork` IPC + advanced serialization；帧内容不变，仅传输层，记录）。`node` 解析：登录 PATH 水合（preflight 先例）+ `ORCINUS_NODE_PATH` 覆盖；要求 `node >= 18`（版本探测，低于→errored+提示）。
- 帧（父→子）：`init{pluginId,pluginRoot,mainEntry,grantedCapabilities}`、`invokeCommand{callId,commandId,args?}`、`deliverEvent{eventId,event,payload}`、`hostResult{callId,ok,value?,errorCode?,error?}`、`shutdown`；（子→父）：`ready{commands[]}`、`commandResult{callId,ok,value?,error≤8192}`、`eventAck{eventId}`、`hostCall{callId,method,params?}`、`log{level,message≤8192}`、`fatal{error≤8192}`。
- 限额/监管照 oracle：ready 10s、invoke 30s、event 5min、idle reap 5min、max 5 workers、grace 2s、≤64 待投事件；crash 重启 ≤3 次 backoff [500,2000,5000]，超限状态 `errored`；一插件一 worker。

### 3.3 双作用域模型

- 路径（`{appData}` = Tauri app data dir，即 `<app_data_dir>`；`ade/` 子层由 app data 目录本身承担）：
  - 全局：`<app_data_dir>/plugins/<key>/{<hash>/,current}`、`<app_data_dir>/plugin-data/<key>/{storage.json,settings.json,audit.log(.1)}`、`<app_data_dir>/plugins.lock.json`
  - 项目：`<repo>/.ade/plugins/<key>/...`、`<repo>/.ade/plugin-data/<key>/...`、提交 `<repo>/.ade/plugins.lock.json`；安装时若无 `<repo>/.ade/.gitignore` 则写入 `plugins/`、`plugin-data/` deny
- 遮罩：同 key 项目级遮罩全局（列表单条，`scope:'project'`）；右侧栏只显示项目级、左 rail/全局视图只显示全局级（§6.3）。
- consent/禁用态（main-owned settings）键：`global:<key>` / `project:<repoId>:<key>`；两作用域独立同意；项目级首次启用必须显式确认（对话框加「项目插件代码来自仓库、视为不可信」提示）。
- 生命周期：启动加载全局已启用；项目注册（ProjectsStore 现存集合）加载该项目 enabled 项目插件；项目移除终止其 worker/会话。空闲回收兜底。
- lockfile 一键安装：渲染层新面（§3.10），Rust 提供 lock 状态与批量安装命令。

### 3.4 安装四源

local-path / git（`git.rs`） / marketplace（§3.7） / bundled（跳过）。全部经 §3.1 staging 管线；安装需 scope（项目级需 repoId）；项目级/全局各自写对应 lockfile；hash 目录已存在则校验既有字节而非重指；更新=同管线；卸载=安装目录+数据目录（realpath 校验）+lock 条目。

### 3.5 Host methods（14）与 capability

| method | params | result | cap | panel/tab |
|---|---|---|---|---|
| workspace.readContext | `{}` | `{branch,displayName,terminals[{id}]}`或 null | workspace:read | 是 |
| workspace.openTab（新） | `{tabId,title?,params?}` | `{instanceId,worktreeId}` | workspace:open-tab | 是 |
| terminal.sendText | `{terminalId,text≤4096,enter=false}` | `{accepted}` | terminal:send | 是 |
| notifications.show | `{title≤120,body≤1000?}` | `{delivered}` | notifications:show | 是 |
| storage.get/set/delete/keys | 照 oracle | 照 oracle | storage | 否 |
| secrets.get/set/delete | 照 oracle（set ≤64KiB） | 照 oracle | secrets | 否 |
| settings.get/set | 照 oracle | 照 oracle | settings:own | 否 |
| events.subscribe | `{events[]}` | `{subscribed[]}` | events:subscribe | 否 |

- capabilities 闭集扩第 8 项 `workspace:open-tab`；**worker 可调全部 14 方法**；panel/tab 可调集合 = readContext/openTab/sendText/notifications（4，超集外返回 `panel_forbidden`）。
- `workspace.openTab` 语义：解析上下文 worktree（tab 会话=绑定的 worktree；面板/worker=活动 worktree）；`tabId` 必须属于该插件已启用作用域的 `contributes.tabs` 否则 `invalid_params`；`params` ≤64KiB JSON、`title` ≤128；同 worktree 内 `(pluginKey,tabId,params 深比较)` 命中→聚焦既有实例并返回；否则新开（每插件每 worktree ≤8 实例；速率 ≤4 次/10s→`rate_limited`）；审计。
- `workspace.readContext`/`terminal.sendText`：比对 worktree 内终端 id 白名单（照 oracle handler）。

### 3.6 面板与主区 Tabs

- **面板（右侧栏）**：后端产出 `readPanelEntry({pluginKey,panelId,scope?,repoId?})` → shell HTML + token；`panelAction` 走 §3.5 gate + admission（预算渲染侧已有，main 侧再校验 token/owner/manifestRevision/panel 存在）。
- **全局插件视图**（左 rail `plugin:<key>`）：`GlobalPluginHostView` 接真（shell + 会话，上下文=活动 worktree）。
- **主区 Tabs（fork 扩展）**：
  - tab 模型：`TabContentType` + `'plugin'`（`WorkspaceVisibleTabType` 同）；Tab 增可选 `pluginTab?: {pluginKey, tabId, scope, repoId?, instanceId, titleHint?, params}`；`entityId = <pluginKey>/<tabId>/<instanceId>`；label 默认贡献 title/覆盖 titleHint；icon 取贡献（curated 映射+fallback）；水合兼容（旧状态无该字段照常）。
  - 渲染：`PluginTabContent`（复用面板 shell/iframe/桥；session 绑 worktree）；`orca-panel-init` 消息在 pong 后下发 `{params}`（≤64KiB，随 tab 持久化）。
  - 打开入口：① 新建 tab「+」菜单「插件 Tab」分组（当前 worktree 可用 scope 的 tabs，默认参数）；② host method `workspace.openTab`（§3.5，核心联动）。
  - 关闭/固定/分组/最近关闭沿用既有 tab 语义；插件禁用/卸载/项目移除→对应 tab 显示不可用态并可关闭。

### 3.7 市场（git-based）

- 源：sources（`{sourceId,url,ref,addedAt}`）+ 快照（`orca-marketplace.json` 解析：entries/trust）；官方源自动种子；增/删/刷新（clone 或 ls-remote + 取 marketplace 文件）；official trust 三重校验照 oracle。
- preview：checkout 固定 commit + inspect（hash/fingerprint/manifest 投影）；install：要求 marketplace commit 与插件 commit 均未变，scope 选择，经 §3.4；rollback：上一 hash。
- 渲染：设置内 Market 浏览器/源管理对话框与中心市场 tab 接真数据（既有组件）。

### 3.8 开发中 tab 与 dev watcher

- `devPluginPaths`（设置已存在）→ 每路径 notify 递归（批处理 150/500ms）→ 重发现；manifestRevision 变化→运行中 worker 热重载（stop→start 新 revision）；dev 遮罩 installed；同 key 双 dev 路径冲突→该项 invalid 投影。
- 开发中 tab：列出 dev 插件（状态/路径省略绝对路径或截断显示）、启用/重载按钮。

### 3.9 Tauri 命令与事件

- 命令（`plugins_*`，对齐 preload 语义）：`list/list_language_packs/consent/set_enabled/read_panel_entry/invoke_command/panel_action/install/list_marketplaces/add_marketplace/remove_marketplace/refresh_marketplaces/list_marketplace_plugins/preview_marketplace_plugin/install_marketplace_plugin/preview_marketplace_update/rollback_marketplace_plugin/remove/get_logs/refresh`（20）+ 新增 `get_project_lock_state({repoId})`、`install_missing_from_lockfile({repoId})`；事件 `plugins:changed`（payload 照 `plugin-change-event.ts` 扩展 `{contentPacksChanged, pluginsChanged?}`）。
- 契约扩展（`src/shared/preload-api/api/plugin-host-api.ts` + `src/shared/plugins/*`）：`PluginHostListEntry` 增 `scope:'global'|'project'` 与 `repoId?`；`PluginRef`（pluginKey+scope+repoId?）用于 consent/setEnabled/remove/install；列表条目增 `tabs[{id,title,icon?}]`；`PluginHostInstallSource` 增 scope；新 lock 结果类型；panel action 集合增 `workspace.openTab`；capability 联合增 `workspace:open-tab`。mock 同步（fixtures scope 变真字段，删 SAFETY cast）；web 端不变。
- bindings 重生成 + freshness。

### 3.10 渲染层改动清单

1. `PluginHostListEntry.scope` 真字段：`store/slices/plugin-center/plugin-center-slice.ts` 不再从 mock fixtures 取类型；`plugin-panel-activity-items.ts` 过滤改用真 scope+repoId。
2. 安装（local/git/市场）对话框增加**作用域选择**（global/project；无活动项目时 project 禁用）；consent 对话框展示 scope 与项目不可信提示。
3. **lockfile 提示**：插件中心 installed tab 顶部 banner（「当前项目 lockfile 有 N 项未就绪」）+ 弹层逐条（key/version/source/status：installed/missing/hash-mismatch/source-unavailable）+ 「安装缺失」批次动作；无活动项目不显示。
4. 市场 tab 与设置市场对话框接真；开发中 tab 列出 dev 插件 + 重载动作。
5. 主区 Tabs（§3.6）：tab 模型/渲染/新建菜单/水合/不可用态。
6. `GlobalPluginHostView` 接真。
7. 既有测试适配（scope 语义、fixtures 迁移、插件面板空态、快捷键/命令消费不变）。

### 3.11 打包与依赖

- `src-tauri/resources/plugins/host-entry.mjs` 提交入库；tauri.conf resources 加 `{"resources/plugins/": "plugins/"}`（映射记法，同 computer-use）。
- 新 Rust deps：`sha2/hex/walkdir`（lock 内版本）+ `keyring`（macOS Keychain；统一 crate 接口）+ `notify`（工作区已有 ade-fs 版本）；marketplace 无需 HTTP（git only）。
- node 前提：系统 `node >= 18`（`ORCINUS_NODE_PATH` 可覆盖）；缺失/低版本→插件 worker 状态 `errored` + UI 提示。

## 4. 数据流

1. 安装：UI（对话框/市场/lockfile 批量）→ 命令 → staging 管线 → 发布（指针+lockfile）→ `plugins:changed` → 列表刷新。
2. 启用：consent（指纹核对）→ settings 写 → 激活协调（全局/项目范围）→ 配置声明命令/事件时按需激活 worker。
3. 命令：快捷键/cmd-j/面板 → `invoke_command` → gate → worker `invokeCommand` → `commandResult`。
4. 事件：bridge（worktree 生命周期/agent 状态）→ 投影 → 订阅者 worker `deliverEvent` → ack/超时。
5. 面板：`read_panel_entry` → shell+token → 宿主消息 → `panel_action` → gate → 执行 → 回传。
6. 主区 tab：菜单或 `workspace.openTab` → tab 模型新增 → `PluginTabContent` 会话 → `orca-panel-init` 参数 → 插件页 host 调用。
7. 市场：源刷新→快照；preview→固定 commit inspect；install→校验未变→§4.1；rollback→上一 hash。
8. 开发：notify → 重发现 → 热重载。

## 5. 错误处理与边界

| 场景 | 行为 |
|---|---|
| manifest 非法/超限 | 安装拒绝 `invalid_manifest`（投影 invalid 条目带 error） |
| 引擎门不符 | `engine_incompatible`（拒绝安装/启用） |
| consent 指纹变化 | 激活态 `pending`/`needsReconsent`；enable 拒绝 |
| 项目插件未同意 | 保持 disabled；不自动启用 |
| lockfile 损坏 | 视为空（不致命） |
| lockfile 条目源不可用/哈希不符 | 状态列 `source-unavailable`/`hash-mismatch`；批量安装跳过并列明 |
| node 缺失/低版本 | worker `errored` + 提示（含安装/覆盖方式） |
| worker 崩溃 | 重启 ≤3（backoff），超限 `errored`，pending 拒绝 |
| gate 拒绝 | 稳定错误码（§3.5） |
| openTab 未声明 tabId/超限/超速 | `invalid_params`/实例上限返回既有聚焦或 `rate_limited` |
| 面板 owner 消失 | 会话失效，action 拒绝 `unavailable` |
| Keychain 不可用 | secrets 调用 fail-closed（`unavailable`） |
| 项目关闭（移除） | 终止其 worker 与会话；tab 不可用态 |

## 6. 测试与门禁

### 6.1 Rust

- manifest/指纹/content-hash/lockfile（双作用域）/staging/回滚/atomic_write/artifact/path-safety/保留名校验（移植 oracle 测试锚）。
- worker：脚本化 fake node 宿主（NDJSON 帧回放/延迟/崩溃/超时/热重载）+ supervisor/限额/lazy 激活。
- host gate/methods 全矩阵（14 方法 × 错误码 × panel 权限 × 限额）；storage/settings/secrets（fake keychain trait）。
- marketplace：临时 git 仓库（marketplace+plugin 两仓）preview/install/commit 变化拒绝/rollback/trust 校验。
- dev watcher（tempdir 变更→重发现）；events 投影与投递；项目生命周期（注册/移除）。
- openTab：声明校验/实例上限/深比较去重/速率限制/上下文 worktree 解析。
- 命令清单 + bindings 新鲜度。

### 6.2 TS

- `real/plugins.ts` 21 方法路由 + 新增 2 lockfile 方法；scope/锁面契约形状；mock 更新；parity 更新。
- 渲染层：安装对话框作用域选择、lockfile banner/弹层、市场/开发 tab、tab 模型扩展（创建/水合/关闭/不可用态）、GlobalPluginHostView；既有插件测试适配并全绿。
- web composition 仍不含 `plugins`。

### 6.3 手工验收

1. local 安装一个测试插件（带 panel+tab+command）→ consent → 启用 → 右侧栏面板可交互（readContext/sendText/notifications）；
2. 面板按钮 `workspace.openTab` → 主区新 tab（带参数），重复打开聚焦既有实例；「+」菜单可手动打开；
3. git 源安装 + 更新 + 回滚；
4. 项目级安装 → `<repo>/.ade/plugins.lock.json` 提交、`.ade/.gitignore` deny；模拟新克隆（删 `.ade/plugins`）→ 插件中心提示 → 一键安装；
5. 市场：添加自定义 git 源→刷新→浏览→preview→安装→回滚；
6. 开发中：设置 dev 路径→开发中 tab 出现→改插件文件→worker 热重载；
7. secrets 写入/读取（Keychain）→ 重启用仍可读；
8. 禁用/卸载→worker 终止、面板/tab 不可用态、数据目录与 lock 条目清理。

### 6.4 门禁

`cargo test --workspace`、`pnpm typecheck && pnpm build:web`、`pnpm test`；bindings 新鲜度。

## 7. 风险与偏差备案

1. **Node 运行时依赖**：worker 需系统 `node >= 18`（不内置；缺失→errored+提示）；插件生态按 Node ESM 语义。
2. **传输层偏离**：NDJSON stdio 替代 Node `fork` IPC（帧模型不变；helper 同模式先例）；`plugin-host-conformance` relay 半边不移植。
3. **kill list 跳过**：无远程吊销；consent/哈希/惰性完整性保留（记录安全差距+后续项）。
4. **bundled 跳过**：不随包分发插件；schema 仍识别。
5. **vmRecipes/agents 保留不消费**：schema/artifact/指纹保留兼容（consent 预览仍展示）；执行与消费为后续项。
6. **项目作用域为 fork 新面**：oracle 无参照；遮罩/独立 consent/lockfile 语义按 §7.1–7.3 设计实现；「项目打开/关闭」以 ProjectsStore 注册集合近似（记录）。
7. **主区 Tabs 为 fork 新面**：manifest `contributes.tabs`、`workspace:open-tab` capability、`workspace.openTab`、`orca-panel-init` 均无 oracle 参照；扩展面全部记录进契约与指纹。
8. **Keychain**：ad-hoc 签名开发期重建可能触发授权弹窗（真签名缓解）；fail-closed 语义同 oracle。
9. **市场为 git-only**：无 HTTP API/私有认证；官方 trust 三重校验照搬；自定义源第一类。
10. **host-entry.mjs 上游同步**：手工 cherry-pick（fork 适配很小）。
11. **性能**：发现并发 8、哈希 2k 文件上限、watcher 批处理；大插件树首装耗时披露。
12. **web 端维持无插件面**：composition 测试锁定。
