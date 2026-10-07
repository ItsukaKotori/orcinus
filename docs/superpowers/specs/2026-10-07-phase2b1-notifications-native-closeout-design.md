# Phase 2 子项目 B.1：通知收尾（原生权限探测 + 精确 dismiss + 内置音效）设计规格

- 日期：2026-10-07
- 状态：brainstorming 输出（范围 = 三块一起、通道 = ade-bridge objc2 原生三命令、平台分流 + 失败回退——用户逐节批准；本规格为实现依据）
- 基线：`main@d99361d`（Phase 2B 已合入并手工验收通过）
- 行为 oracle：`/Users/itsuka/CodeSpace/orca`（Electron 版，只读参照；`orca:` 前缀均为其内路径）
- 上游：`docs/phase2b-agent-hooks-notifications-record.md` §6「2B.1 跟进」；2B 规格 §3.8 通知域

## 1. 背景与目标

2B 的通知域有三个已知缺口：

1. **权限面是假的**：`tauri-plugin-notification` 2.5.1 desktop 恒报 Granted（`desktop.rs:85-99`/`commands.rs:10-20`），`getPermissionStatus.requested` 实际表示"已授权"而非 fork 语义"是否触发过弹窗"，`probeDelivery` 恒 `{delivered, authoritative:false}`；macOS 静默吞通知时 `blocked-by-system` 回退不可达。
2. **桌面精确 dismiss 不可达**：插件桌面用 `NSUserNotificationCenter`（notify-rust 默认后端），投递不带标识符、无删除 API、`remove_active` 未在桌面注册；FNV 哈希映射无从匹配。
3. **内置音效静音**：9 个内置 id 可选但 `playSound` 只支持 custom path。

2B.1 目标：macOS 上接真权限读口与投递/dismiss（`UNUserNotificationCenter`），并让 9 个内置音效可播；Win/Linux 行为不变。

**验收（自动 + 手工）**：系统设置改通知权限 → 卡片状态实时正确（authoritative 读口）；首次未决时探测触发系统弹窗；拒绝时 dispatch 走 `blocked-by-system` 回退 toast；内置/自定义音效可听且无双声；通知中心 dismiss 真删除横幅。自动化门禁：`cargo test --workspace` 全绿 + `pnpm test` 全绿 + `pnpm typecheck && pnpm build:web` exit 0。

## 2. 范围

### 2.1 实现面

| 面 | 载体 | 说明 |
|---|---|---|
| native 命令（4） | `ade-bridge` `commands/notifications_native.rs` | `notifications_get_authorization_status` / `notifications_request_authorization` / `notifications_deliver_native` / `notifications_dismiss_native`；macOS 实现走 objc2，非 macOS 返回 `unsupported` 桩，命令**始终注册**（bindings 平台稳定） |
| UN 通道 | 同上 | `run_on_main_thread` 发起 + mpsc oneshot 收 completion（2s 超时）；投递带稳定字符串标识符；dismiss 先 `getDeliveredNotifications` 求交集再删，返回真实命中数 |
| bundle identity | `src-tauri/macos-info.plist` | 补 `CFBundleIdentifier = dev.itsuka.orcinus`（dev 裸二进制 UN 可用性前提，spike 验证） |
| TS 分流 + 回退 | `src/bridge/real/notifications.ts` | macOS 首次 status 成功 → `nativeAvailable`；任何 native 失败 → 回退插件路径并如实报 `authoritative:false` |
| 权限语义 | 同上 + `ui_get`/`ui_set` | `requested` = 持久化 `notificationPermissionRequested`（读到/写入）；probe 触发弹窗一次/session；dispatch 在 not-determined 时触发 |
| 内置音效 | `resources/notification-sounds/*.mp3`（9 个，拷自 oracle）+ `src/renderer/src/lib/built-in-notification-sounds.ts` + `playSound` | Vite `?url` 资源导入；custom 走原 base64 通道，内置走资源 URL，`system` 仍 `missing-path` |

### 2.2 明确不做（防蔓延）

Win/Linux 投递切换（仍 tauri-plugin-notification）；通知点击聚焦（UN delegate）与 action 按钮；通知样式/分组配置；GitLab/其它 provider 的权限面；`Notification`/`PreCompact` hook 事件；系统音效枚举（`customSoundId==='system'` 仍用 UN 默认音）。

## 3. 架构

### 3.1 native 命令（macOS-only 实现，平台稳定注册）

命令与返回形状（camelCase，specta 三点登记）：

- `notifications_get_authorization_status()` → `{ status: 'authorized' | 'denied' | 'not-determined' | 'unknown', available: boolean }`；non-macOS → `{ status: 'unknown', available: false }`（不报错，便于 TS 探测）。
- `notifications_request_authorization()` → `{ status: <同上>, available: boolean }`；macOS 调 `requestAuthorizationWithOptions([Alert, Sound])`，completion 后重读 `getNotificationSettings` 返回终态。
- `notifications_deliver_native({ id, title, body, silent })` → `{ ok: true } | { ok: false, error: string }`；`silent === true` 不设 sound，否则 `UNNotificationSound.defaultSound`；`id` 即 `UNNotificationRequest` 标识符（`requestWithIdentifier:content:trigger:nil`）。
- `notifications_dismiss_native({ ids: string[] })` → `{ dismissed: number }`；`getDeliveredNotifications` 结果与 `ids` 求交、`removeDeliveredNotificationsWithIdentifiers`、返回交集数。

实现约束：objc2 调用统一经 `AppHandle::run_on_main_thread` 发起（`UNUserNotificationCenter` 主线程最稳），completion 回调用 `std::sync::mpsc` 传回，异步命令侧 `recv_timeout(2s)` 等待；超时/异常 → `{ok:false,error}` 或 `unknown`，绝不 panic。状态常量映射：`Authorized(2)/Provisional(3)/Ephemeral(4)→authorized`、`Denied(1)→denied`、`NotDetermined(0)→not-determined`（对齐 oracle `notification-authorization-status.ts:46-91`）。

依赖：`[target.'cfg(target_os = "macos")'.dependencies]` 增 `objc2 = "0.6"`、`objc2-foundation = "0.3"`（NSArray/NSString/NSObject 等 feature）、`objc2-user-notifications = "0.3"`（默认 feature）、`block2 = "0.6"`——版本均已在 `Cargo.lock`（wry 的 iOS 目标传递依赖），macOS 首次编译，无新解析。TS 无新增 npm 依赖。

### 3.2 权限语义（TS 侧）

- `getPermissionStatus()`：`{ supported: true, platform: 'darwin', requested }`；`requested` 经 `ui_get` 读 `notificationPermissionRequested`（缺失 false）。**只读不改**——卡片在 `probeDelivery` 前调用它拿 promptedBefore（`mac-notification-permission-card.tsx:80-99` 顺序契约）。
- `probeDelivery()`（darwin + nativeAvailable）：native 读口为准，`authoritative: true`——`authorized→{delivered}`、`denied→{blocked}`；`not-determined` 触发 `requestAuthorization`（每 session 一次），随后重读：授权→`{delivered}`、拒绝→`{blocked}`、仍未决→`{awaiting-decision}`；触发即 `ui_set({notificationPermissionRequested:true})`。
- `dispatch()`（darwin + nativeAvailable）：先读授权态——`authorized` → 原生投递；`denied` → `{delivered:false, reason:'blocked-by-system'}`；`not-determined` → 触发授权窗后重读并投递或 blocked-by-system；触发即写 `requested=true`。
- 非 darwin / native 不可用：行为与 2B 完全一致（`authoritative:false` 的插件探测；dispatch 恒 best-effort）。

### 3.3 投递与 dismiss

- **标识符**：`args.notificationId`（`buildAgentNotificationId` 稳定字符串）存在即用；缺失（如 terminal-bell）生成 `orcinus:<uuid>`。UN 标识符无 32 位限制，macOS 直接按原始字符串匹配——**不再走 FNV 哈希**；哈希保留给插件回退路径。
- **内容**：title/body 复用 `buildNotificationCopy`；声音规则对齐 fork——`customSoundId === 'system'` 由 UN 播默认音（`silent:false`），其它 id（custom/内置）`silent:true`，音效由 renderer `playSound` 播，杜绝双声。
- **返回**：投递成功 `{delivered:true}`；native 失败（含超时）→ `{delivered:false, reason:'not-displayed'}`；若失败同时读到 `denied` → `blocked-by-system`。
- **dismiss**（darwin + nativeAvailable）：原始字符串 ids 交给 native 命令，返回交集计数；插件回退路径维持 2B 现状（FNV 哈希 + `removeActive`，异常兜底 `{dismissed:0}`）。

### 3.4 内置音效

- 9 个 mp3（`two-tone/bong/thump/blip/sonar/blop/ding/clack/beep`，orca `resources/notification-sounds/`，各 12,717B）拷入 `ade/resources/notification-sounds/` 并 git 跟踪；来源为同作者 fork 的同名资源，属计划内移植。
- `src/renderer/src/lib/built-in-notification-sounds.ts`：`const BUILT_IN_SOUND_URLS: Record<id, string>`，每项 `import url from '<相对路径>/resources/notification-sounds/<id>.mp3?url'`——相对深度与现有 `AppIconSelector.tsx:3-5` 的资源导入一致，实现时按实际路径核对（Vite `?url` 会在构建产物中生成相对资源，`base: './'` 已保证 `tauri://` 下可解析）。
- `playSound` 解析顺序：`customSoundId==='custom'` 且有 path → 现有 `notifications_read_sound` base64 通道；9 个内置 id → 资源 URL 直接 `Audio` 播放（dedupe 键用 id，`force` 跳过在播去重）；`system`/无路径 → `{played:false, reason:'missing-path'}`。音量 `options.volume/100`（0–100 契约，2B 终审已修）不变。

### 3.5 平台分流与回退

- `real/notifications.ts` 模块级 `nativeAvailable: boolean | null`：**能力探测**只在 darwin 首次调用 `notifications_get_authorization_status` 时进行，`available:true` 才启用原生路径；命令抛错或 `available:false` → 置 `false` 并记一次 warn，此后 dispatch/probe/dismiss 全走插件路径。
- 单次业务失败（投递超时/被拒、dismiss 无交集）**不改变** `nativeAvailable`：本次如实返回失败，后续调用仍用原生（权限被拒属正常业务态，不是能力缺失）。
- 回滚策略：移除 TS 对 native 命令的调用即回到 2B 行为；Rust 命令留桩不影响。

## 4. 错误处理与边界

| 场景 | 行为 |
|---|---|
| dev 裸二进制 UN 不可用（无/坏 bundle identity） | status 返回 `available:false` → TS 回退插件路径，行为等同 2B；打包产物照常原生 |
| `requestAuthorization` 超时 | 返回最近一次读到的状态；仍 not-determined → `awaiting-decision`；绝不阻塞 UI |
| 投递时权限被系统关掉 | 读口报 `denied` → `blocked-by-system`（renderer 显示回退 toast，2B 已有） |
| dismiss 的 id 不在 delivered 列表 | 交集为 0 → `{dismissed:0}`（幂等） |
| 通知尚未展示（UN 异步） | deliver 返回 `ok:true` 即视为已投递；`requireDisplayConfirmation` 继续忽略（2B 同） |
| 系统/自定义音效双声 | 规则保证：任一路径只一个声源（`system`→UN 默认、其它→renderer） |
| 内置音效资源加载失败 | `Audio.onerror` → `{played:false, reason:'playback-failed'}` |

## 5. 测试与门禁

### 5.1 Rust

- 四个命令的 serde 形状与 non-macOS 桩（`status:'unknown'`/`ok:false`）单元测试；
- UN 状态常量→字符串映射测试（可用 `#[cfg(test)]` 纯函数暴露映射，避免真调用）；
- binding 三点登记（`collect_commands!`/命令清单测试/重新生成 bindings）+ 新鲜度测试；
- `cargo test --workspace` 全绿。

### 5.2 TS

- `real/notifications.test.ts`：native 可用路径（mock `notifications_*_native` invoke）——status→requested 语义、probe 三态与 authoritative、dispatch blocked-by-system/投递、dismiss 交集计数；native 失败回退（`available:false` → 插件路径、`authoritative:false`）；`notificationPermissionRequested` 读写（mock `ui_get`/`ui_set`）；
- 内置音效：id→URL 播放、`force`、dedupe、`system`→missing-path；
- 全仓 `pnpm test`、`pnpm typecheck && pnpm build:web`。

### 5.3 手工验收（规格 §1 链）

1. `pnpm dev`（spike）：card 首启读到真实状态；UN 不可用则记录并改在打包产物验收。
2. 系统设置 → 通知 改权限 → 卡片 2.5s 轮询内状态实时变化（authoritative）。
3. 首次未决 → probe 触发系统授权弹窗；拒绝后 dispatch → blocked-by-system 回退 toast。
4. 设置页依次试听 9 个内置 + custom：可听、无双重声、音量滑杆生效。
5. 触发一条 waiting 通知 → 系统通知中心 dismiss（或 `ui-slice-activity-actions` 路径）→ 横幅被移除、计数正确。

## 6. 风险与偏差备案

1. **dev 裸二进制 bundle identity**：`macos-info.plist` 补 `CFBundleIdentifier` 是主对策；若 spike 仍不可用，回退网保证功能不退化，验收移至 `tauri build` 产物（计划第一步 spike 定论）。
2. **投递通道迁移**：macOS 切换为 UN 后，插件此前投递的 `NSUserNotificationCenter` 横幅无法被 UN 删除（旧横幅属上一会话，实际窗口极小）；不双通道并存，一次性切换。
3. **UN 投递与插件权限语义分离**：插件 `isPermissionGranted` 恒 true 的假象仅在 native 不可用时兜底使用；启用 native 后不再以其为准。
4. **`requested` 语义变更**：从"已授权"改为"触发过弹窗"，与卡片 `promptedBefore` 契约一致；首次安装未触发时卡片显示 awaiting-permission 属预期。
5. **objc2 版本**：全部沿用 Cargo.lock 既有版本（0.6.4/0.3.2/0.3.2/0.6.2），不引入新解析；若 tauri 升级需复核 feature 兼容。
6. **音效资源移植**：9 个 mp3 为 oracle 同作者仓库资源，计划内移植；如需替换音色后续可换文件不动代码。
