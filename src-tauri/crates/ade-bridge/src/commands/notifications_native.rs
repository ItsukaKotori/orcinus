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
