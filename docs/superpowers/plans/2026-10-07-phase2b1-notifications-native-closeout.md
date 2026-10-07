# Phase 2 子项目 B.1：通知收尾（原生权限探测 + 精确 dismiss + 内置音效）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** macOS 上接真通知权限读口与 UN 投递/精确 dismiss，并让 9 个内置音效可播；Win/Linux 完全不变。

**Architecture:** `ade-bridge` 新增平台稳定注册的 4 个 native 命令（macOS 走 objc2 `UNUserNotificationCenter`，非 macOS 返回 `unsupported` 桩）；TS `real/notifications.ts` 运行时能力探测，macOS 走原生、失败自动回退 2B 插件路径；`requested` 改为持久化 UI 态 `notificationPermissionRequested`；内置音效资产拷入 `resources/notification-sounds/` 走 Vite `?url` 播放。

**Tech Stack:** Rust（objc2 0.6 / objc2-foundation 0.3 / objc2-user-notifications 0.3 / block2 0.6，均已在 Cargo.lock）、tauri-specta、TS（React + vitest + happy-dom）。

**Spec:** `docs/superpowers/specs/2026-10-07-phase2b1-notifications-native-closeout-design.md`

## Global Constraints

- 门禁：`cargo test --workspace` 全绿；`pnpm test` 全绿；`pnpm typecheck && pnpm build:web` exit 0
- Rust 快测：`cargo test -p ade-bridge`（在 `src-tauri/` 下执行）
- 新增 Rust 依赖**仅 macOS target**：`block2 = "0.6"`、`objc2 = "0.6"`、`objc2-foundation = "0.3"`、`objc2-user-notifications = "0.3"`（版本均在 Cargo.lock，无新解析；若 cargo 要求 `objc2-core-location` 等传递件已在 lock 内）；**无新增 npm 依赖**
- 命令平台稳定：4 个 native 命令始终注册（非 macOS 桩返回 `unsupported`），bindings 不随平台漂移；bindings 单点登记（`collect_commands!` + `export_lists_every_command` 清单 + 重新生成）
- TS 契约只做加法：`NotificationsApi` 不变；mock/web stub 不变；native 任何失败必须回退 2B 插件路径并如实报 `authoritative:false`，绝不静默成功
- 内置音效资产：从 `/Users/itsuka/CodeSpace/orca/resources/notification-sounds/` 拷贝 9 个 mp3（同作者 fork 资源，规格 §6.6），git 跟踪
- 提交信息：中文 conventional commits（`feat(...)`/`fix(...)`），结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 执行前用 superpowers:using-git-worktrees（或沿用既有 SDD 会话先例的在库 feature branch）

---

### Task 1: CFBundleIdentifier + native 授权读口/触发命令（含 dev 可用性 spike）

**Files:**
- Modify: `src-tauri/macos-info.plist`
- Modify: `src-tauri/crates/ade-bridge/Cargo.toml`
- Create: `src-tauri/crates/ade-bridge/src/commands/notifications_native.rs`
- Modify: `src-tauri/crates/ade-bridge/src/commands/mod.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: `AppState.app: AppHandle`（`commands/state.rs`）、`commands::run_blocking`
- Produces（Task 2/4/5 依赖）:
  - `NotificationAuthorizationStatus`（serde `kebab-case`：`authorized|denied|not-determined|unknown`）
  - `NotificationAuthorizationResult { status, available: bool }`（camelCase）
  - 命令 `notifications_get_authorization_status()` / `notifications_request_authorization()`（后者返回同一结果形状）
  - `map_status` 纯映射（UN 常量 → 枚举；macOS-only）

- [x] **Step 1: 加 CFBundleIdentifier 与 macOS 依赖**

`src-tauri/macos-info.plist` 在 `<dict>` 后追加（放在现有注释之后）：

```xml
	<key>CFBundleIdentifier</key>
	<string>dev.itsuka.orcinus</string>
```

`src-tauri/crates/ade-bridge/Cargo.toml` 末尾追加：

```toml
[target.'cfg(target_os = "macos")'.dependencies]
block2 = "0.6"
objc2 = "0.6"
objc2-foundation = "0.3"
objc2-user-notifications = "0.3"
```

- [x] **Step 2: dev 二进制 UN 可用性 spike（先验证，再决定验收位置）**

> **Spike 结论（实测）：UN 仅打包产物可用。** 裸二进制与「嵌入 `CFBundleIdentifier` plist」两形态调用 `UNUserNotificationCenter.currentNotificationCenter()` 均抛 `NSInternalInconsistencyException: bundleProxyForCurrentProcess is nil` 并 exit 134（不可捕获；plist 本身可被 `NSBundle` 读到但 LaunchServices 不认）。据此加 R3 打包进程门（`is_bundled_app_process()`，4 命令前置短路），dev 恒走插件回退、不崩；Task 6 手工验收改在 `pnpm tauri build` 产物上执行。详见 task-1-report.md §1/§7 与本收尾记录 §5.1。

```bash
cat > /tmp/un_probe.swift <<'EOF'
import UserNotifications
import Foundation
let sem = DispatchSemaphore(value: 0)
UNUserNotificationCenter.current().getNotificationSettings { settings in
  print("auth=\(settings.authorizationStatus.rawValue) bundle=\(Bundle.main.bundleIdentifier ?? "nil")")
  sem.signal()
}
_ = sem.wait(timeout: .now() + 3)
EOF
swiftc /tmp/un_probe.swift -o /tmp/un_probe
/tmp/un_probe
```

Expected（裸二进制，无 plist）：打印 `auth=… bundle=nil` 或超时/abort——记录实际输出。
再验证「嵌入带 CFBundleIdentifier 的 plist」形态：

```bash
swiftc -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker src-tauri/macos-info.plist /tmp/un_probe.swift -o /tmp/un_probe_bundled
/tmp/un_probe_bundled
```

Expected：`bundle=dev.itsuka.orcinus` 且 `auth=` 有值 → dev 路径可用；若仍 abort/超时 → 在任务报告中记为「UN 仅打包产物可用」，回退网照常（功能不退化），Task 6 手工验收改在 `tauri build` 产物上执行。`swiftc` 不存在时记录并跳过该步骤（不阻塞）。

- [x] **Step 3: 写失败测试**

创建 `src-tauri/crates/ade-bridge/src/commands/notifications_native.rs`（骨架 + 测试）：

```rust
use serde::Serialize;

use crate::errors::BridgeError;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationAuthorizationStatus {
    Authorized,
    Denied,
    NotDetermined,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationAuthorizationResult {
    pub status: NotificationAuthorizationStatus,
    pub available: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_get_authorization_status(
    state: tauri::State<'_, AppState>,
) -> Result<NotificationAuthorizationResult, BridgeError> {
    todo!()
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_request_authorization(
    state: tauri::State<'_, AppState>,
) -> Result<NotificationAuthorizationResult, BridgeError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_status_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_value(NotificationAuthorizationStatus::NotDetermined).unwrap(),
            serde_json::json!("not-determined")
        );
        assert_eq!(
            serde_json::to_value(NotificationAuthorizationStatus::Authorized).unwrap(),
            serde_json::json!("authorized")
        );
        assert_eq!(
            serde_json::to_value(NotificationAuthorizationStatus::Denied).unwrap(),
            serde_json::json!("denied")
        );
        assert_eq!(
            serde_json::to_value(NotificationAuthorizationStatus::Unknown).unwrap(),
            serde_json::json!("unknown")
        );
    }

    #[test]
    fn authorization_result_serializes_camel_case() {
        assert_eq!(
            serde_json::to_value(NotificationAuthorizationResult {
                status: NotificationAuthorizationStatus::Authorized,
                available: true,
            })
            .unwrap(),
            serde_json::json!({ "status": "authorized", "available": true })
        );
    }
}
```

`commands/mod.rs` 加 `pub mod notifications_native;`。

- [x] **Step 4: 运行确认失败**

Run: `cargo test -p ade-bridge notifications_native`（在 `src-tauri/` 下）
Expected: 编译通过但命令 `todo!()`（未注册前测试可过 serde 两项；`todo!()` 仅编译占位）——若两测试已绿，进入 Step 5 实现；命令未注册前不影响既有测试

- [x] **Step 5: 实现**

`notifications_native.rs` 替换 `todo!()` 并追加 macOS 模块：

```rust
#[tauri::command]
#[specta::specta]
pub async fn notifications_get_authorization_status(
    state: tauri::State<'_, AppState>,
) -> Result<NotificationAuthorizationResult, BridgeError> {
    #[cfg(target_os = "macos")]
    {
        let app = state.app.clone();
        return crate::commands::run_blocking(move || {
            crate::commands::notifications_native::macos::read_authorization_status(&app)
        })
        .await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &state;
        Ok(NotificationAuthorizationResult {
            status: NotificationAuthorizationStatus::Unknown,
            available: false,
        })
    }
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_request_authorization(
    state: tauri::State<'_, AppState>,
) -> Result<NotificationAuthorizationResult, BridgeError> {
    #[cfg(target_os = "macos")]
    {
        let app = state.app.clone();
        return crate::commands::run_blocking(move || {
            crate::commands::notifications_native::macos::request_authorization(&app)
        })
        .await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &state;
        Ok(NotificationAuthorizationResult {
            status: NotificationAuthorizationStatus::Unknown,
            available: false,
        })
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNNotificationSettings,
        UNUserNotificationCenter,
    };
    use tauri::AppHandle;

    use super::{
        NotificationAuthorizationResult, NotificationAuthorizationStatus,
    };
    use crate::errors::BridgeError;

    const CALLBACK_TIMEOUT: Duration = Duration::from_secs(2);

    pub fn map_status(status: UNAuthorizationStatus) -> NotificationAuthorizationStatus {
        if status == UNAuthorizationStatus::Authorized
            || status == UNAuthorizationStatus::Provisional
            || status == UNAuthorizationStatus::Ephemeral
        {
            NotificationAuthorizationStatus::Authorized
        } else if status == UNAuthorizationStatus::Denied {
            NotificationAuthorizationStatus::Denied
        } else if status == UNAuthorizationStatus::NotDetermined {
            NotificationAuthorizationStatus::NotDetermined
        } else {
            NotificationAuthorizationStatus::Unknown
        }
    }

    /// 在主线程发起一次 UN 调用并通过 mpsc 取回结果；超时/调度失败 → Err。
    fn read_status_once(app: &AppHandle) -> Result<NotificationAuthorizationStatus, BridgeError> {
        let (tx, rx) = mpsc::sync_channel::<NotificationAuthorizationStatus>(1);
        app.run_on_main_thread(move || {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
                // SAFETY: ObjC 传回的 settings 在回调期间有效。
                let settings = unsafe { settings.as_ref() };
                let _ = tx.send(map_status(settings.authorizationStatus()));
            });
            center.getNotificationSettingsWithCompletionHandler(&block);
        })
        .map_err(|error| BridgeError::message(format!("read status: dispatch failed: {error}")))?;
        rx.recv_timeout(CALLBACK_TIMEOUT)
            .map_err(|_| BridgeError::message("read status: callback timed out"))
    }

    pub fn read_authorization_status(
        app: &AppHandle,
    ) -> Result<NotificationAuthorizationResult, BridgeError> {
        Ok(NotificationAuthorizationResult {
            status: read_status_once(app)?,
            available: true,
        })
    }

    pub fn request_authorization(
        app: &AppHandle,
    ) -> Result<NotificationAuthorizationResult, BridgeError> {
        let (tx, rx) = mpsc::sync_channel::<bool>(1);
        app.run_on_main_thread(move || {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
            let block = RcBlock::new(move |granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| {
                let _ = tx.send(granted.as_bool());
            });
            center.requestAuthorizationWithOptions_completionHandler(options, &block);
        })
        .map_err(|error| {
            BridgeError::message(format!("request authorization: dispatch failed: {error}"))
        })?;
        let granted = rx
            .recv_timeout(CALLBACK_TIMEOUT)
            .map_err(|_| BridgeError::message("request authorization: callback timed out"))?;
        // 以权威读口为准（用户点「稍后」会留在 not-determined）；读口失败时退回 granted 布尔。
        match read_status_once(app) {
            Ok(status) => Ok(NotificationAuthorizationResult {
                status,
                available: true,
            }),
            Err(error) => {
                eprintln!("[ade-bridge] request authorization re-read failed: {error}");
                Ok(NotificationAuthorizationResult {
                    status: if granted {
                        NotificationAuthorizationStatus::Authorized
                    } else {
                        NotificationAuthorizationStatus::Denied
                    },
                    available: true,
                })
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maps_un_status_constants_like_the_oracle() {
            assert_eq!(
                map_status(UNAuthorizationStatus::Authorized),
                NotificationAuthorizationStatus::Authorized
            );
            assert_eq!(
                map_status(UNAuthorizationStatus::Provisional),
                NotificationAuthorizationStatus::Authorized
            );
            assert_eq!(
                map_status(UNAuthorizationStatus::Ephemeral),
                NotificationAuthorizationStatus::Authorized
            );
            assert_eq!(
                map_status(UNAuthorizationStatus::Denied),
                NotificationAuthorizationStatus::Denied
            );
            assert_eq!(
                map_status(UNAuthorizationStatus::NotDetermined),
                NotificationAuthorizationStatus::NotDetermined
            );
        }
    }
}
```

- [x] **Step 6: 注册命令 + 重新生成 bindings**

`specta_export.rs`：`collect_commands!` 在 `commands::notifications::notifications_read_sound,` 后加两行：

```rust
                commands::notifications_native::notifications_get_authorization_status,
                commands::notifications_native::notifications_request_authorization,
```

`.typ` 链加：

```rust
            .typ::<commands::notifications_native::NotificationAuthorizationStatus>()
            .typ::<commands::notifications_native::NotificationAuthorizationResult>()
```

`export_lists_every_command` 清单加 `"notifications_get_authorization_status"`、`"notifications_request_authorization"`。

Run: `cargo run -p ade-bridge --bin export-bindings`（在 `src-tauri/` 下）
Expected: bindings 出现两命令 + `NotificationAuthorizationStatus`/`NotificationAuthorizationResult`

- [x] **Step 7: 运行测试确认通过**

Run: `cargo test -p ade-bridge`
Expected: 全绿（含 `bindings_are_fresh`/`export_lists_every_command`；macOS 下 `map_status` 测试通过）

- [x] **Step 8: Commit**

```bash
git add src-tauri/macos-info.plist src-tauri/Cargo.lock src-tauri/crates/ade-bridge src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): macOS UN 授权读口/触发命令 + CFBundleIdentifier（含 dev 可用性 spike）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: native 投递 / dismiss 命令

**Files:**
- Modify: `src-tauri/crates/ade-bridge/src/commands/notifications_native.rs`
- Modify: `src-tauri/crates/ade-bridge/src/specta_export.rs`
- Regenerate: `src/bridge/real/generated/tauri-bindings.ts`

**Interfaces:**
- Consumes: Task 1 的 `macos` 模块（`CALLBACK_TIMEOUT`/`read_status_once` 同文件）与命令文件
- Produces（Task 5 依赖）:
  - `NotificationsDeliverNativeArgs { id, title, body, silent: bool }`（camelCase，`silent` default false）
  - `NotificationNativeDeliverResult { ok: bool, error?: string }`
  - `NotificationsDismissNativeArgs { ids: Vec<String> }`
  - `NotificationNativeDismissResult { dismissed: u32 }`
  - 命令 `notifications_deliver_native` / `notifications_dismiss_native`（dismiss 先 `getDeliveredNotifications` 求交集再删，返回真实命中数）

- [x] **Step 1: 写失败测试**

`notifications_native.rs` 追加类型与测试：

```rust
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsDeliverNativeArgs {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub silent: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationNativeDeliverResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct NotificationsDismissNativeArgs {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationNativeDismissResult {
    pub dismissed: u32,
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_deliver_native(
    state: tauri::State<'_, AppState>,
    args: NotificationsDeliverNativeArgs,
) -> Result<NotificationNativeDeliverResult, BridgeError> {
    todo!()
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_dismiss_native(
    state: tauri::State<'_, AppState>,
    args: NotificationsDismissNativeArgs,
) -> Result<NotificationNativeDismissResult, BridgeError> {
    todo!()
}
```

测试追加：

```rust
    #[test]
    fn deliver_args_and_results_serialize_camel_case() {
        let args: NotificationsDeliverNativeArgs = serde_json::from_value(serde_json::json!({
            "id": "agent:w1:leaf:1",
            "title": "claude",
            "body": "Waiting for input",
            "silent": true
        }))
        .unwrap();
        assert_eq!(args.id, "agent:w1:leaf:1");
        assert!(args.silent);
        assert_eq!(
            serde_json::to_value(NotificationNativeDeliverResult { ok: true, error: None }).unwrap(),
            serde_json::json!({ "ok": true })
        );
        assert_eq!(
            serde_json::to_value(NotificationNativeDeliverResult {
                ok: false,
                error: Some("boom".to_string()),
            })
            .unwrap(),
            serde_json::json!({ "ok": false, "error": "boom" })
        );
    }

    #[test]
    fn dismiss_args_and_result_serialize_camel_case() {
        let args: NotificationsDismissNativeArgs =
            serde_json::from_value(serde_json::json!({ "ids": ["a", "b"] })).unwrap();
        assert_eq!(args.ids, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            serde_json::to_value(NotificationNativeDismissResult { dismissed: 1 }).unwrap(),
            serde_json::json!({ "dismissed": 1 })
        );
    }
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p ade-bridge notifications_native`
Expected: `todo!()` 相关的命令未被直接调用，先绿 serde 测试；随后 Step 3 实现

- [x] **Step 3: 实现**

命令体（非 macOS 桩在 `#[cfg(not(...))]` 内返回 `ok:false/error:"unsupported-platform"` 与 `{dismissed:0}`；macOS 走 `macos::deliver_native`/`macos::dismiss_native`，结构同 Task 1 的 `run_blocking` 桥接）。

`macos` 模块追加：

```rust
    use objc2_foundation::{NSArray, NSError, NSString};
    use objc2_user_notifications::{
        UNMutableNotificationContent, UNNotification, UNNotificationRequest, UNNotificationSound,
    };

    pub fn deliver_native(
        app: &AppHandle,
        id: String,
        title: String,
        body: String,
        silent: bool,
    ) -> Result<bool, BridgeError> {
        let (tx, rx) = mpsc::sync_channel::<bool>(1);
        app.run_on_main_thread(move || {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&title));
            content.setBody(&NSString::from_str(&body));
            if !silent {
                let sound = UNNotificationSound::defaultSound();
                content.setSound(Some(&sound));
            }
            let identifier = NSString::from_str(&id);
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &identifier,
                &content,
                None,
            );
            let block = RcBlock::new(move |error: *mut NSError| {
                let _ = tx.send(error.is_null());
            });
            let center = UNUserNotificationCenter::currentNotificationCenter();
            center.addNotificationRequest_withCompletionHandler(&request, Some(&block));
        })
        .map_err(|error| BridgeError::message(format!("deliver: dispatch failed: {error}")))?;
        rx.recv_timeout(CALLBACK_TIMEOUT)
            .map_err(|_| BridgeError::message("deliver: callback timed out"))
    }

    pub fn dismiss_native(app: &AppHandle, ids: Vec<String>) -> Result<u32, BridgeError> {
        let (tx, rx) = mpsc::sync_channel::<u32>(1);
        app.run_on_main_thread(move || {
            let wanted: std::collections::HashSet<String> = ids.into_iter().collect();
            let block = RcBlock::new(move |delivered: NonNull<NSArray<UNNotification>>| {
                // SAFETY: 回调期数组有效。
                let delivered = unsafe { delivered.as_ref() };
                let mut matched: Vec<objc2::rc::Retained<NSString>> = Vec::new();
                for notification in delivered.iter() {
                    let identifier = notification.request().identifier();
                    if wanted.contains(&identifier.to_string()) {
                        matched.push(identifier);
                    }
                }
                let dismissed = matched.len() as u32;
                if !matched.is_empty() {
                    let array = NSArray::from_retained_slice(&matched);
                    let center = UNUserNotificationCenter::currentNotificationCenter();
                    center.removeDeliveredNotificationsWithIdentifiers(&array);
                }
                let _ = tx.send(dismissed);
            });
            let center = UNUserNotificationCenter::currentNotificationCenter();
            center.getDeliveredNotificationsWithCompletionHandler(&block);
        })
        .map_err(|error| BridgeError::message(format!("dismiss: dispatch failed: {error}")))?;
        rx.recv_timeout(CALLBACK_TIMEOUT)
            .map_err(|_| BridgeError::message("dismiss: callback timed out"))
    }
```

命令体（macOS 分支）：

```rust
        let app = state.app.clone();
        let args = args;
        return crate::commands::run_blocking(move || {
            match crate::commands::notifications_native::macos::deliver_native(
                &app, args.id, args.title, args.body, args.silent,
            ) {
                Ok(true) => Ok(NotificationNativeDeliverResult { ok: true, error: None }),
                Ok(false) => Ok(NotificationNativeDeliverResult {
                    ok: false,
                    error: Some("delivery failed".to_string()),
                }),
                Err(error) => Ok(NotificationNativeDeliverResult {
                    ok: false,
                    error: Some(error.to_string()),
                }),
            }
        })
        .await;
```

dismiss 同形：`macos::dismiss_native(&app, args.ids)` → `Ok(NotificationNativeDismissResult { dismissed })`，`Err` → `dismissed: 0`。

- [x] **Step 4: 注册 + bindings + 测试**

`specta_export.rs` 加两命令（`collect_commands!`、清单测试）与 `.typ`：

```rust
            .typ::<commands::notifications_native::NotificationNativeDeliverResult>()
            .typ::<commands::notifications_native::NotificationNativeDismissResult>()
```

Run: `cargo run -p ade-bridge --bin export-bindings && cargo test -p ade-bridge`
Expected: bindings 更新，全部测试绿

- [x] **Step 5: Commit**

```bash
git add src-tauri/crates/ade-bridge src/bridge/real/generated/tauri-bindings.ts
git commit -m "feat(bridge): UN 原生投递/精确 dismiss 命令（交集计数）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

### Task 3: 内置音效资产 + playSound 播放

**Files:**
- Create: `resources/notification-sounds/{two-tone,bong,thump,blip,sonar,blop,ding,clack,beep}.mp3`（拷自 oracle）
- Create: `src/renderer/src/lib/built-in-notification-sounds.ts`
- Modify: `src/bridge/real/notifications.ts`（playSound 解析顺序 + 抽出播放助手）
- Modify: `src/bridge/real/notifications.test.ts`

**Interfaces:**
- Consumes: `getBootstrap().settings.notifications.customSoundId/customSoundPath`、现有 `playingSoundPaths`/`NotificationSoundResult`
- Produces:
  - `BUILT_IN_SOUND_URLS: Record<BuiltInNotificationSoundId, string>`、`builtInSoundUrl(id: string): string | null`
  - `playSound` 支持 9 个内置 id（dedupe 键 = id；`force` 跳过在播去重；音量 0–100 → /100）

- [x] **Step 1: 拷贝资产并建映射**

```bash
mkdir -p resources/notification-sounds
cp /Users/itsuka/CodeSpace/orca/resources/notification-sounds/{two-tone,bong,thump,blip,sonar,blop,ding,clack,beep}.mp3 resources/notification-sounds/
ls -l resources/notification-sounds/   # 期望 9 个文件，各 12717 字节
```

`src/renderer/src/lib/built-in-notification-sounds.ts`：

```ts
// Why: orca 的 9 个内置提示音随仓库分发；renderer 经 Vite `?url` 直接拿到
// 构建产物 URL 播放（与 AppIconSelector 的资源导入同机制），无需宿主 IPC。
import twoToneUrl from '../../../../resources/notification-sounds/two-tone.mp3?url'
import bongUrl from '../../../../resources/notification-sounds/bong.mp3?url'
import thumpUrl from '../../../../resources/notification-sounds/thump.mp3?url'
import blipUrl from '../../../../resources/notification-sounds/blip.mp3?url'
import sonarUrl from '../../../../resources/notification-sounds/sonar.mp3?url'
import blopUrl from '../../../../resources/notification-sounds/blop.mp3?url'
import dingUrl from '../../../../resources/notification-sounds/ding.mp3?url'
import clackUrl from '../../../../resources/notification-sounds/clack.mp3?url'
import beepUrl from '../../../../resources/notification-sounds/beep.mp3?url'

export const BUILT_IN_NOTIFICATION_SOUND_IDS = [
  'two-tone',
  'bong',
  'thump',
  'blip',
  'sonar',
  'blop',
  'ding',
  'clack',
  'beep'
] as const

export type BuiltInNotificationSoundId = (typeof BUILT_IN_NOTIFICATION_SOUND_IDS)[number]

export const BUILT_IN_SOUND_URLS: Record<BuiltInNotificationSoundId, string> = {
  'two-tone': twoToneUrl,
  bong: bongUrl,
  thump: thumpUrl,
  blip: blipUrl,
  sonar: sonarUrl,
  blop: blopUrl,
  ding: dingUrl,
  clack: clackUrl,
  beep: beepUrl
}

export function builtInSoundUrl(id: string): string | null {
  return (BUILT_IN_SOUND_URLS as Record<string, string>)[id] ?? null
}
```

- [x] **Step 2: 写失败测试**

`notifications.test.ts` 增补（并在既有 FakeAudio 上加构造参数记录 `src`；custom 测试的 `URL` stub 不受影响）：

```ts
  it('plays built-in sounds from bundled assets, dedupes while playing and honors force', async () => {
    installSettings({ customSoundId: 'two-tone' })
    const sources: string[] = []
    const volumes: number[] = []
    const pending: Array<() => void> = []
    class FakeAudio {
      volume = 1
      onended: (() => void) | null = null
      onerror: (() => void) | null = null
      constructor(src: string) {
        sources.push(src)
      }
      play(): Promise<void> {
        volumes.push(this.volume)
        pending.push(() => this.onended?.())
        return Promise.resolve()
      }
    }
    vi.stubGlobal('Audio', FakeAudio)
    const api = createNotificationsRealApi()
    // 第一次调用同步注册在播集合（playAudio 之前的 add 是同步的）。
    const first = api.playSound({ volume: 60 })
    expect(await api.playSound({ volume: 60 })).toEqual({ played: false, reason: 'deduped' })
    const forced = api.playSound({ volume: 60, force: true })
    pending.forEach((finish) => finish())
    expect(await first).toEqual({ played: true })
    expect(await forced).toEqual({ played: true })
    expect(sources).toEqual([expect.stringMatching(/two-tone/), expect.stringMatching(/two-tone/)])
    expect(volumes[0]).toBe(0.6)
    vi.unstubAllGlobals()
  })

  it('returns missing-path for system and unknown ids', async () => {
    installSettings({ customSoundId: 'system' })
    expect(await createNotificationsRealApi().playSound({})).toEqual({
      played: false,
      reason: 'missing-path'
    })
    installSettings({ customSoundId: 'not-a-sound' as never })
    expect(await createNotificationsRealApi().playSound({})).toEqual({
      played: false,
      reason: 'missing-path'
    })
  })
```

> 注：`force` 用例依赖「首次调用在 playAudio 前同步注册在播集合」这一实现细节；若顺序调整导致该断言不稳，改为 deferred 控制 `onended` 后逐次 await。

- [x] **Step 3: 运行确认失败**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts`
Expected: 内置音效用例失败（当前 `missing-path`）

- [x] **Step 4: 实现**

`real/notifications.ts` 顶部加 `import { builtInSoundUrl } from '../../renderer/src/lib/built-in-notification-sounds'`（路径按实际相对位置核对：`src/bridge/real/` → `../../renderer/src/lib/...`），并加播放助手：

```ts
function playAudio(url: string, volume: number | null): Promise<void> {
  return new Promise((resolve, reject) => {
    const audio = new Audio(url)
    if (volume !== null) {
      // Why: renderer contract is 0..100 (preload divides by 100); Audio.volume is 0..1.
      audio.volume = Math.min(1, Math.max(0, volume))
    }
    audio.onended = () => resolve()
    audio.onerror = () => reject(new Error('playback failed'))
    void audio.play().catch(reject)
  })
}
```

`playSound` 改为（保留 custom 通道原行为）：

```ts
    playSound: async (options): Promise<NotificationSoundResult> => {
      const settings = getBootstrap()?.settings?.notifications
      const volume = typeof options?.volume === 'number' ? options.volume / 100 : null
      const soundId = settings?.customSoundId
      if (soundId && soundId !== 'custom' && soundId !== 'system') {
        const url = builtInSoundUrl(soundId)
        if (!url) {
          return { played: false, reason: 'missing-path' }
        }
        if (options?.force !== true && playingSoundPaths.has(soundId)) {
          return { played: false, reason: 'deduped' }
        }
        playingSoundPaths.add(soundId)
        try {
          await playAudio(url, volume)
          return { played: true }
        } catch {
          return { played: false, reason: 'playback-failed' }
        } finally {
          playingSoundPaths.delete(soundId)
        }
      }
      const path = settings?.customSoundPath
      if (!path || soundId !== 'custom') {
        return { played: false, reason: 'missing-path' }
      }
      if (options?.force !== true && playingSoundPaths.has(path)) {
        return { played: false, reason: 'deduped' }
      }
      const loaded = await invokeCommand<{/* 同现状 */}>('notifications_read_sound', { args: { path } })
      // …现状失败映射保持不变…
      const bytes = Uint8Array.from(atob(loaded.dataBase64), (char) => char.charCodeAt(0))
      const objectUrl = URL.createObjectURL(new Blob([bytes], { type: loaded.mimeType }))
      playingSoundPaths.add(path)
      try {
        await playAudio(objectUrl, volume)
        return { played: true }
      } catch {
        return { played: false, reason: 'playback-failed' }
      } finally {
        playingSoundPaths.delete(path)
        URL.revokeObjectURL(objectUrl)
      }
    }
```

- [x] **Step 5: 运行确认通过 + typecheck**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts && pnpm typecheck`
Expected: 全绿（既有 custom 用例的 `volume: 30 → 0.3` 断言保持；`?url` 导入类型由 Vite 客户端类型提供）

- [x] **Step 6: Commit**

```bash
git add resources/notification-sounds src/renderer/src/lib/built-in-notification-sounds.ts src/bridge/real/notifications.ts src/bridge/real/notifications.test.ts
git commit -m "feat(renderer): 9 个内置通知音效资源与播放解析（含 force/dedupe/音量契约）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: TS native 分流 + 权限语义（requested 持久化 / probeDelivery）

**Files:**
- Modify: `src/bridge/real/notifications.ts`
- Modify: `src/bridge/real/notifications.test.ts`

**Interfaces:**
- Consumes: Task 1 两命令；`ui_get`/`ui_set` 命令；持久化字段 `notificationPermissionRequested`（`src/shared/persisted-ui-state-types.ts:114`）
- Produces（Task 5 依赖）:
  - 域实例内 `nativeAvailable` 能力探测 + `readNativeStatus()`/`requestNativeAuthorization()`/`readRequestedFlag()`/`stampRequestedFlag()`
  - `getPermissionStatus`：`requested` 读持久化标志（只读不改）
  - `probeDelivery`：darwin + native → 权威三态；`not-determined` 每实例触发一次授权窗并写标志；native 不可用 → 现插件回退（`authoritative:false`）

- [x] **Step 1: 写失败测试**

```ts
  it('uses the native authoritative readout for permission status and probe', async () => {
    let statusCall = 0
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        statusCall += 1
        return { status: 'authorized', available: true }
      }
      if (command === 'ui_get') {
        return { notificationPermissionRequested: false }
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: false
    })
    expect(await api.probeDelivery()).toEqual({ state: 'delivered', authoritative: true })
    // ensureNativeAvailability + readNativeStatus 各读一次。
    expect(statusCall).toBe(2)
  })

  it('triggers the authorization dialog once per session on not-determined and stamps requested', async () => {
    let requested = false
    let requestCalls = 0
    const uiWrites: unknown[] = []
    invokeMock.mockImplementation(async (command: string, payload?: { args?: unknown }) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: requested ? 'authorized' : 'not-determined', available: true }
      }
      if (command === 'notifications_request_authorization') {
        requestCalls += 1
        requested = true
        return { status: 'authorized', available: true }
      }
      if (command === 'ui_get') {
        return { notificationPermissionRequested: false }
      }
      if (command === 'ui_set') {
        uiWrites.push(payload?.args)
        return payload?.args
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.probeDelivery()).toEqual({ state: 'delivered', authoritative: true })
    expect(await api.probeDelivery()).toEqual({ state: 'delivered', authoritative: true })
    expect(requestCalls).toBe(1)
    expect(uiWrites).toContainEqual({ notificationPermissionRequested: true })
  })

  it('reports denied as authoritative blocked and reads requested from persisted ui state', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'denied', available: true }
      }
      if (command === 'ui_get') {
        return { notificationPermissionRequested: true }
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.probeDelivery()).toEqual({ state: 'blocked', authoritative: true })
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: true
    })
  })

  it('falls back to the plugin probe when the native channel is unavailable', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        throw new Error('unavailable')
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    permissionGranted = false
    requestResult = 'denied'
    const api = createNotificationsRealApi()
    expect(await api.probeDelivery()).toEqual({ state: 'blocked', authoritative: false })
    expect(await api.getPermissionStatus()).toEqual({
      supported: true,
      platform: 'darwin',
      requested: false
    })
  })
```

> 注：既有 `reports blocked-by-system when permission stays denied` 用例的 `getPermissionStatus.requested` 断言将变为 `false`（ui_get 未 mock → undefined → false），保持即可；若其 mock 返回 undefined 导致 native 探测失败，该用例走插件回退，行为不变。

- [x] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts`
Expected: 新用例失败（当前全部走插件假授权）

- [x] **Step 3: 实现**

在 `createNotificationsRealApi` 工厂内、`return withMethodFallback` 之前加实例态与助手：

```ts
type NativeAuthorizationStatus = 'authorized' | 'denied' | 'not-determined' | 'unknown'
type NativeAuthorizationResult = { status: NativeAuthorizationStatus; available: boolean }

// 工厂内（每域实例一份）：
let nativeAvailable: boolean | null = null
let probeRequestedThisSession = false

const ensureNativeAvailability = async (): Promise<boolean> => {
  if (platformForPermissionStatus() !== 'darwin') {
    nativeAvailable = false
    return false
  }
  if (nativeAvailable !== null) {
    return nativeAvailable
  }
  try {
    const result = await invokeCommand<NativeAuthorizationResult>(
      'notifications_get_authorization_status'
    )
    nativeAvailable = result?.available === true
    if (!nativeAvailable) {
      console.warn('native notification channel unavailable; using the plugin fallback')
    }
  } catch (error) {
    console.warn('native notification channel unavailable; using the plugin fallback', error)
    nativeAvailable = false
  }
  return nativeAvailable
}

const readNativeStatus = async (): Promise<NativeAuthorizationStatus | null> => {
  try {
    const result = await invokeCommand<NativeAuthorizationResult>(
      'notifications_get_authorization_status'
    )
    return result?.status ?? null
  } catch (error) {
    console.warn('native authorization read failed', error)
    return null
  }
}

const requestNativeAuthorization = async (): Promise<NativeAuthorizationStatus | null> => {
  try {
    const result = await invokeCommand<NativeAuthorizationResult>(
      'notifications_request_authorization'
    )
    return result?.status ?? null
  } catch (error) {
    console.warn('native authorization request failed', error)
    return null
  }
}

const readRequestedFlag = async (): Promise<boolean> => {
  try {
    const ui = await invokeCommand<{ notificationPermissionRequested?: boolean }>('ui_get')
    return ui?.notificationPermissionRequested === true
  } catch {
    return false
  }
}

const stampRequestedFlag = async (): Promise<void> => {
  try {
    await invokeCommand('ui_set', { args: { notificationPermissionRequested: true } })
  } catch (error) {
    console.warn('failed to persist notificationPermissionRequested', error)
  }
}
```

`getPermissionStatus` 替换为：

```ts
    getPermissionStatus: async (): Promise<NotificationPermissionStatusResult> => ({
      supported: true,
      platform: platformForPermissionStatus(),
      requested: await readRequestedFlag()
    }),
```

`probeDelivery` 替换为：

```ts
    probeDelivery: async () => {
      if (await ensureNativeAvailability()) {
        let status = await readNativeStatus()
        if (status === 'not-determined' && !probeRequestedThisSession) {
          probeRequestedThisSession = true
          status = (await requestNativeAuthorization()) ?? status
          void stampRequestedFlag()
        }
        if (status === 'authorized') {
          return { state: 'delivered', authoritative: true }
        }
        if (status === 'denied') {
          return { state: 'blocked', authoritative: true }
        }
        if (status === 'not-determined') {
          return { state: 'awaiting-decision', authoritative: true }
        }
        // unknown → 落到插件探测（authoritative:false）
      }
      if (await isPermissionGranted()) {
        return { state: 'delivered', authoritative: false }
      }
      const permission = await requestPermission()
      if (permission === 'granted') {
        return { state: 'delivered', authoritative: false }
      }
      if (permission === 'denied') {
        return { state: 'blocked', authoritative: false }
      }
      return { state: 'awaiting-decision', authoritative: false }
    },
```

- [x] **Step 4: 运行确认通过**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts && pnpm typecheck`
Expected: 全绿

- [x] **Step 5: Commit**

```bash
git add src/bridge/real/notifications.ts src/bridge/real/notifications.test.ts
git commit -m "feat(renderer): notifications native 能力探测 + 授权读口/触发与 requested 持久化语义

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

<!-- PLAN-CONTINUE -->

---

### Task 5: TS 原生投递 / dismiss 分流

**Files:**
- Modify: `src/bridge/real/notifications.ts`
- Modify: `src/bridge/real/notifications.test.ts`

**Interfaces:**
- Consumes: Task 2 两命令；Task 4 的 `ensureNativeAvailability`/`readNativeStatus`/`requestNativeAuthorization`/`stampRequestedFlag`/`probeRequestedThisSession`
- Produces:
  - `dispatch`（darwin + native）：先读授权态——`authorized` → `notifications_deliver_native({id,title,body,silent})`；`denied` → `blocked-by-system`；`not-determined` → 触发授权窗后重读并按结果投递/拒绝；失败 → `not-displayed`
  - 标识符：有 `args.notificationId` 用之；否则 `orcinus:<uuid>`（`crypto.randomUUID` 不可用时退 `Date.now()` 组合）
  - 静音规则：`customSoundId === 'system'` → `silent:false`，其余 → `silent:true`
  - `dismiss`（darwin + native）：原始字符串 ids 走 `notifications_dismiss_native`，返回真实交集计数；否则维持哈希 + `removeActive`

- [x] **Step 1: 写失败测试**

```ts
  it('delivers through the native channel with a stable identifier and system-sound rule', async () => {
    const deliverCalls: unknown[] = []
    invokeMock.mockImplementation(async (command: string, payload?: { args?: unknown }) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'authorized', available: true }
      }
      if (command === 'notifications_deliver_native') {
        deliverCalls.push(payload?.args)
        return { ok: true }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    installSettings({ customSoundId: 'two-tone' })
    const api = createNotificationsRealApi()
    const result = await api.dispatch({
      source: 'agent-task-complete',
      worktreeId: 'r-native',
      notificationId: 'agent:r-native:t1:leaf:100',
      terminalTitle: 'claude',
      agentState: 'waiting',
      agentPrompt: 'Fix login bug'
    })
    expect(result).toEqual({ delivered: true })
    expect(sendNotificationMock).not.toHaveBeenCalled()
    expect(deliverCalls[0]).toEqual({
      id: 'agent:r-native:t1:leaf:100',
      title: 'claude',
      body: 'Fix login bug',
      silent: true
    })
    installSettings({ customSoundId: 'system' })
    await createNotificationsRealApi().dispatch({
      source: 'agent-task-complete',
      worktreeId: 'r-native-2',
      notificationId: 'agent:r-native-2:t1:leaf:200'
    })
    expect(deliverCalls[1]).toMatchObject({ silent: false })
  })

  it('returns blocked-by-system for denied and not-displayed for native delivery failure', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'denied', available: true }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    expect(
      await createNotificationsRealApi().dispatch({
        source: 'agent-task-complete',
        worktreeId: 'r-denied'
      })
    ).toEqual({ delivered: false, reason: 'blocked-by-system' })

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'authorized', available: true }
      }
      if (command === 'notifications_deliver_native') {
        return { ok: false, error: 'boom' }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    expect(
      await createNotificationsRealApi().dispatch({
        source: 'agent-task-complete',
        worktreeId: 'r-failed'
      })
    ).toEqual({ delivered: false, reason: 'not-displayed' })
  })

  it('requests authorization on not-determined before delivering', async () => {
    let requested = false
    const commands: string[] = []
    invokeMock.mockImplementation(async (command: string) => {
      commands.push(command)
      if (command === 'notifications_get_authorization_status') {
        return { status: requested ? 'authorized' : 'not-determined', available: true }
      }
      if (command === 'notifications_request_authorization') {
        requested = true
        return { status: 'authorized', available: true }
      }
      if (command === 'notifications_deliver_native') {
        return { ok: true }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    const result = await createNotificationsRealApi().dispatch({
      source: 'agent-task-complete',
      worktreeId: 'r-ask'
    })
    expect(result).toEqual({ delivered: true })
    expect(commands).toContain('notifications_request_authorization')
    expect(commands).toContain('notifications_deliver_native')
  })

  it('dismisses through the native channel with raw ids and counts the intersection', async () => {
    const dismissCalls: unknown[] = []
    invokeMock.mockImplementation(async (command: string, payload?: { args?: unknown }) => {
      if (command === 'notifications_get_authorization_status') {
        return { status: 'authorized', available: true }
      }
      if (command === 'notifications_dismiss_native') {
        dismissCalls.push(payload?.args)
        return { dismissed: 1 }
      }
      if (command === 'ui_get') {
        return {}
      }
      return undefined
    })
    const api = createNotificationsRealApi()
    expect(await api.dismiss(['agent:r1:t1:leaf:100'])).toEqual({ dismissed: 1 })
    expect(dismissCalls[0]).toEqual({ ids: ['agent:r1:t1:leaf:100'] })
  })
```

- [x] **Step 2: 运行确认失败**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts`
Expected: 新用例失败（当前无 dispatch/dismiss 分流）

- [x] **Step 3: 实现**

加随机标识符助手（模块级）：

```ts
function randomNotificationIdentifier(): string {
  try {
    return `orcinus:${crypto.randomUUID()}`
  } catch {
    return `orcinus:${Date.now()}-${Math.random().toString(36).slice(2)}`
  }
}
```

`dispatch` 的插件路径之前插入 native 分支（放在 cooldown 之后、原 `isPermissionGranted` 之前）：

```ts
      if (await ensureNativeAvailability()) {
        let status = await readNativeStatus()
        if (status === 'not-determined') {
          if (!probeRequestedThisSession) {
            probeRequestedThisSession = true
            await requestNativeAuthorization()
          }
          void stampRequestedFlag()
          status = await readNativeStatus()
        }
        if (status === 'denied') {
          return { delivered: false, reason: 'blocked-by-system' }
        }
        if (status === 'authorized') {
          const copy = buildNotificationCopy(args)
          const identifier = args.notificationId ?? randomNotificationIdentifier()
          const nativeResult = await invokeCommand<{ ok: boolean; error?: string }>(
            'notifications_deliver_native',
            {
              args: {
                id: identifier,
                title: copy.title,
                body: copy.body,
                silent: settings?.customSoundId !== 'system'
              }
            }
          )
          if (nativeResult?.ok) {
            return { delivered: true }
          }
          return { delivered: false, reason: 'not-displayed' }
        }
        // unknown → 插件回退
      }
```

`dismiss` 在插件路径之前插入：

```ts
      if (await ensureNativeAvailability()) {
        try {
          const result = await invokeCommand<{ dismissed?: number }>('notifications_dismiss_native', {
            args: { ids: unique }
          })
          return { dismissed: typeof result?.dismissed === 'number' ? result.dismissed : 0 }
        } catch (error) {
          console.warn('native dismiss failed', error)
          return { dismissed: 0 }
        }
      }
```

- [x] **Step 4: 运行确认通过**

Run: `pnpm vitest run src/bridge/real/notifications.test.ts && pnpm typecheck`
Expected: 全绿；既有插件路径用例（invoke 默认 undefined → native 探测失败 → 回退）保持通过

- [x] **Step 5: Commit**

```bash
git add src/bridge/real/notifications.ts src/bridge/real/notifications.test.ts
git commit -m "feat(renderer): 原生投递/dismiss 分流（稳定标识符、静音规则、blocked/not-displayed 映射）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: 收尾记录 + 全量门禁 + 手工验收

**Files:**
- Create: `docs/phase2b1-notifications-native-closeout-record.md`
- Modify: 本计划勾选状态

**Interfaces:**
- Consumes: Task 1-5 产物
- Produces: 验收记录（git 历史为最终依据）

- [x] **Step 1: 全量门禁**

```bash
cd src-tauri && cargo test --workspace
pnpm test
pnpm typecheck && pnpm build:web
```

Expected: 三组全绿（`pnpm test` 的 palette 性能预算用例在负载下可能 flaky，隔离复跑通过即按环境 flaky 记档）。

- [ ] **Step 2: 写记录文档**

`docs/phase2b1-notifications-native-closeout-record.md` 按既有体例（参考 2B record）：交付清单（4 命令/TS 分流/音效资产）、spike 结论（Task 1 Step 2 的实际输出：dev 二进制 UN 可用性 → 验收位置）、验收证据（三组门禁摘要）、手工验收清单（下方六项，标记 `待人工执行`）、偏差与边界备案（至少含：`requested` 语义变更、native 失败回退、dismiss 计数为交集、UN 与插件 NSUserNotificationCenter 不互删、音量 0–100、objc2 版本沿用 lock）。

> 记录文档已写入（自动门禁证据见其 §3）；本 Step 因下方六项手工验收**待人工执行**而保持未勾选（打包产物 `src-tauri/target/release/bundle/macos/Orcinus.app`）。

手工验收清单（待人工执行）：

1. `pnpm dev`（或打包产物，按 spike 结论）：通知权限卡片首启读到真实状态。
2. 系统设置改通知权限 → 卡片 2.5s 轮询内状态实时变化（authoritative）。
3. 首次未决 → probe 触发系统授权弹窗；拒绝后 dispatch → blocked-by-system 回退 toast。
4. 设置页依次试听 9 个内置 + custom：可听、无双声、音量滑杆生效。
5. 触发 waiting 通知 → dismiss（或 `ui-slice-activity-actions` 路径）→ 系统通知中心横幅被移除、计数正确。
6. 重启 app（权限已授权）→ 通知仍正常投递、dismiss 仍匹配（标识符稳定）。

- [x] **Step 3: 计划勾选同步**

把本计划所有实际完成步骤 `- [ ]` 改 `- [x]`；Step 2 手工项保持未勾选并附一行 `待人工执行` 注记。若 spike 结论为「UN 仅打包产物可用」，在 Task 1 Step 2 旁补记。

- [x] **Step 4: Commit**

```bash
git add docs/phase2b1-notifications-native-closeout-record.md docs/superpowers/plans/2026-10-07-phase2b1-notifications-native-closeout.md
git commit -m "docs: Phase 2 子项目 B.1 通知收尾实施记录（门禁证据 + 手工验收清单）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## 自查结论（写计划时完成）

- **规格覆盖**：§2.1 六行实现面 → T1（plist/status/request）/T2（deliver/dismiss）/T3（音效）/T4（权限语义）/T5（投递分流）；§5.1-§5.3 测试与手工面落在各 Task Step 与 T6 清单；§2.2 排除项未开工。
- **接口一致性**：Rust 命令名四处一致（`notifications_get_authorization_status`/`notifications_request_authorization`/`notifications_deliver_native`/`notifications_dismiss_native`：collect_commands/清单测试/bindings/TS invoke）；`NotificationAuthorizationStatus` 的 kebab-case 字符串（authorized/denied/not-determined/unknown）与 TS 联合类型一一对应；`silent` 布尔与静音规则在 T2/T5 两侧一致；dismiss 计数为交集（T2 Rust 求交、T5 TS 透传）。
- **计划内风险已前置**：T1 Step 2 的 dev 二进制 UN spike 给出明确分支（可用→dev 验收；不可用→打包产物验收），回退网在 T4/T5 全程生效；既有测试（invoke 默认 undefined）自动走插件回退，不会被 native 探测破坏。
- **路径核实**：`src/renderer/src/lib/` 到仓库根为 4 级上溯（与 `components/settings/` 的 5 级不同，已按实际核对）；`?url` 导入依赖 Vite 客户端类型（`src/renderer/src/env.d.ts` 已存在该类声明；若 typecheck 报缺失，用 `vite/client` 引用补齐——仅此一处可增量调整）。

