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
pub async fn notifications_get_authorization_status(
    state: tauri::State<'_, AppState>,
) -> Result<NotificationAuthorizationResult, BridgeError> {
    #[cfg(target_os = "macos")]
    {
        if !crate::commands::notifications_native::macos::is_bundled_app_process() {
            return Ok(NotificationAuthorizationResult {
                status: NotificationAuthorizationStatus::Unknown,
                available: false,
            });
        }
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
        if !crate::commands::notifications_native::macos::is_bundled_app_process() {
            return Ok(NotificationAuthorizationResult {
                status: NotificationAuthorizationStatus::Unknown,
                available: false,
            });
        }
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

#[tauri::command]
#[specta::specta]
pub async fn notifications_deliver_native(
    state: tauri::State<'_, AppState>,
    args: NotificationsDeliverNativeArgs,
) -> Result<NotificationNativeDeliverResult, BridgeError> {
    #[cfg(target_os = "macos")]
    {
        // R3：非 .app 进程进入 UN 会 NSInternalInconsistencyException abort，
        // 任何 UN 调用前必须短路。
        if !crate::commands::notifications_native::macos::is_bundled_app_process() {
            return Ok(NotificationNativeDeliverResult {
                ok: false,
                error: Some("not-bundled".to_string()),
            });
        }
        let app = state.app.clone();
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
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &state;
        let _ = &args;
        Ok(NotificationNativeDeliverResult {
            ok: false,
            error: Some("unsupported-platform".to_string()),
        })
    }
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_dismiss_native(
    state: tauri::State<'_, AppState>,
    args: NotificationsDismissNativeArgs,
) -> Result<NotificationNativeDismissResult, BridgeError> {
    #[cfg(target_os = "macos")]
    {
        // R3：非 .app 进程进入 UN 会 NSInternalInconsistencyException abort，
        // 任何 UN 调用前必须短路。
        if !crate::commands::notifications_native::macos::is_bundled_app_process() {
            return Ok(NotificationNativeDismissResult { dismissed: 0 });
        }
        let app = state.app.clone();
        return crate::commands::run_blocking(move || {
            match crate::commands::notifications_native::macos::dismiss_native(&app, args.ids) {
                Ok(dismissed) => Ok(NotificationNativeDismissResult { dismissed }),
                Err(error) => {
                    eprintln!("[ade-bridge] notifications_dismiss_native: {error}");
                    Ok(NotificationNativeDismissResult { dismissed: 0 })
                }
            }
        })
        .await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &state;
        let _ = &args;
        Ok(NotificationNativeDismissResult { dismissed: 0 })
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2_foundation::{NSArray, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
        UNNotificationRequest, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter,
    };
    use tauri::AppHandle;

    use super::{
        NotificationAuthorizationResult, NotificationAuthorizationStatus,
    };
    use crate::errors::BridgeError;

    const CALLBACK_TIMEOUT: Duration = Duration::from_secs(2);

    /// 纯路径判定：只有真实 app bundle 内的可执行文件（`.../Foo.app/Contents/MacOS/...`）
    /// 才可进入 UN —— 非 .app 进程 `UNUserNotificationCenter.currentNotificationCenter()`
    /// 会抛 NSInternalInconsistencyException 直接 abort（spike 实证）。
    pub fn is_bundled_executable_path(path: &std::path::Path) -> bool {
        path.to_string_lossy().contains(".app/Contents/MacOS/")
    }

    /// UNUserNotificationCenter 在非 .app 进程里会抛 NSInternalInconsistencyException
    /// 并直接 abort（spike 实证）；只有打包进程（.../Foo.app/Contents/MacOS/...）才可进入。
    pub fn is_bundled_app_process() -> bool {
        std::env::current_exe()
            .map(|path| is_bundled_executable_path(&path))
            .unwrap_or(false)
    }

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
        // 契约：本命令绝不因「用户尚未应答系统弹窗」而失败。完成回调只在用户
        // 作答后触发，人类应答常超过 CALLBACK_TIMEOUT；超时不是错误，而是
        // 「仍在等待」的常态，此时以权威读口兜底（读到什么就报什么，读口也
        // 失败则保守报 not-determined），始终保持 available: true 的同形状结果。
        let granted = match rx.recv_timeout(CALLBACK_TIMEOUT) {
            Ok(granted) => granted,
            Err(_) => {
                eprintln!(
                    "[ade-bridge] request authorization: callback timed out; falling back to a status read"
                );
                return match read_status_once(app) {
                    Ok(status) => Ok(NotificationAuthorizationResult {
                        status,
                        available: true,
                    }),
                    Err(error) => {
                        eprintln!(
                            "[ade-bridge] request authorization: status read after timeout failed: {error}"
                        );
                        Ok(NotificationAuthorizationResult {
                            status: NotificationAuthorizationStatus::NotDetermined,
                            available: true,
                        })
                    }
                };
            }
        };
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn bundled_executable_path_detection_is_pure() {
            assert!(!is_bundled_executable_path(std::path::Path::new(
                "/tmp/target/debug/orcinus-app"
            )));
            assert!(is_bundled_executable_path(std::path::Path::new(
                "/Applications/Orcinus.app/Contents/MacOS/orcinus-app"
            )));
        }

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
}
