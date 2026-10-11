# Phase 2 子项目 G：computer-use（桌面控制，macOS 全量自足）设计规格

- 日期：2026-10-11
- 状态：brainstorming 输出（范围 = 全量自足；原生层 = vendor Swift helper 改名复用；agent 接口 = 自有 skill + `orcinus` CLI；可发现性 = 终端 env 注入 + 真实 PATH 安装——均已批准；本规格为实现依据）
- 基线：`main@37e8381f`（Phase 2F 记录）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游规格：`docs/superpowers/specs/2026-09-14-ade-design.md`（§2.1 H「computer-use」保留；§5 `ade-shell` 职责；§8 Phase 2 最后一项；§10 风险 6「必要时降级」——本切片选择不降级）
- 前置事实（2026-10-11 本会话三探索代理，含 file:line 证据）：
  - **oracle 组成**：① `orca:native/computer-use-macos` Swift helper（~5.4k 行 + 测试；AX 树/截屏/输入合成/快照缓存/焦点栅栏；Unix socket 上 NDJSON `{id,method,params,token}`）② `orca:src/main/computer/` 编排（~4.8k 行：spawn/token/重连/协议门/动作校验/verification 归一化/剪贴板 16MiB 上限）③ runtime RPC `computer.*` 15 方法（`orca:src/main/runtime/rpc/methods/computer.ts:34-144`）④ CLI `orca computer` 14 子命令 + `status`/`open`/`skills get computer-use`（`orca:src/cli/`）⑤ 设置页权限面（`orca:src/main/ipc/computer-use-permissions.ts` + `macos-computer-use-permissions.ts`）。
  - **helper 认证**：oracle helper 校验对端必须是 Electron 侧车（`ps` 匹配 `computer-sidecar.js` + bundle ID `com.stablyai.orca*`）——Tauri 进程天然不符，**必须改**（见 §3.1）。
  - **fork 现状**：设置页 `ComputerUsePane`/`ComputerUseSkillSetupPanel` 已移植且挂载（`src/renderer/src/components/settings/`）；`computerUsePermissions` 域不存在（调用 reject）；`cli` 域 = mock（`commandName:'ade'`、全 unsupported）；`skills` 域 = mock（`discover` 恒空 → 安装检测恒「未安装」）；feature-interaction catalog 无 `computer-use` id（2 条债务偿还记录注明「实现时补回」）；PTY env 注入点 `src-tauri/crates/ade-bridge/src/commands/pty.rs:110-140` `build_spawn_env`；fork 无任何 Unix socket 基建；`tauri.conf.json` 无 `resources`/`externalBin`。
  - **打包/签名**：2B.1 已建立 `APPLE_SIGNING_IDENTITY="Apple Development: …" pnpm tauri build` 流程（WWDR G3 已导入）；嵌套 helper 需在外部签名前先签（hardened runtime + entitlements），oracle 参照 `orca:config/scripts/build-computer-macos.mjs`。
  - **技能面**：官方 `computer-use` skill stub 解析 `ORCA_CLI_COMMAND` → `orca`；本切片不劫持该变量（会波及同终端内 orca-cli/orchestration 等其他 skill），改用自有 stub + `ORCINUS_CLI_COMMAND`。
  - **许可**：orca 为 MIT（Lovecast Inc.）——vendor 合法，保留版权头。

## 1. 背景与目标

Phase 2 仅剩 computer-use。本子项目（2G）交付 macOS 全量自足切片：agent 在 Orcinus 终端内通过 `orcinus` CLI 驱动任意本机 app（列 app/窗口、读 AX 快照、点击/输入/滚动/拖拽/设值、截屏），设置页可查看/授予/重置 TCC 权限并一键安装自有用 skill。

**验收（自动 + 手工）**：`orcinus status/open/computer *` 全链路真实可用（经 app 内 RPC → vendored helper）；设置页权限状态/授予/重置真实生效；skill 安装检测可用；PATH 安装可 `which orcinus`。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| 原生 helper（vendored Swift） | `src-tauri/native/computer-use-macos/`（新） | 改名「Orcinus Computer Use.app」；认证改为 token 同 uid（§3.1） |
| 构建脚本 | `config/scripts/build-computer-macos.mjs`（新） | swift build → .app → 签名 → `src-tauri/resources/computer-use/` |
| Provider 内核 | `ade-computer`（新 crate） | 协议类型/路径/可用性/transport/client/动作校验/权限（§3.2–3.4） |
| Runtime RPC server | `ade-computer::rpc`（app 内启动） | Unix socket + runtime.json + 15 `computer.*` + `status.get`（§3.5） |
| CLI | `orcinus-cli`（新 crate → bin `orcinus`） | status/open/skills get/computer 14 子命令（§3.6） |
| 有 skill | `skills/computer-use/SKILL.md`（本仓库新增） | 解析 `ORCINUS_CLI_COMMAND` → `orcinus`（§3.7） |
| 终端 env 注入 | `commands/pty.rs` `build_spawn_env` | `ORCINUS_CLI_COMMAND=<打包/开发路径>`（§3.7） |
| `cli` 域接真 | `ade-bridge` 命令 + `real/cli.ts` + parity | PATH 安装/状态/移除（§3.8） |
| `computerUsePermissions` 接真 | `ade-bridge` 3 命令 + `real/computer-use-permissions.ts` | getStatus/openSetup/reset（§3.4） |
| `skills.discover` 本地最小真实现 | `ade-bridge` 命令 + `real/skills.ts` | 扫描 home 技能根（§3.9） |
| feature-interaction | catalog + RPC server 记录 | 恢复 `computer-use` id（§3.10） |
| 打包 | `tauri.conf.json` `resources` + `externalBin` | helper .app + orcinus bin（§3.11） |

### 2.2 明确不做（防蔓延）

Windows/Linux provider（desktop-script 全族）；WebSocket 远程/`--environment`/配对；`serve`/orchestration envelope；`--worktree` 包围工作树自动解析（仅显式 `--session/--worktree` 透传）；人类可读输出逐字对标（保证可读 + 错误 nextSteps）；oracle 的 osascript 管理员提权安装（不可写则回退 `~/.local/bin`，见 §3.8）；repo 级 skill 根扫描（仅 home 根）；`skills` 域其余方法（install/remove/freshness/share 等维持 fallback）；emulator/浏览器相关 skill 指南（仅内嵌 computer-use 指南）；helper 上游后续修复自动同步（手工 cherry-pick）；Swift helper 测试进门禁（`swift test` 手动，记录）。

## 3. 架构

### 3.1 原生 helper（vendored Swift，改名）

- **源码**：整树拷贝 `orca:native/computer-use-macos` → `src-tauri/native/computer-use-macos/`（含 `Sources/`、`Tests/`、`Package.swift`)。改名点：产物名/可执行文件 `orcinus-computer-use-macos`、显示名「Orcinus Computer Use」、bundle ID 默认 `dev.itsuka.orcinus.computer-use`（信息 plist 由构建脚本生成，可 `ORCINUS_COMPUTER_MACOS_BUNDLE_ID` 覆盖）；handshake 返回的 `provider` 字段改 `orcinus-computer-use-macos`；权限设置窗文案中的 Orca 字样改为 Orcinus。
- **认证改动（唯一实质逻辑改动）**：`main.swift` 对端校验去掉 Electron 侧车命令路径与 `com.stablyai.orca*` bundle ID 判据，改为：**token 严格匹配（主）+ `LOCAL_PEERPID` 同 uid 活性检查**；socket 目录 0700、socket 0600 保持不变。其余（协议、方法、快照缓存 32 条/2 分钟、焦点栅栏、验证回读、设置窗）原样。
- **构建**（`config/scripts/build-computer-macos.mjs`，移植 oracle 脚本）：`swift build -c release`（arm64；universal 可选留待后续）→ 组装 `Orcinus Computer Use.app/Contents/{MacOS,Info.plist}` → plist：`CFBundleIdentifier`/`CFBundleName`/`LSUIElement=true`（与 oracle 一致；设置窗由 setActivationPolicy(.regular) 前台化）等 → 签名：`APPLE_SIGNING_IDENTITY` 存在时 `codesign --force --options runtime --timestamp --entitlements <vendored entitlements>`，否则 ad-hoc `codesign --force --sign -`（记录：ad-hoc 每次构建 CDHash 变化 → TCC 需重新授权）→ 输出 `src-tauri/resources/computer-use/Orcinus Computer Use.app`。`package.json` 加 `"build:computer-macos"`。
- **运行时解析**（`ade-computer::paths`）：`ORCINUS_COMPUTER_MACOS_HELPER_APP_PATH` 覆盖 → 打包 `resource_dir()/computer-use/Orcinus Computer Use.app` → 开发 `src-tauri/native/computer-use-macos/.build/release/Orcinus Computer Use.app`。
- **可用性**：macOS 14+（Darwin ≥ 23）且 helper 存在；否则 `provider_unavailable` 文案（区分「平台不支持」/「未构建 helper，运行 pnpm build:computer-macos」）。

### 3.2 Provider transport 与 client（`ade-computer`）

- **cli 无关纯 Rust**（不依赖 tauri/store；deps：serde/serde_json/thiserror/libc/rand）。
- **transport**（对齐 `orca:src/main/computer/macos-native-provider-transport.ts`）：首次请求按需 spawn `…/orcinus-computer-use-macos --agent <socket> --token-file <token>`（detached、stdio ignore）；父进程建 `$TMPDIR/orcinus-computer-use-XXXXXX`（0700）、`provider.sock`、`provider.token`（0600，连接成功后删除）；连接重试 100ms×10s；请求 60s 超时（超时 → 错误 `action_timeout` + 整体重启）；NDJSON 行协议；重启后旧 socket 迟到错误静默丢弃（generation 守卫）；`shutdown()` 发送 `{method:'terminate'}` 并清理 socket 目录。
- **client**（对齐 `macos-native-provider-client.ts`）：id 关联请求/响应；首次连接先 `handshake`，`protocolVersion != 1` → `provider_incompatible`；方法集 `handshake,listApps,listWindows,getAppState,click,performSecondaryAction,scroll,drag,typeText,pressKey,hotkey,pasteText,setValue,terminate`；写失败/socket 关闭/错误 → 重启 + 单次重试；串行化调用（单飞队列，保持 oracle 「一次一个」语义）。
- **动作预校验**（对齐 `computer-provider-action-validation.ts` + `shared/computer-use-key-spec.ts`，Rust 重写）：app 必填；`windowId XOR windowIndex`；click `elementIndex XOR (x,y)`、`clickCount` 正整数、`mouseButton ∈ left/right/middle`、modifiers 键规格；scroll direction ∈ up/down/left/right、pages 正数；drag 两端 element XOR 四坐标；pressKey 单键、hotkey ≥1 修饰键 + 恰 1 键；paste 16 MiB UTF-8 上限（错误文案对齐）；非法 → `invalid_argument`。
- **verification 归一化**：action 缺 `verification` 时按 `path` 补 `unverified{reason:`synthetic_input`/`clipboard_paste`/`accessibility_action_unasserted`}`（对齐 26 行参照逻辑）。
- **错误目录**：`COMPUTER_ERROR_CODES` 15 码（fork 已有 `src/shared/computer-use-runtime-types.ts` 同名单，Rust 侧同集）；client 附加 `provider_incompatible`/`unsupported_capability`/`action_timeout`。

### 3.3 权限（`ade-computer::permissions`）

- **状态**：`open -n <helper.app> --args --permission-status-file <tmp>` → 轮询 50×100ms → 读 `{accessibility, screenshots}`；helper 缺失 → `helperUnavailableReason` + 空 permissions。
- **授予**：刷新状态 → 计算 `nextStep` 文案 → `pkill -f 'orcinus-computer-use-macos --permission(s)?'` 清旧设置窗 → `open -n <helper.app> --args --permission <id>` 或 `--permissions`（打开 helper 设置窗 + 系统设置跳转）。
- **重置**：`tccutil reset Accessibility|ScreenCapture <bundleId>`；bundleId 经 `PlistBuddy -c 'Print :CFBundleIdentifier' <app>/Contents/Info.plist` 读取，失败回退默认。
- 结果形状对齐 `src/shared/computer-use-permissions-types.ts`（fork 已有）。

### 3.4 Tauri 命令（`computerUsePermissions`）

`computer_use_permissions_get_status() -> ComputerUsePermissionStatus`、`computer_use_permissions_open_setup({id?})`、`computer_use_permissions_reset() -> {…, bundleId}`；specta 注册 + bindings 重生成；`real/computer-use-permissions.ts` 显式 3 方法 + parity（`create-api.ts` `RealDomains` 加 `'computerUsePermissions'`）。`openSetup` 成功后调 feature-interaction 记录（`computer-use-setup`，renderer 既有调用保持不变）。

### 3.5 Runtime RPC server（app 内）

- **位置**：`ade-computer::rpc`（纯 Rust 模块，接受 `RpcServerConfig{runtime_id, socket_path, metadata_path, auth_token}` + `FeatureRecorder` 回调 trait）；`ade-bridge` 在 `AppState::initialize` 组装（socket/metadata 路径 = Tauri app data dir `~/Library/Application Support/dev.itsuka.orcinus/`，`ORCINUS_USER_DATA_PATH` 覆盖；`auth_token` = 24 字节随机 hex），`Exit` 时关闭并从磁盘移除 metadata。
- **metadata**：`runtime.json`（0600）：`{runtimeId, pid, transports:[{kind:"unix", endpoint}], authToken, startedAt}`——字段名与 oracle `shared/runtime-bootstrap.ts` 同形（CLI 端逻辑同构，便于对照）。
- **socket**：`runtime.sock`（0600）；NDJSON；请求 `{id, authToken, method, params}`；限制：1 MB 消息、30s 空闲断开、32 并发连接、keepalive `{"_keepalive":true}\n`（10s 发送）；响应 `{id, ok:true, result, _meta:{runtimeId}}` / `{id, ok:false, error:{code,message,data?}}`；`bad_request`/`unauthorized`/`request_too_large`/`runtime_busy` 对齐 oracle 准入错误。
- **方法**：`computer.capabilities`（触发 provider 初始化并缓存 handshake 能力；helper 未构建/不可用 → 错误 `provider_unavailable` + 构建提示，不合成占位）、`computer.listApps/listWindows/getAppState/click/performSecondaryAction/scroll/drag/typeText/pressKey/hotkey/pasteText/setValue`（直通 provider）、`computer.permissions`（openSetup 语义）、`computer.permissionsStatus`（状态；web 客户端契约已在等）、`status.get`（`{app:{running:true, pid, desktopWindowStatus:'available'}, runtime:{state:'ready', reachable:true, runtimeId, appVersion}, graph:{state:'not_ready'}}`——graph 恒占位）。
- **feature-interaction 记录**：`computer.permissions` → `computer-use-setup`；其余 `computer.*`（除 `capabilities`/`permissionsStatus`）→ `computer-use`；经 `FeatureRecorder` 注入（app 侧落 `UiStateStore::record_feature_interaction_in_memory`）。

### 3.6 CLI（`orcinus-cli` → `orcinus`）

- **依赖**：`ade-computer`（协议/错误码/结果类型）+ serde/serde_json + clap（derive；XOR/条件校验的自定义层保证错误文案与 oracle 一致）；不依赖 tauri/store。
- **通用**：`--json` 输出信封 `{id, ok:true, result, _meta:{runtimeId}}`；失败 `{id:"local", ok:false, error:{code,message,data?}}` 且 exit 1；人类输出可读 + 错误 nextSteps（`computer-use-error-recovery` 移植）；`--help`/`--version`。
- **连接**：读 `ORCINUS_USER_DATA_PATH` 或默认 app data dir 的 `runtime.json` → Unix socket NDJSON（uuid id + authToken）；缺失/不完整/连接失败 → `runtime_unavailable`（人读文案「Orcinus is not running. Run 'orcinus open' first.」）；`status.get` 1s 超时；请求默认 60s。
- **命令面**（对标 oracle `specs/computer.ts`；flag 名全同）：
  - `status --json`：不启动；metadata 缺失 → 合成结果 `{app:{running:false}, runtime:{state:'not_running'|'stale_bootstrap', reachable:false}}`；可达 → `status.get` 投影；人类模式 `!reachable` exit 1（同 oracle）。
  - `open --json`：`ORCINUS_OPEN_COMMAND`（shell）> `ORCINUS_APP_EXECUTABLE` > 默认 `open -a Orcinus`（dev 场景引导 `pnpm dev` 或环境变量）；轮询 250ms×15s 至可达，超时 `runtime_open_timeout`。
  - `skills get computer-use`：内嵌 fork 版指南（`skill-guides/computer-use.md`，构建期 include_str!）；`--json` → `{name, full, markdown}`；未知 topic → `invalid_argument`（列可用 topic）。
  - `computer permissions [--id accessibility|screenshots]`：→ RPC `computer.permissions`；结果 `{platform, helperAppPath, openedSettings, launchedHelper, permissionId?, nextStep?, permissions?}`。
  - `computer`：`capabilities`、`list-apps`、`permissions`（见上）、`list-windows --app`、`get-app-state --app`、`click`（`--element-index XOR (--x+--y)`、`--click-count`、`--mouse-button`、`--modifiers`）、`perform-secondary-action`（`--element-index --action`）、`scroll`（element XOR coords、`--direction`、`--pages`）、`drag`（两端 XOR）、`type-text`（`--text XOR --text-stdin`）、`press-key`、`hotkey`、`paste-text`、`set-value`（`--element-index`、`--value XOR --value-stdin`，可空串）；共同目标 flag：`--app`（必需，`pid:<n>`/名称/bundle id）、`--session`/`--worktree`（互斥、显式透传）、`--window-id`/`--window-index`、`--restore-window`、`--no-screenshot`。
  - 客户端预校验与 oracle 文案一致（`Click requires --element-index or both --x and --y` 等，Rust 移植）。
  - `--json` 截图落盘：`result.screenshot.data`（base64）写入 `$ORCINUS_COMPUTER_SCREENSHOT_TMPDIR` 或 `$TMPDIR/orcinus-computer-use/<id>-screenshot.png`（0600），置 `path`/`dataOmitted:true`/`expiresAt(+24h)`，并做 >24h 清扫；写失败保留内联。
- **不做**：WebSocket/远程、orchestration envelope、`serve`、`--worktree` 自动解析（透传显式值；指南注明）。

### 3.7 自有 skill 与可发现性

- **stub**（新文件 `skills/computer-use/SKILL.md`，本仓库分发）：解析顺序 `ORCINUS_CLI_COMMAND` → PATH `orcinus`；**禁止回退到 `orca`**（不同产品，明确报错停步）；引导 `orcinus skills get computer-use` 取版本匹配指南；`orcinus status --json` / `orcinus open --json`。
- **指南**（`skill-guides/computer-use.md`，新文件）：移植 oracle 版（`skill-guides/computer-use.md`）并适配：`ORCA`→`orcinus`、去掉浏览器/orchestration/worktree 自动解析措辞、保留 snapshot→act→snapshot 工作法/动作优先级/错误恢复。
- **终端 env 注入**：`build_spawn_env` 追加 `ORCINUS_CLI_COMMAND=<path>`（打包：`Contents/MacOS/orcinus`；开发：`target/{debug,release}/orcinus`，存在才注入；不覆盖调用方已给值）；Rust 单测覆盖。
- **达方式**：`npx skills add https://github.com/ItsukaKotori/orcinus --skill computer-use --global`（`agent-feature-install-commands.ts` 常量改 URL；update 命令不变式 `npx skills update computer-use --global`）。

### 3.8 `cli` 域接真（PATH 安装）

- `ade-bridge` 命令：`cli_get_install_status`、`cli_install`、`cli_remove`（桌面 3）+ `cli_get_wsl_install_status`/`cli_install_wsl`/`cli_remove_wsl`（恒 `{state:'unsupported', unsupportedReason:'platform_not_supported'}`）；`real/cli.ts` 6 方法 + parity。
- 语义（对齐 oracle `src/main/cli/`，简化版）：`commandName='orcinus'`；mac 目标：`/usr/local/bin/orcinus` 目录存在**且可写**则用之，否则 `~/.local/bin/orcinus`（`ORCINUS_CLI_INSTALL_PATH` 覆盖）；symlink 指打包/开发二进制；状态判定 installed/stale（同路径指向旧目标）/conflict（他物占位，拒绝覆盖）/unsupported；`pathConfigured` = 扫描 `PATH` 是否含目标目录；**不修改 shell profile**（同 oracle POSIX 语义）；remove：symlink 校验后删除 + legacy 清理。
- 打包二进制来源：Tauri `externalBin`（见 §3.11）→ `Contents/MacOS/orcinus`；开发 = 构建产物路径。

### 3.9 `skills.discover` 本地最小真实现

- `ade-bridge` 命令 `skills_discover({refresh?})`：扫描 home 技能根（移植 oracle `skill-discovery-sources.ts` 的 home 表：codex/claude/agents/grok/opencode/pi/omp/hermes/prime-agent/gemini(+antigravity)/cursor/droid/continue/trae/aug + `codex-plugin-cache`）中 `<root>/<dir>/SKILL.md`；名称取 frontmatter `name` 或目录名；`.system` 下 → `bundled`；返回 `{skills, sources, scannedAt}`（字段满足 fork hook 读取面：name/directoryPath/skillFilePath/sourceKind/sourceLabel/providers/owner 等；freshness 类字段给最小合法值）。
- `real/skills.ts` 仅显式 `discover`（其余维持 fallback）+ parity；`refresh:true` 绕过内存缓存（TTL 10s）。
- 边界：repo 级根、WSL/远端、install/remove/freshness 不进本切片。

### 3.10 feature-interaction 恢复

- catalog 恢复 `computer-use` id（union + 定义 + `expectedIds`），保留 `computer-use-setup`；生产调用方 = RPC server 记录路径（Rust）+ 既有 renderer `computer-use-setup`；`feature-interactions.test.ts` 的「生产调用方」ratchet 扫描面扩展为同时接受 `src-tauri/crates/ade-computer/src` 下含字面量 `"computer-use"` 的方法映射（如 `map_rpc_method_to_feature_interaction`）。

### 3.11 打包与签名

- `tauri.conf.json`：`bundle.resources: ["resources/computer-use/**"]` + `bundle.externalBin: ["binaries/orcinus"]`（Tauri sidecar 命名 `binaries/orcinus-aarch64-apple-darwin`，由构建流程拷贝）；`src-tauri/resources/computer-use/` 与 `src-tauri/binaries/` gitignore（构建产物）。
- 顺序：`pnpm build:computer-macos`（swift + 签 helper）→ `pnpm tauri build`（含外部签名）；记录 ad-hoc 开发期 TCC 重授权代价与真签名路径。
- 开发流：`pnpm build:computer-macos` 后 `pnpm dev`；helper 解析 fallback 命中 `.build/release`，无需打包也可用。

## 4. 数据流

1. agent（Orcinus 终端）→ `orcinus computer …` → 读 runtime.json → socket 请求 → app RPC server → 动作校验/provider client → helper Unix socket（NDJSON）→ AX/截屏/输入 → 结果沿路回归（verification 归一化 + 信封）→ CLI `--json` 打印（截图落盘）。
2. 设置页 → `computerUsePermissions.getStatus/openSetup/reset` → Tauri 命令 → permissions 模块（open helper/tccutil）。
3. 技能安装：面板按钮 → 终端 `npx skills add ItsukaKotori/orcinus --skill computer-use --global` → OSC133 完成 → `skills.discover({refresh:true})` → 检测已安装。
4. PATH 安装：面板/`cli.install()` → 命令 → symlink + 状态回报。
5. app 退出：RPC server 关闭 → metadata 移除 → helper terminate。

## 5. 错误处理与边界

| 场景 | 行为 |
|---|---|
| app 未运行/元数据缺失 | CLI `runtime_unavailable`（「Run 'orcinus open' first.」），exit 1 |
| helper 未构建 | provider_unavailable + 构建提示；`computer.capabilities` 返回占位能力 |
| handshake 协议不符 | `provider_incompatible` |
| 请求超时 | `action_timeout` + provider 重启 |
| 元素过期/窗口失焦/权限拒绝等 | helper 原错误码透传（15 码目录） |
| paste 超 16 MiB | `invalid_argument`（文案对齐） |
| socket 伪造/token 不符 | RPC server `unauthorized`；helper 拒连 |
| 并发超限/超长消息 | `runtime_busy`/`request_too_large` |
| TCC 未授权 | 状态 not-granted；动作报 `permission_denied`/`accessibility_error` |
| `cli.install` 目标冲突 | 状态 `conflict`，拒绝覆盖 |
| 非 macOS | `unsupported`（本切片 macOS-only 前提） |

## 6. 测试与门禁

### 6.1 Rust

- **transport/client**：fake helper（测试内脚本化 NDJSON Unix socket：握手/方法回显/延迟/断连/坏 token）→ 覆盖 spawn 参数、重试、60s 超时（缩短注入）、错误重启、generation 守卫、terminate 清理。
- **action validation**：oracle 文案矩阵（click/scroll/drag/key/paste/selectors）。
- **verification 归一化 / 错误目录**：全码表 + path→reason 映射。
- **permissions**：状态文件 fixture 解析；bundleId PlistBuddy 解析（fixture plist）；命令构造断言（`open`/`tccutil` 经注入 runner 校验 argv）。
- **rpc server**：临时 socket 集成（auth 通过/拒绝、信封形状、1MB/30s/32 连接界限、keepalive、方法分派、feature 记录回调、metadata 生命周期）。
- **CLI 集成**：`cargo test -p orcinus-cli` 起 fake runtime server（临时 metadata + socket）→ 用 `env!("CARGO_BIN_EXE_orcinus")` + `std::process::Command` 跑 `status/open/computer */skills get`，断言 JSON 形状、错误信封、exit code、截图落盘。
- **env 注入**：`build_spawn_env` 单测（存在/不存在/不覆盖）。
- bindings 新鲜度 + 命令清单（新增命令全登记）。

### 6.2 TS

- `real/cli.ts`/`real/computer-use-permissions.ts`/`real/skills.ts` 路由断言 + parity 更新；`create-api` RealDomains；`agent-feature-install-commands` URL 断言更新；catalog/ratchet 更新；ComputerUsePane/ComputerUseSkillSetupPanel 既有测试适配（mock 升级为真实域 stub；行为不变）。

### 6.3 手工验收（最后统一）

1. `pnpm build:computer-macos` 成功（helper 建立并签名）→ `pnpm dev` 起 app；
2. 设置页 Computer Use 显示两权限状态；点授予 → helper 设置窗/系统设置跳转；授权后刷新变 Granted；重置按钮生效（回 not-granted）；
3. 面板安装 skill（本仓库源）→ 终端完成 → 检测「已安装」；
4. `cli.getInstallStatus` → install → `which orcinus` 命中 symlink；remove 清理；
5. Orcinus 终端内 `orcinus status --json`（reachable:true）；`orcinus skills get computer-use` 输出指南；
6. agent 实操：`orcinus computer list-apps --json`、`get-app-state --app Finder --json`、`click --element-index …`、`type-text`、截图落盘（`path` 非空）；
7. app 退出后再跑 computer 命令 → `runtime_unavailable`；`orcinus open` 拉起后恢复；
8. feature-interaction：跑若干 computer 命令后 UI 状态记录含 `computer-use`。

### 6.4 门禁

`cargo test --workspace`、`pnpm typecheck && pnpm build:web`、`pnpm test`；bindings 新鲜度。Swift helper 构建/测试不在门禁内（工具链依赖；`swift test` 与 `pnpm build:computer-macos` 手动执行并记录）。

## 7. 风险与偏差备案

1. **helper 认证放宽**：token 严格 + 同 uid 对端检查替代 Electron bundle 判据（Tauri 进程身份不符的必然改动；socket 权限 0600 + 目录 0700 保持）。
2. **上游同步**：vendored Swift 与 oracle 上游漂移需手工 cherry-pick（改名/认证 diff 保留本地小面）。
3. **ad-hoc 签名**：开发期每次构建 CDHash 变化 → TCC 需重新授权（真签名路径已备；记录）。
4. **门禁不含 helper**：macOS 工具链不假设常驻；fake helper 覆盖 provider 逻辑；真实 helper 验证归手工验收与 `swift test`。
5. **CLI 面裁剪**：无远程/WebSocket/serve/orchestration/worktree 自动解析；`--session/--worktree` 仅透传；人类输出非逐字对标。
6. **skills.discover 范围**：仅 home 根（无 repo 根/远端）；字段最小合法集（freshness 类占位）——面板安装检测够用。
7. **PATH 安装简化**：无 osascript 提权分支（不可写回退 `~/.local/bin`）；不写 shell profile（同 oracle）；非 macOS unsupported。
8. **`computer-use` 幂等记录**：RPC 侧按方法名映射记录（同 oracle 语义）；ratchet 测试扩展说明。
9. **打包依赖手序**：helper 构建是 `tauri build` 前置（文档化；未构建时 provider_unavailable 而非构建失败）。
10. **截图临时目录**：使用 `ORCINUS_COMPUTER_SCREENSHOT_TMPDIR`/`$TMPDIR/orcinus-computer-use/`（不复用 `ORCA_*` 名，避免与原版互踩）。
