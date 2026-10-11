# Phase 2G computer-use（桌面控制，macOS 全量自足）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** agent 在 Orcinus 终端内经 `orcinus` CLI 驱动任意本机 app（AX 快照/点击/输入/滚动/拖拽/设值/截屏），设置页权限与 PATH 安装接真，自有 computer-use skill 全链路可用。

**Architecture:** vendored Swift helper（改名 + token 认证）→ 新 crate `ade-computer`（协议/路径/provider/权限/RPC server）→ app 内 Unix socket runtime RPC → 新 crate `orcinus-cli`（bin `orcinus`）；TS 侧 `computerUsePermissions`/`cli`/`skills` 三域接真。参照实现 `/Users/itsuka/CodeSpace/orca`（只读），`orca:` 前缀均为其内路径。

**Tech Stack:** Rust（ade-computer、orcinus-cli、serde/serde_json/thiserror/uuid/rand/libc/clap）、Swift（vendored helper，仅构建）、TypeScript（Vitest、Tauri IPC/specta）。

**Spec:** `docs/superpowers/specs/2026-10-11-computer-use-design.md`（执行者须同时阅读）

## Global Constraints

- Rust 依赖：`ade-computer` 新增 `serde/serde_json/thiserror/uuid/rand/libc`；`orcinus-cli` 新增 `clap(4, derive)/serde/serde_json/uuid/libc`；均为 workspace 常规依赖；**不新增任何 npm 依赖**。
- 新 crate 放 `src-tauri/crates/`（`members=["crates/*"]` 自动加入）；`ade-computer` 不依赖 tauri/specta/ade-store；`orcinus-cli` 不依赖 tauri。
- bindings 唯一生成方式：`cargo run -p ade-bridge --bin export-bindings`（workdir `src-tauri`）；`bindings_are_fresh` 校验；新命令名进 `specta_export.rs` 两处清单。
- 提交信息中文 conventional + `Co-Authored-By: Claude Code <noreply@anthropic.com>`。
- cargo 命令在 `src-tauri` 下执行；pnpm/node 在仓库根。
- 仅 macOS/POSIX；可选字段省略不物化 null。
- oracle 逐字对齐面：动作校验错误文案、错误码集合、CLI 信封形状、runtime metadata 字段名——**执行时从 `orca:` 参照文件复制原字符串**，不凭记忆写。
- **测试不得依赖 Swift helper 与真实 app**：provider 走 fake helper（脚本化 NDJSON Unix socket，`tests/common/mod.rs`），CLI 走 fake runtime server；Swift 构建（T1）与 `pnpm tauri build`（T13）例外，属该任务显式验证步骤。
- 渲染层既有测试全绿是硬门禁（ComputerUsePane/技能面板行为不变）。

---

### Task 1: vendored Swift helper（改名 + 认证）+ 构建脚本 + 资源接线

**Files:**
- Create: `src-tauri/native/computer-use-macos/**`（整树拷贝自 `orca:native/computer-use-macos`）
- Modify: `src-tauri/native/computer-use-macos/Sources/**`（改名/认证点，见 Step 2）
- Create: `config/scripts/build-computer-macos.mjs`
- Modify: `package.json`（`"build:computer-macos"` 脚本）
- Modify: `src-tauri/tauri.conf.json`（`bundle.resources` 加 `"resources/computer-use/**"`）
- Modify: `.gitignore`（`src-tauri/native/computer-use-macos/.build/`、`src-tauri/resources/computer-use/`）

**Interfaces:**
- Produces（后续任务依赖，名称固定）：
  - 产物 app：`<repo>/src-tauri/resources/computer-use/Orcinus Computer Use.app`，bundle ID `dev.itsuka.orcinus.computer-use`，可执行 `Contents/MacOS/orcinus-computer-use-macos`。
  - helper 启动参数保持 `--agent <socket> --token-file <token>`；权限参数 `--permission <id>`/`--permissions`/`--permission-status-file <path>`（原样）。
  - handshake 返回 `provider:"orcinus-computer-use-macos"`、`protocolVersion:1`。

- [ ] **Step 1: 拷贝树 + .gitignore**

```bash
cp -R /Users/itsuka/CodeSpace/orca/native/computer-use-macos src-tauri/native/computer-use-macos
```

`.gitignore` 追加（构建产物不入库）：

```
src-tauri/native/computer-use-macos/.build/
src-tauri/resources/computer-use/
src-tauri/binaries/
```

- [ ] **Step 2: 改名 + 认证改动（唯一实质逻辑改动）**

在拷贝树中执行（用 Grep 找点，逐处确认）：
1. **认证放宽**：`Sources/OrcaComputerUseMacOS/main.swift` 对端校验（参照 `orca:native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift:4080-4114` 的 `computer-sidecar.js` + `com.stablyai.orca*` 判据）：删除 Electron 命令路径与 bundle ID 判据，保留 **token 严格匹配（主防线）+ `LOCAL_PEERPID` 同 uid 活性检查**；socket 目录 0700/socket 0600 不动。
2. **provider 名**：handshake 的 `"provider"` 字面量 → `orcinus-computer-use-macos`。
3. **显示名/文案**：设置窗与权限文案中的 Orca → Orcinus（`grep -ri "orca" Sources/` 全过一遍；`com.stablyai.orca.computer-use` 默认 bundle ID 若出现在 Swift 内 → 改 `dev.itsuka.orcinus.computer-use`；**产品名/可执行名由构建脚本的 Info.plist/输出路径决定，Swift 内不写死可执行名**）。
4. `Package.swift` 目标名可保留 `OrcaComputerUseMacOS*`（内部符号，不改动以最小化 diff）——构建脚本产物名与之解耦。

- [ ] **Step 3: 构建脚本**

`config/scripts/build-computer-macos.mjs`（移植 `orca:config/scripts/build-computer-macos.mjs` 骨架；Electron 特有部分删除）：

```js
// 1) swift build -c release（workdir src-tauri/native/computer-use-macos）
// 2) 组装 out/Orcinus Computer Use.app/Contents/{MacOS,Info.plist}
//    Info.plist: CFBundleIdentifier=ORCINUS_COMPUTER_MACOS_BUNDLE_ID||dev.itsuka.orcinus.computer-use,
//                CFBundleName=Orcinus Computer Use, CFBundleExecutable=orcinus-computer-use-macos,
//                LSMinimumSystemVersion=14.0、LSUIElement=true（与 oracle 一致；设置窗由 setActivationPolicy(.regular) 前台化）
// 3) 拷贝 .build/release/orcinus-computer-use-macos → Contents/MacOS/
// 4) entitlements 从 orca:resources/build/entitlements.computer-use.mac.plist 拷贝为
//    config/scripts/entitlements.computer-use.mac.plist（如参照存在）
// 5) 签名：APPLE_SIGNING_IDENTITY 有 → codesign --force --options runtime --timestamp --entitlements ...；
//    否则 codesign --force --sign -（打印 ad-hoc 警告：TCC 需重新授权）
// 6) 输出到 src-tauri/resources/computer-use/ 并打印 codesign -dv 摘要
```

`package.json` `scripts` 加：`"build:computer-macos": "node config/scripts/build-computer-macos.mjs"`。

- [ ] **Step 4: 验证（本任务专项）**

```bash
pnpm build:computer-macos
codesign -dv "src-tauri/resources/computer-use/Orcinus Computer Use.app" 2>&1 | grep Identifier   # dev.itsuka.orcinus.computer-use
file "src-tauri/resources/computer-use/Orcinus Computer Use.app/Contents/MacOS/orcinus-computer-use-macos"  # Mach-O arm64
cd src-tauri/native/computer-use-macos && swift test 2>&1 | tail -5
```

预期：构建 exit 0、产物存在、`swift test` 全绿（改名/认证后编译通过）。若 `swift test` 存在仅依赖 Electron 身份的用例，删除该用例并在 commit message 记录（不得改断言放水）。

- [ ] **Step 5: 提交**

```bash
git add -A src-tauri/native config/scripts package.json src-tauri/tauri.conf.json .gitignore
git commit -m "feat(computer): vendor Swift helper 改名 Orcinus Computer Use 并改 token 认证"
```

---

### Task 2: `ade-computer` 骨架 + 协议类型 + 路径/可用性

**Files:**
- Create: `src-tauri/crates/ade-computer/Cargo.toml`
- Create: `src-tauri/crates/ade-computer/src/lib.rs`
- Create: `src-tauri/crates/ade-computer/src/protocol.rs`
- Create: `src-tauri/crates/ade-computer/src/paths.rs`
- Create: `src-tauri/crates/ade-computer/src/availability.rs`
- Test: 同 crate `#[cfg(test)]`

**Interfaces:**
- Produces（后续任务依赖，名称固定）：
  - `protocol::{ComputerError, ComputerErrorCode}`：15 码枚举（`app_not_found/app_blocked/window_not_found/window_not_focused/window_stale/provider_incompatible/unsupported_capability/permission_denied/element_not_found/element_not_clickable/action_not_supported/value_not_settable/action_timeout/screenshot_failed/accessibility_error`）+ `provider_incompatible`/`unsupported_capability`/`action_timeout` 复合使用；`as_str()` 与 `src/shared/computer-use-runtime-types.ts` 全同。
  - `protocol::COMPUTER_RPC_METHODS: [&str; 15]` = `["computer.capabilities","computer.listApps","computer.permissions","computer.permissionsStatus","computer.listWindows","computer.getAppState","computer.click","computer.performSecondaryAction","computer.scroll","computer.drag","computer.typeText","computer.pressKey","computer.hotkey","computer.pasteText","computer.setValue"]`。
  - `protocol::RUNTIME_METADATA_FILE="runtime.json"`、`RUNTIME_SOCKET_FILE="runtime.sock"`；`protocol::RuntimeMetadata{runtime_id,pid,transports:Vec<RuntimeTransport>,auth_token,started_at}`（serde camelCase：`runtimeId/pid/transports[{kind:"unix",endpoint}]/authToken/startedAt`；序列化与读取双用）。
  - `paths::helper_app_path(env_override: Option<&str>, resource_dir: Option<&Path>) -> Option<PathBuf>`：override → `resource_dir/computer-use/Orcinus Computer Use.app`（存在才返回）→ 开发回退 `Path::new(env!("CARGO_MANIFEST_DIR")).join("../../native/computer-use-macos/.build/release/Orcinus Computer Use.app")`（存在才返回）。
  - `paths::helper_executable(app: &Path) -> PathBuf` = `<app>/Contents/MacOS/orcinus-computer-use-macos`；`paths::helper_bundle_id(app: &Path) -> Option<String>`（读 Info.plist 文本，简单字符串提取；权限模块用）。
  - `availability::provider_unavailable_message(reason: ProviderUnavailable, helper: Option<&Path>) -> String`（平台不支持 / 未构建 helper→提示 `pnpm build:computer-macos`）；`enum ProviderUnavailable { PlatformUnsupported, HelperMissing }`。

- [ ] **Step 1: crate 骨架**

`Cargo.toml`（镜像 ade-automations 惯例）：

```toml
[package]
name = "ade-computer"
version = "0.0.1"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
rand = "0.8"
libc = "0.2"
```

`lib.rs`：`pub mod protocol; pub mod paths; pub mod availability;`（后续任务追加模块）。

- [ ] **Step 2: 写失败测试 → 实现 → 通过**

测试锚：
1. `ComputerErrorCode::as_str()` 15 码与 TS 目录逐字一致（测试内列全比对）。
2. `RuntimeMetadata` serde 往返：字段名 camelCase；`transports` 数组形状。
3. `helper_app_path` 优先级：env override（指向真实临时目录建 `Orcinus Computer Use.app`）> resource_dir > dev 回退；不存在 → None。
4. `helper_executable`/`helper_bundle_id`：临时 app 目录 + 手写 Info.plist fixture。

Run: `cargo test -p ade-computer`（预期 RED → GREEN）。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-computer src-tauri/Cargo.lock
git commit -m "feat(computer): ade-computer 骨架、协议类型与 helper 路径解析"
```

---

### Task 3: transport（spawn/socket/token/NDJSON/重试/超时/重启/terminate）+ fake helper 测试基建

**Files:**
- Create: `src-tauri/crates/ade-computer/src/socket.rs`
- Create: `src-tauri/crates/ade-computer/src/transport.rs`
- Modify: `src-tauri/crates/ade-computer/src/lib.rs`
- Create: `src-tauri/crates/ade-computer/tests/common/mod.rs`（fake helper：脚本化 NDJSON Unix socket server）
- Create: `src-tauri/crates/ade-computer/tests/transport.rs`

**Interfaces:**
- Produces：
  - `socket::connect_with_retry(path: &Path, attempt_interval: Duration, max_attempts: u32) -> io::Result<UnixStream>`（默认 100ms×100）。
  - `transport::HelperTransport::start(helper_app: &Path, request_timeout: Duration) -> Result<Self, ComputerError>`：建 `$TMPDIR/orcinus-computer-use-XXXXXX`（0700）、`provider.sock`、`provider.token`（0600，48 hex）；spawn helper（`--agent <sock> --token-file <token>`，stdio 忽略、detached）；连接成功即删 token 文件；失败路径 kill 子进程并清理。
  - `transport::HelperTransport::request(&mut self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, ComputerError>`：信封 `{"id":n,"method":m,"params":p,"token":t}` 单行 + `\n`；响应 `{"id":n,"ok":true,"result":…}` / `{"id":n,"ok":false,"error":{"code","message"}}`；**任何 IO/超时错误 → 标记 generation 失效 + 重启 helper**（本请求返回错误；超时映射 `action_timeout`）。
  - `transport::HelperTransport::shutdown(&mut self)`：发 `{"method":"terminate"}`（尽力）→ kill → 删 socket 目录。
  - `transport::helper_request_timeout() -> Duration`（60s；测试可注入）。
  - `tests/common`：`FakeHelper::start()` → `{socket_path, token_path, join_handle, requests_rx, respond_with(script)}`；支持：握手响应、按方法回显、延迟、断开、拒 token。
- 语义要点（对齐参照）：重启后旧连接迟到响应按 id+generation 丢弃（`ignore_stale` 语义，`orca:src/main/computer/macos-native-provider-transport.ts:24-46,100-115`）。

- [ ] **Step 1: 写失败测试（fake helper 驱动）**

锚：
1. `start` 后 fake helper 收到 argv `--agent <sock> --token-file <token>`，且 token 文件在连接建立后被删除。
2. `request("handshake")` 往返成功；请求行含正确 token。
3. fake helper 延迟 > 注入的 `request_timeout` → `action_timeout` + helper 收到 SIGTERM/重启（第二次 request 走新进程）。
4. fake helper 中途关闭连接 → 本请求错误 + 重启；下一请求成功。
5. 错误响应 `{"ok":false,"error":{"code":"permission_denied",...}}` 原样映射 `ComputerError`。
6. `shutdown` 后 socket 目录消失、子进程退出。

- [ ] **Step 2: 跑测试确认失败 → 实现 → 通过**

Run: `cargo test -p ade-computer`

- [ ] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-computer
git commit -m "feat(computer): helper transport（spawn/token/NDJSON/重试/超时/重启/terminate）"
```

---

### Task 4: client + 动作校验 + verification 归一化 + provider trait

**Files:**
- Create: `src-tauri/crates/ade-computer/src/validation.rs`
- Create: `src-tauri/crates/ade-computer/src/verification.rs`
- Create: `src-tauri/crates/ade-computer/src/client.rs`
- Create: `src-tauri/crates/ade-computer/src/provider.rs`
- Modify: `src-tauri/crates/ade-computer/src/lib.rs`
- Test: 同 crate + `tests/client.rs`（复用 `tests/common`）

**Interfaces:**
- Produces：
  - `validation::validate_computer_action(method: &str, params: &serde_json::Value) -> Result<(), ComputerError>`：仅对动作方法（`getAppState`/`click`/`performSecondaryAction`/`scroll`/`drag`/`typeText`/`pressKey`/`hotkey`/`pasteText`/`setValue`）执行；app 必填、`windowId XOR windowIndex`、click `elementIndex XOR (x,y)`/`clickCount` 正整数/`mouseButton∈{left,right,middle}`/modifiers 键规格、scroll `direction∈{up,down,left,right}`/`pages` 正数、drag 两端 XOR、pressKey 单键（`+` 字面量除外）、hotkey ≥1 修饰 + 恰 1 键、paste/typeText 文本存在、paste 16 MiB UTF-8；错误码 `invalid_argument`，**文案逐字取自 `orca:src/main/computer/computer-provider-action-validation.ts` 与 `orca:src/shared/computer-use-key-spec.ts`**（键规格函数移植为 Rust：`hotkey_validation_message/press_key_validation_message/click_modifiers_validation_message`）。
  - `verification::normalize_action_result(result: &mut serde_json::Value)`：`action.verification` 缺失时按 `action.path` 补 `{state:'unverified',reason}`（`synthetic→synthetic_input`、`clipboard→clipboard_paste`、`accessibility→accessibility_action_unasserted`；参照 `orca:src/main/computer/computer-action-verification-normalization.ts`）。
  - `client::ProviderClient`：`new(helper_app: PathBuf, request_timeout) `；`ensure_ready(&mut self) -> Result<Value, ComputerError>`（首次 `handshake`，`protocolVersion!=1` → `provider_incompatible`；缓存 capabilities）；`call(&mut self, method, params) -> Result<Value, ComputerError>`（单飞互斥：内部 `Mutex`，请求串行；动作方法先 `validate`、结果过 `normalize_action_result`）；`shutdown(&mut self)`。
  - `provider::ComputerProvider: Send` trait：`fn capabilities(&mut self) -> Result<serde_json::Value, ComputerError>; fn request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, ComputerError>;`；`impl ComputerProvider for ProviderClient`（`capabilities` → `ensure_ready` 的 handshake 结果；`request` → `call`）；**`provider::UnavailableProvider`**（helper 缺失时装配：两方法恒返回 `unsupported_capability`，message 取 `availability::provider_unavailable_message`）。

- [ ] **Step 1: 写失败测试**

锚：
1. 校验矩阵：每个方法的合法/非法组合（缺 app、window XOR、click 坐标 XOR、非法 mouseButton/direction、hotkey 无修饰、pressKey 带 `+`（除 `+` 本身）、paste 超限）→ 错误码与文案（从 oracle 文件拷贝的字符串断言）。
2. `normalize_action_result`：三种 path → 三种 reason；已有 verification 不覆盖。
3. `client`：fake helper 首握手 `protocolVersion:2` → `provider_incompatible`；正常握手后 `getAppState` 结果补 verification；两次并发 `call` 串行（fake helper 记录请求间隔/顺序）。
4. capabilities 缓存：第二次 `capabilities` 不再发 handshake。

- [ ] **Step 2: 跑测试确认失败 → 实现 → 通过**

Run: `cargo test -p ade-computer`

- [ ] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-computer
git commit -m "feat(computer): provider client、动作校验与 verification 归一化"
```

---

### Task 5: 权限模块（探针/openSetup/reset）

**Files:**
- Create: `src-tauri/crates/ade-computer/src/permissions.rs`
- Modify: `src-tauri/crates/ade-computer/src/lib.rs`
- Create: `src-tauri/crates/ade-computer/tests/permissions.rs`

**Interfaces:**
- Produces（serde camelCase，形状对齐 `src/shared/computer-use-permissions-types.ts`）：
  - `permissions::PermissionId { Accessibility, Screenshots }`（serde `accessibility|screenshots`）
  - `permissions::PermissionState { Granted, NotGranted, Unsupported }`（serde `granted|not-granted|unsupported`）
  - `permissions::PermissionStatus { platform, helper_app_path: Option<String>, helper_unavailable_reason: Option<String>, permissions: Vec<PermissionStatusEntry{id,status}> }`
  - `permissions::OpenSetupResult { opened_settings: bool, launched_helper: bool, permission_id: Option<PermissionId>, next_step: Option<String>, permissions: Option<Vec<Entry>> }`
  - `permissions::ResetResult { bundle_id: String }`
  - `permissions::CommandRunner` trait（`fn run(&self, program: &str, args: &[String]) -> io::Result<CommandOutput{status: i32, stdout: String, stderr: String}>`）+ `SystemRunner`。
  - `permissions::probe_status(runner: &dyn CommandRunner, helper: Option<&Path>) -> PermissionStatus`：写临时 status file → `open -n <helper.app> --args --permission-status-file <tmp>` → 轮询 50×100ms 读 `{accessibility, screenshots}`（值 `granted|not-granted` → 映射；读不到 → 各 `not-granted`）；helper 缺失 → `helperUnavailableReason`（文案取 `orca:src/main/computer/computer-provider-unavailable-message.ts` 语义，替换 Orcinus）。
  - `permissions::open_setup(runner, helper, id: Option<PermissionId>) -> OpenSetupResult`：先 `probe_status` 刷新 → `pkill -f 'orcinus-computer-use-macos --permission(s)?'` → `open -n <helper.app> --args --permission <id>`/`--permissions` → `nextStep` 文案（对齐 `orca:src/main/computer/macos-computer-use-permissions.ts:17-80` 的 nextStep 规则）。
  - `permissions::reset(runner, helper) -> ResetResult`：bundle ID = `helper_bundle_id(helper)`（Task 2）回退 `dev.itsuka.orcinus.computer-use` → `tccutil reset Accessibility <id>` + `tccutil reset ScreenCapture <id>`。
- 全过程无 helper 时：`probe_status` 返回 unavailable 形状；`open_setup` 返回 `opened_settings:false,next_step:Some(构建/安装提示)`；`reset` 照常执行（bundle ID 回退）。

- [ ] **Step 1: 写失败测试（fake runner 捕获 argv + 状态文件 fixture）**

锚：`open`/`pkill`/`tccutil` argv 逐字断言；状态文件 `{accessibility:"granted",screenshots:"not-granted"}` → 映射；status 文件始终缺失 → 两权限 `not-granted`；helper 缺失分支；bundleId 从 Info.plist fixture 读取 + 回退。

- [ ] **Step 2: 跑测试确认失败 → 实现 → 通过**

Run: `cargo test -p ade-computer permissions`

- [ ] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-computer
git commit -m "feat(computer): TCC 权限探针、授予与重置"
```

---

### Task 6: runtime RPC server

**Files:**
- Create: `src-tauri/crates/ade-computer/src/rpc.rs`
- Modify: `src-tauri/crates/ade-computer/src/lib.rs`
- Create: `src-tauri/crates/ade-computer/tests/rpc_server.rs`

**Interfaces:**
- Produces：
  - `rpc::FeatureRecorder: Send + Sync` trait：`fn record(&self, id: &str);`
  - `rpc::feature_id_for_method(method: &str) -> Option<&'static str>`：`computer.permissions` → `Some("computer-use-setup")`；其余 `computer.*`（除 `computer.capabilities`/`computer.permissionsStatus`）→ `Some("computer-use")`；其他 → `None`。（字面量必须为 `"computer-use"`——T10 ratchet 扫描依赖。）
  - `rpc::RpcServerConfig { runtime_id: String, user_data_dir: PathBuf, auth_token: String, app_version: String, max_message_bytes: usize, idle_timeout: Duration, max_connections: usize, keepalive_interval: Duration }` + `limits()` 默认（1MB/30s/32/10s；测试可缩）。
  - `rpc::RpcServer::start(config, provider: Arc<Mutex<dyn ComputerProvider>>, permissions: Arc<dyn PermissionsApi>, recorder: Arc<dyn FeatureRecorder>) -> io::Result<RpcServerHandle>`：写 `runtime.json`（0600，`protocol::RuntimeMetadata`，`transports:[{kind:"unix",endpoint}]`）→ 绑 `runtime.sock`（0600）→ 非阻塞 accept 轮询线程。
  - `rpc::RpcServerHandle { runtime_id: String, shutdown(self) }`：停 accept、join、删 socket 与 metadata。
  - `rpc::PermissionsApi` trait：`fn status(&self) -> PermissionStatus; fn open_setup(&self, id: Option<PermissionId>) -> OpenSetupResult;`
  - 协议：请求 `{"id":<string>,"authToken":<hex>,"method":<string>,"params":{…}}` 单行 NDJSON；响应 `{"id":…,"ok":true,"result":…,"_meta":{"runtimeId":…}}` / `{"id":…,"ok":false,"error":{"code","message","data"?}}`；`status.get` 结果 `{"app":{"running":true,"pid":…,"desktopWindowStatus":"available"},"runtime":{"state":"ready","reachable":true,"runtimeId":…,"appVersion":…},"graph":{"state":"not_ready"}}`（pid 为进程自身）。
  - 准入：JSON 解析失败/缺字段 → `bad_request`；`authToken` 不符 → `unauthorized`；行超 `max_message_bytes` → `request_too_large`；连接数超限 → `runtime_busy`；30s 无数据 → 断开；处理耗时 > `keepalive_interval` 时每 10s 向连接写 `{"_keepalive":true}\n`。
  - `computer.capabilities` → `provider.capabilities()`（helper 不可用错误照抛）；其余 `computer.*` → `provider.request(method, params)`；方法成功/失败后按 `feature_id_for_method` 调 `recorder.record`（**仅 `computer.*` 方法记录**；`status.get` 不记）。

- [ ] **Step 1: 写失败测试（fake provider + fake recorder + 临时目录）**

锚：
1. metadata 文件写出（0600 + 字段）并在 `shutdown` 后消失。
2. 无 token/错 token → `unauthorized`；坏 JSON → `bad_request`；超长行（配置缩到 1KB 测试）→ `request_too_large`。
3. `status.get` 形状；`computer.getAppState` 直通 fake provider；`computer.permissionsStatus` → PermissionsApi.status。
4. feature 记录：`computer.click` → `computer-use`；`computer.permissions` → `computer-use-setup`；`computer.capabilities`/`status.get` → 无记录。
5. 慢 provider（注入 50ms keepalive + 200ms 延迟）→ 客户端先收到 `{"_keepalive":true}` 再收结果。
6. 并发超限（max_connections=2）→ 第 3 连接 `runtime_busy`。

- [ ] **Step 2: 跑测试确认失败 → 实现 → 通过**

Run: `cargo test -p ade-computer`

- [ ] **Step 3: 提交**

```bash
git add src-tauri/crates/ade-computer
git commit -m "feat(computer): runtime RPC server（metadata/socket/信封/分派/feature 记录）"
```

---

### Task 7: App 装配 + `computerUsePermissions` Tauri 命令 + TS 域

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/state.rs`（`AppState` 加 `computer: Arc<ComputerService>`）
- Create: `src-tauri/crates/ade-bridge/src/computer_service.rs`（provider/permissions/rpc 组装 + `StoreFeatureRecorder`）
- Create: `src-tauri/crates/ade-bridge/src/commands/computer_use.rs`（3 命令 + specta DTO）
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`、`specta_export.rs`（两处清单）、`lib.rs`
- Modify: `src-tauri/src/lib.rs`（Exit 时 `computer.shutdown()`）
- Create: `src/bridge/real/computer-use-permissions.ts`
- Modify: `src/bridge/create-api.ts`（`RealDomains` 加 `'computerUsePermissions'`）
- Modify: `src/bridge/real/parity.test.ts`、`create-api.test.ts`（如需）

**Interfaces:**
- Consumes：`ade-computer::{paths,client,provider,permissions,rpc}`（T2–T6）；`ade-store` UI store `record_feature_interaction_in_memory`。
- Produces：
  - `ComputerService { client: Mutex<ProviderClient>, permissions_impl, rpc: Mutex<Option<RpcServerHandle>>, user_data_dir, helper_app: Option<PathBuf> }`；`initialize(app_data_dir, resource_dir) -> io::Result<ComputerService>`（helper 缺失也返回——权限面仍可用）；`shutdown()`。
  - `StoreFeatureRecorder`（impl `rpc::FeatureRecorder` → `UiStateStore`）。
  - Tauri 命令（camelCase args，DTO 对齐 `src/shared/computer-use-permissions-types.ts`）：
    - `computer_use_permissions_get_status() -> ComputerUsePermissionStatusDto`
    - `computer_use_permissions_open_setup({id?}) -> ComputerUseOpenSetupResultDto`
    - `computer_use_permissions_reset() -> ComputerUseResetResultDto`
  - TS：`createComputerUsePermissionsRealApi()` 3 方法 `invokeCommand`；4 域 parity。
- 说明：renderer 的 `openSetup` 已自行 `recordFeatureInteraction('computer-use-setup')`，Tauri 命令路径**不重复记录**；RPC（CLI）路径记录在 T6 server。

- [ ] **Step 1: Rust 装配（AppState + 命令）**

`state.rs initialize`：`app_data_dir` = Tauri `app.path().app_data_dir()`（`dev.itsuka.orcinus`；`ORCINUS_USER_DATA_PATH` 覆盖，测试友好）；`auth_token` = 24 字节随机 hex；`runtime_id` = UUID；`RpcServer::start(...)`；`helper_app` = `paths::helper_app_path(env::var("ORCINUS_COMPUTER_MACOS_HELPER_APP_PATH").ok().as_deref(), resource_dir)`。`Exit` 调 `shutdown()`（RPC 停 + provider terminate + metadata 删除）。

- [ ] **Step 2: 命令 + bindings**

按既有命令惯例（`#[tauri::command]` + `*_impl` 可测函数 + DTO specta derive）；`cargo run -p ade-bridge --bin export-bindings` 重生成；`cargo test -p ade-bridge`（`bindings_are_fresh`）。

- [ ] **Step 3: TS 域 + 测试**

`real/computer-use-permissions.ts`（模式照 `real/preflight.ts`）；parity `surfaceCases` 加 `computerUsePermissions` 显式 3 / missing 0（该域离线全 fallback）；`pnpm vitest run src/bridge`。

- [ ] **Step 4: 提交**

```bash
git add src-tauri src/bridge
git commit -m "feat(bridge): computerUsePermissions 三命令与 AppState 装配"
```

---

### Task 8: `cli` 域接真（PATH 安装/状态/移除）

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/cli_install.rs`（6 命令 + specta DTO）
- Modify: `commands/mod.rs`、`specta_export.rs`、`lib.rs`（bridge）
- Create: `src/bridge/real/cli.ts`
- Modify: `src/bridge/create-api.ts`（`RealDomains` 加 `'cli'`）
- Modify: `parity.test.ts` 等

**Interfaces:**
- Consumes：`protocol`（无）；当前可执行文件路径（`std::env::current_exe()`）。
- Produces：
  - DTO `CliInstallStatusDto`：字段逐字对齐 `src/shared/cli-install-types.ts`（`platform,commandName,commandPath,pathDirectory,pathConfigured,launcherPath,installMethod,supported,state,currentTarget,unsupportedReason,detail`）。
  - `resolve_cli_launcher() -> PathBuf`：`current_exe().parent()/orcinus`。
  - `install_target_path(system_dir: &Path /* /usr/local/bin */, home: &Path, env_override: Option<&Path>) -> PathBuf`：override → 系统目录存在且**可写**（`access` W_OK）→ `/usr/local/bin/orcinus` → 否则 `~/.local/bin/orcinus`。
  - `cli_get_install_status/install/remove`（`*_impl` 可测，注入 launcher/target/home/PATH）；`cli_*_wsl` 3 命令恒 `state:"unsupported", unsupportedReason:"platform_not_supported", supported:false`。
  - 状态判定：不存在 → `not_installed`；symlink 解析 == canonical launcher → `installed`；指向托管目录但目标不同 → `stale`；其他占位 → `conflict`（install 拒绝 + `detail`）。`pathConfigured` = `$PATH` 含目标目录（含即 true，缺 false，恒可读 → null 不出现）。`installMethod:"symlink"`。
  - TS `createCliRealApi()` 6 方法；parity 显式 6 / missing 0。
- 不做：管理员提权（osascript）、shell profile 改写、Windows 分支（非 macOS → `unsupported`/`platform_not_supported`）。

- [ ] **Step 1: Rust 测试先行（tempdir 全流程）**

锚：not_installed → install（symlink 存在且指向 launcher）→ installed；他物占位 → conflict 且 install 拒绝；stale（托管目录旧目标）识别；remove 后回 not_installed；`/usr/local/bin` 不可写 → 落 `~/.local/bin`（临时目录模拟可写性）；PATH 探针。

- [ ] **Step 2: 命令 + bindings + TS + 测试 + 提交**

`cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`；`pnpm vitest run src/bridge`。

```bash
git add src-tauri src/bridge
git commit -m "feat(bridge): cli 域接真（orcinus PATH 安装/状态/移除）"
```

---

### Task 9: `skills.discover` 本地最小真实现

**Files:**
- Create: `src-tauri/crates/ade-bridge/src/commands/skills.rs`
- Modify: `commands/mod.rs`、`specta_export.rs`、`lib.rs`（bridge）
- Create: `src/bridge/real/skills.ts`
- Modify: `src/bridge/create-api.ts`（`RealDomains` 加 `'skills'`）
- Modify: `parity.test.ts`

**Interfaces:**
- Produces：
  - `skills::home_skill_roots(home: &Path, env_overrides…) -> Vec<SkillRoot{id,label,path,source_kind,providers,owner}>`：移植 `orca:src/main/skills/skill-discovery-sources.ts:104-227` 的 **home 表**（codex/claude/agents/grok/opencode/pi/omp/hermes/prime-agent/gemini/antigravity/cursor/droid/continue/trae/aug + `codex-plugin-cache`；providerRootOverrides 仅支持 `CLAUDE_SKILLS_ROOT` 等环境变量若有——按参照 `resolveEnvironmentSkillProviderRoots` 摘要实现或省略并在 detail 记录）。
  - `skills::discover(home) -> DiscoveredSkills`：各根下 `<dir>/SKILL.md`（跳过 `.` 开头目录）；name = frontmatter `name:`（前 20 行，缺失回退目录名）；`sourceKind`：`.system` 第一段 → `bundled`；`skillFilePath`/`directoryPath` 绝对路径；`sources` 含 `exists`；排序 name（大小写不敏感）。
  - Tauri 命令 `skills_discover({refresh?}) -> {skills: Json, sources: Json, scannedAt: String}`（10s 内存缓存，refresh 绕过）。
  - TS `createSkillsRealApi()` 仅 `discover` 显式，其余维持 fallback；parity 1/missing-N。
- 已知边界（照 spec）：无 repo 根、无 WSL/远端、无 freshness/install/remove。

- [ ] **Step 1: Rust 测试先行（fixture home 树）**

锚：claude/codex/agents 根各放一个 skill（frontmatter name 命中/缺失回退）；`.system` → bundled；不存在的根 `exists:false` 且不出 skills；排序；TTL 缓存（同 home 二次不重扫——用扫描计数注入或 mtime 变更断言）与 refresh 绕过。

- [ ] **Step 2: 命令 + bindings + TS + 测试 + 提交**

```bash
git add src-tauri src/bridge
git commit -m "feat(bridge): skills.discover 本地最小真实现（home 技能根扫描）"
```

---

### Task 10: PTY env 注入 + feature-interaction 恢复

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/pty.rs`（`resolve_orcinus_cli_path` + `build_spawn_env`）
- Modify: `src/shared/feature-interaction-catalog.ts`（恢复 `computer-use` id）
- Modify: `src/shared/feature-interactions.test.ts`（`expectedIds` + ratchet 扫描面）
- Modify: `src-tauri/crates/ade-computer/src/rpc.rs`（如 `feature_id_for_method` 已含字面量，无需改；本轮只补测试）

**Interfaces:**
- Produces：
  - `resolve_orcinus_cli_path(current_exe: &Path) -> Option<PathBuf>`：`current_exe().parent()/orcinus` 且 `is_file()`。
  - `build_spawn_env` 在无既有 `ORCINUS_CLI_COMMAND` 时注入该值（存在才注入；不覆盖调用方 env）。
  - catalog：union/`FEATURE_INTERACTIONS`/`expectedIds` 加 `computer-use`（定义文案照 oracle `orca:src/shared/feature-interaction-catalog.ts` 对应条目摘要）；ratchet 生产调用方扫描面扩展为同时接受 `src-tauri/crates/ade-computer/src` 中含字面量 `"computer-use"` 的映射（`feature_id_for_method`）。

- [ ] **Step 1: 测试先行**

Rust：`resolve_orcinus_cli_path`（临时目录造 `orcinus` 文件 → Some；缺 → None）；`build_spawn_env` 注入/不覆盖断言（扩展现有 `pty.rs:1264-1320` 测试样式）。
TS：`pnpm test src/shared/feature-interactions.test.ts`（RED：缺 id → GREEN）。

- [ ] **Step 2: 实现 + 提交**

```bash
git add src-tauri src/shared
git commit -m "feat: 终端注入 ORCINUS_CLI_COMMAND 与 computer-use 交互 id 恢复"
```

---

### Task 11: `orcinus-cli` 骨架（status/open/skills get）+ 指南与 skill stub

**Files:**
- Create: `src-tauri/crates/orcinus-cli/Cargo.toml`
- Create: `src-tauri/crates/orcinus-cli/src/{main,errors,client,status,open,skills}.rs`
- Create: `src-tauri/crates/orcinus-cli/tests/common/mod.rs`（fake runtime server：读 `ade-computer::protocol` 形状）
- Create: `src-tauri/crates/orcinus-cli/tests/cli_basics.rs`
- Create: `skill-guides/computer-use.md`
- Create: `skills/computer-use/SKILL.md`
- Modify: `src/shared/agent-feature-install-commands.ts`（URL → `https://github.com/ItsukaKotori/orcinus`）
- Modify: `src/shared/agent-feature-install-commands.test.ts`（断言更新）

**Interfaces:**
- Produces：
  - `Cargo.toml` deps：`ade-computer = { path = "../ade-computer" }`、`clap = { version = "4", features = ["derive"] }`、`serde/serde_json`、`uuid`、`libc`。
  - `main.rs`：clap 子命令 `status [--json]`、`open [--json]`、`skills get <topic> [--full] [--json]`、`--version/-V`、`--help`；错误 → stderr/JSON 信封 + exit 1。
  - `errors::CliError { code: String, message: String, data: Option<Value> }`；成功输出 `{"id":<uuid>,"ok":true,"result":…,"_meta":{"runtimeId":…}}`；失败 `{"id":"local","ok":false,"error":{…}}`（`--json` 时；人读模式打印 message + nextSteps）。
  - `client::RuntimeClient`：metadata 路径 = `ORCINUS_USER_DATA_PATH` 或 `~/Library/Application Support/dev.itsuka.orcinus/runtime.json`；`connect()` 读 `protocol::RuntimeMetadata`（缺/不完整 → `runtime_unavailable`）；`call(method, params, timeout)` NDJSON 单行 + 读行（`_keepalive` 刷新 deadline）；响应 `_meta.runtimeId` 不符 → `invalid_runtime_response`。
  - `status.rs`：metadata 缺失 → 合成 `{app:{running:false},runtime:{state:"not_running"|"stale_bootstrap",reachable:false}}`；否则 `status.get`（1s 超时）；`--json` 恒 exit 0，人读 `!reachable` exit 1。
  - `open.rs`：`ORCINUS_OPEN_COMMAND`（`sh -c`）> `ORCINUS_APP_EXECUTABLE` > `open -a Orcinus`；失败 → `runtime_open_failed`；轮询 250ms×15s 至可达，超时 `runtime_open_timeout`。
  - `skills.rs`：`include_str!("../../../../skill-guides/computer-use.md")`（编译期嵌入；测试断言非空）；`--json` → `{name:"computer-use",full:true,markdown}`；未知 topic → `invalid_argument`（列可用 topic）。
  - 指南内容：移植 `orca:skill-guides/computer-use.md` 并适配（`ORCA`→`orcinus`；删除嵌入浏览器/orchestration/远程措辞；`--worktree/--session` 注明仅显式透传）。
  - stub：照 `orca:skills/computer-use/SKILL.md` 骨架，解析顺序 `ORCINUS_CLI_COMMAND` → PATH `orcinus`，**禁止回退 `orca`**；引导 `orcinus skills get computer-use`。
- `.gitignore` 无需改（无产物）。

- [ ] **Step 1: 测试先行（fake runtime server）**

锚：`status --json`（无 metadata → not_running/exit 0；fake server/metadata → reachable true）；错 token → `runtime_unavailable`（连接后 unauthorized 映射）；`open`（`ORCINUS_OPEN_COMMAND=/bin/true` + 无服务 → `runtime_open_timeout`；预可达 → 成功）；`skills get computer-use --json` 形状 + `--full` 等价 + 未知 topic；人读模式 exit code。
（用 `env!("CARGO_BIN_EXE_orcinus")` + `std::process::Command`。）

- [ ] **Step 2: 跑 RED → 实现 → GREEN + TS 测试**

Run: `cargo test -p orcinus-cli`；`pnpm vitest run src/shared/agent-feature-install-commands.test.ts`。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/crates/orcinus-cli skill-guides skills src/shared
git commit -m "feat(cli): orcinus CLI 骨架（status/open/skills get）与自有 computer-use skill"
```

---

### Task 12: `orcinus-cli` computer 子命令

**Files:**
- Create: `src-tauri/crates/orcinus-cli/src/computer.rs`
- Modify: `src-tauri/crates/orcinus-cli/src/main.rs`（挂 `computer` 子命令）
- Create: `src-tauri/crates/orcinus-cli/tests/cli_computer.rs`

**Interfaces:**
- Produces：
  - clap：`computer capabilities|list-apps|permissions|list-windows|get-app-state|click|perform-secondary-action|scroll|drag|type-text|press-key|hotkey|paste-text|set-value`；公共 flag `--app`（必需，除 capabilities/list-apps/permissions）、`--session`/`--worktree`（互斥）、`--window-id`/`--window-index`（互斥）、`--restore-window`、`--no-screenshot`；动作 flag 面与 oracle 一致（见 spec §3.6）。
  - flag 级校验文案**逐字取自 `orca:src/cli/computer-action-flag-validation.ts` 与 `orca:src/cli/handlers/computer.ts`**（click 三态 XOR、mouseButton、direction、drag 四态、text-stdin TTY 拒绝、press-key/hotkey 规格）。
  - 参数组装 → RPC 调用（方法名/字段名照 `orca:src/main/runtime/rpc/methods/computer.ts`；客户端校验后再走 `ade-computer::validation` 双保险）。
  - `--json` 截图落盘：`result.screenshot.data`（base64）→ `$ORCINUS_COMPUTER_SCREENSHOT_TMPDIR` 或 `$TMPDIR/orcinus-computer-use/<id>-screenshot.png`（0600；`<id>` = `snapshot.id`，缺则请求 id）；JSON 移除 `data` + 增 `path`/`dataOmitted:true`/`expiresAt`（now+24h RFC3339）；目录内 >24h 文件尽力清扫；写失败保留内联（`screenshotStatus.state` 不变）。
  - `computer permissions [--id …]` → RPC `computer.permissions` 直通。
  - 人读模式（非 `--json`）：错误打印 `error <code>: <message>`、exit 1；成功打印摘要行（`get-app-state`：`app/窗口/元素数` + treeText；动作：`action=<path> verification=<state>`；list/capabilities/permissions/status 打印关键字段）。**不做 nextSteps 映射表**（记录备案为边界）。

- [ ] **Step 1: 测试先行（fake runtime server 回放各方法）**

锚：14 子命令各一条合法调用 → 请求方法名/params 断言 + 响应原样输出（信封含 `_meta.runtimeId`）；flag XOR 矩阵（click/drag/scroll/type-text/set-value/press-key/hotkey）错误文案 + exit 1；截图落盘（0600、字段替换、目录清扫）；RPC 错误透传（`element_not_found` 等）+ exit 1。

- [ ] **Step 2: 跑 RED → 实现 → GREEN → 提交**

Run: `cargo test -p orcinus-cli`

```bash
git add src-tauri/crates/orcinus-cli
git commit -m "feat(cli): orcinus computer 14 子命令与截图落盘"
```

---

### Task 13: 打包接线（externalBin + 构建脚本 + bundle 验证）

**Files:**
- Create: `config/scripts/build-orcinus-cli.mjs`
- Modify: `package.json`（`"build:orcinus-cli"`）
- Modify: `src-tauri/tauri.conf.json`（`bundle.externalBin: ["binaries/orcinus"]`）

**Interfaces:**
- Produces：`build-orcinus-cli.mjs`：`cargo build -p orcinus-cli --release`（workdir `src-tauri`）→ 拷贝 `target/release/orcinus` → `src-tauri/binaries/orcinus-aarch64-apple-darwin`。
- 验证顺序（手动，记录耗时）：`pnpm build:computer-macos && pnpm build:orcinus-cli && APPLE_SIGNING_IDENTITY=… pnpm tauri build`。

- [ ] **Step 1: 脚本 + 配置**

- [ ] **Step 2: 验证（本任务专项，耗时长可只跑一次）**

```bash
pnpm build:computer-macos && pnpm build:orcinus-cli
APPLE_SIGNING_IDENTITY="Apple Development: 942697184@qq.com (5BKC9569R9)" pnpm tauri build
ls "src-tauri/target/release/bundle/macos/Orcinus.app/Contents/Resources/computer-use/Orcinus Computer Use.app"
ls "src-tauri/target/release/bundle/macos/Orcinus.app/Contents/MacOS/orcinus"
codesign -dv "…/Orcinus Computer Use.app" 2>&1 | grep Identifier
```

预期：全部存在且 helper 标识正确。若 `tauri build` 因环境失败（签名/网络），记录错误并降级验证 `tauri.conf.json` 解析（`pnpm tauri info` 或构建日志），在 commit message 说明。

- [ ] **Step 3: 提交**

```bash
git add config/scripts package.json src-tauri/tauri.conf.json
git commit -m "build: orcinus CLI externalBin 与 computer-use 打包接线"
```

---

### Task 14: 门禁复跑 + 收尾记录

**Files:**
- Create: `docs/phase2g-computer-use-record.md`
- Modify: `docs/superpowers/plans/2026-10-11-computer-use.md`（全部 checkbox 勾选）

**Interfaces:**
- Consumes：spec §6/§7（手工验收清单与偏差备案直接进入记录）。
- Produces：记录文档，结构镜像 `docs/phase2f-agent-session-history-record.md`（§1 范围与验收/§2 提交清单 `git log --oneline --reverse 3cd628ac..HEAD`/§3 三门禁证据 + T1/T13 专项证据（swift test、tauri build、codesign）/§4 手工验收（待用户复核，spec §6.3 八项）/§5 偏差与边界备案（spec §7 十项 + 实施中新发现）/§6 已知边界与后续）。

- [ ] **Step 1: 三门禁 + bindings**

```bash
# workdir src-tauri
cargo test --workspace
cargo run -p ade-bridge --bin export-bindings && git diff --exit-code src/bridge/real/generated/tauri-bindings.ts
# repo root
rm -f tsconfig.tsbuildinfo && pnpm typecheck && pnpm build:web
pnpm test
```

预期：全绿（`browser-history-match.performance.test.ts` 性能 flake → 隔离复跑并记录）。

- [ ] **Step 2: 记录 + 勾选 + 提交**

```bash
git add docs
git commit -m "docs: Phase 2G computer-use 实施记录与门禁证据"
```

---

## 附：任务依赖关系

```
T1 ──> T13 ──┐
T2 ──> T3 ──> T4 ──> T6 ──> T7 ──> T14
        └──> T5 ──┘        T8 ──> T14
T4 ──> T11 ──> T12 ──> T13   T9 ──> T14
T6 ──> T10 ──> T14           T10 ─> T14
```

（T1–T6 串行内核；T7–T10 可交错但均依赖 T6；T11 依赖 T2/T4 协议；T12 依赖 T11；T13 依赖 T1/T12；T14 最后。）
