//! `computerUsePermissions` command surface (spec §3.4): TCC status, setup
//! window and reset over the app's [`ComputerUsePermissionsService`].
//!
//! The command path records no feature interaction: the renderer's `openSetup`
//! already records `computer-use-setup` itself, and RPC-originated setup calls
//! are recorded inside the runtime server (spec §3.5). DTOs mirror
//! `src/shared/computer-use-permissions-types.ts` (camelCase; optional fields
//! omitted, never materialized as null).

use std::sync::Arc;

use ade_computer::permissions::{
    OpenSetupResult as CoreOpenSetupResult, PermissionId as CorePermissionId,
    PermissionState as CorePermissionState, PermissionStatus as CorePermissionStatus,
    PermissionStatusEntry as CorePermissionStatusEntry, ResetResult as CoreResetResult,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::run_blocking;
use crate::computer_service::ComputerUsePermissionsService;
use crate::errors::BridgeError;
use crate::state::AppState;

/// `ComputerUsePermissionId` (`'accessibility' | 'screenshots'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ComputerUsePermissionId {
    Accessibility,
    Screenshots,
}

/// `ComputerUsePermissionStatus` (`'granted' | 'not-granted' | 'unsupported'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum ComputerUsePermissionStatus {
    #[serde(rename = "granted")]
    Granted,
    #[serde(rename = "not-granted")]
    NotGranted,
    #[serde(rename = "unsupported")]
    Unsupported,
}

/// `ComputerUsePermissionState` (`{id, status}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUsePermissionState {
    pub id: ComputerUsePermissionId,
    pub status: ComputerUsePermissionStatus,
}

/// `ComputerUsePermissionStatusResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUsePermissionStatusResult {
    pub platform: String,
    pub helper_app_path: Option<String>,
    pub helper_unavailable_reason: Option<String>,
    pub permissions: Vec<ComputerUsePermissionState>,
}

/// `ComputerUsePermissionSetupResult` (`permissionId`/`permissions` omitted
/// when absent; `nextStep` stays nullable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseOpenSetupResult {
    pub platform: String,
    pub helper_app_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_id: Option<ComputerUsePermissionId>,
    pub opened_settings: bool,
    pub launched_helper: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<ComputerUsePermissionState>>,
    pub next_step: Option<String>,
}

/// `ComputerUsePermissionResetResult` (status plus the reset `bundleId`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseResetResult {
    pub platform: String,
    pub helper_app_path: Option<String>,
    pub helper_unavailable_reason: Option<String>,
    pub permissions: Vec<ComputerUsePermissionState>,
    pub bundle_id: Option<String>,
}

/// `computerUsePermissions.openSetup` args (`{ id? }`).
#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseOpenSetupArgs {
    #[serde(default)]
    pub id: Option<ComputerUsePermissionId>,
}

impl From<CorePermissionId> for ComputerUsePermissionId {
    fn from(id: CorePermissionId) -> Self {
        match id {
            CorePermissionId::Accessibility => Self::Accessibility,
            CorePermissionId::Screenshots => Self::Screenshots,
        }
    }
}

impl From<ComputerUsePermissionId> for CorePermissionId {
    fn from(id: ComputerUsePermissionId) -> Self {
        match id {
            ComputerUsePermissionId::Accessibility => Self::Accessibility,
            ComputerUsePermissionId::Screenshots => Self::Screenshots,
        }
    }
}

impl From<CorePermissionState> for ComputerUsePermissionStatus {
    fn from(state: CorePermissionState) -> Self {
        match state {
            CorePermissionState::Granted => Self::Granted,
            CorePermissionState::NotGranted => Self::NotGranted,
            CorePermissionState::Unsupported => Self::Unsupported,
        }
    }
}

impl From<CorePermissionStatusEntry> for ComputerUsePermissionState {
    fn from(entry: CorePermissionStatusEntry) -> Self {
        Self {
            id: entry.id.into(),
            status: entry.status.into(),
        }
    }
}

impl From<CorePermissionStatus> for ComputerUsePermissionStatusResult {
    fn from(status: CorePermissionStatus) -> Self {
        Self {
            platform: status.platform,
            helper_app_path: status.helper_app_path,
            helper_unavailable_reason: status.helper_unavailable_reason,
            permissions: status.permissions.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<CoreOpenSetupResult> for ComputerUseOpenSetupResult {
    fn from(result: CoreOpenSetupResult) -> Self {
        Self {
            platform: result.platform,
            helper_app_path: result.helper_app_path,
            permission_id: result.permission_id.map(Into::into),
            opened_settings: result.opened_settings,
            launched_helper: result.launched_helper,
            permissions: result
                .permissions
                .map(|entries| entries.into_iter().map(Into::into).collect()),
            next_step: result.next_step,
        }
    }
}

impl From<CoreResetResult> for ComputerUseResetResult {
    fn from(result: CoreResetResult) -> Self {
        Self {
            platform: result.platform,
            helper_app_path: result.helper_app_path,
            helper_unavailable_reason: result.helper_unavailable_reason,
            permissions: result.permissions.into_iter().map(Into::into).collect(),
            bundle_id: result.bundle_id,
        }
    }
}

/// TCC status probe (launches the helper status window, polls up to 5s).
pub fn get_status_impl(
    permissions: &ComputerUsePermissionsService,
) -> ComputerUsePermissionStatusResult {
    permissions.status_impl().into()
}

/// Open the helper setup window (and the system-settings pane for `id`).
pub fn open_setup_impl(
    permissions: &ComputerUsePermissionsService,
    args: Option<ComputerUseOpenSetupArgs>,
) -> ComputerUseOpenSetupResult {
    let id = args.and_then(|args| args.id).map(CorePermissionId::from);
    permissions.open_setup_impl(id).into()
}

/// Reset both TCC services for the helper bundle id (best effort; the result
/// only reflects the refreshed status and the bundle id used).
pub fn reset_impl(permissions: &ComputerUsePermissionsService) -> ComputerUseResetResult {
    permissions.reset_impl().into()
}

#[tauri::command]
#[specta::specta]
pub async fn computer_use_permissions_get_status(
    state: State<'_, AppState>,
) -> Result<ComputerUsePermissionStatusResult, BridgeError> {
    let permissions = Arc::clone(&state.computer.permissions);
    run_blocking(move || Ok(get_status_impl(&permissions))).await
}

#[tauri::command]
#[specta::specta]
pub async fn computer_use_permissions_open_setup(
    state: State<'_, AppState>,
    args: Option<ComputerUseOpenSetupArgs>,
) -> Result<ComputerUseOpenSetupResult, BridgeError> {
    let permissions = Arc::clone(&state.computer.permissions);
    run_blocking(move || Ok(open_setup_impl(&permissions, args))).await
}

#[tauri::command]
#[specta::specta]
pub async fn computer_use_permissions_reset(
    state: State<'_, AppState>,
) -> Result<ComputerUseResetResult, BridgeError> {
    let permissions = Arc::clone(&state.computer.permissions);
    run_blocking(move || Ok(reset_impl(&permissions))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn open_setup_args_deserialize_camel_case_id() {
        let args: ComputerUseOpenSetupArgs =
            serde_json::from_value(json!({ "id": "screenshots" })).unwrap();
        assert_eq!(args.id, Some(ComputerUsePermissionId::Screenshots));
        let empty: ComputerUseOpenSetupArgs = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty.id, None);
    }

    #[test]
    fn status_result_materializes_nullable_fields_and_keeps_the_union_literals() {
        let dto = get_status_impl(&ComputerUsePermissionsService::new(None));
        let value = serde_json::to_value(&dto).unwrap();
        assert_eq!(value["platform"], "darwin");
        assert!(value["helperAppPath"].is_null());
        assert!(value["helperUnavailableReason"].is_string());
        assert_eq!(value["permissions"][0]["id"], "accessibility");
        assert_eq!(value["permissions"][0]["status"], "not-granted");
        assert_eq!(value["permissions"][1]["id"], "screenshots");
    }

    #[test]
    fn open_setup_result_omits_absent_optionals_but_keeps_next_step_null() {
        let setup = open_setup_impl(&ComputerUsePermissionsService::new(None), None);
        let value = serde_json::to_value(&setup).unwrap();
        assert!(value.get("permissionId").is_none());
        assert!(value.get("permissions").is_none());
        assert_eq!(value["openedSettings"], false);
        assert_eq!(value["launchedHelper"], false);
        assert!(value["nextStep"].is_string());
    }

    #[test]
    fn open_setup_result_echoes_a_requested_permission_id() {
        let setup = open_setup_impl(
            &ComputerUsePermissionsService::new(None),
            Some(ComputerUseOpenSetupArgs {
                id: Some(ComputerUsePermissionId::Accessibility),
            }),
        );
        let value = serde_json::to_value(&setup).unwrap();
        assert_eq!(value["permissionId"], "accessibility");
    }

    #[test]
    fn reset_result_carries_the_bundle_id_and_status_shape() {
        struct NoopRunner;
        impl ade_computer::permissions::CommandRunner for NoopRunner {
            fn run(
                &self,
                _program: &str,
                _args: &[String],
            ) -> std::io::Result<ade_computer::permissions::CommandOutput> {
                Ok(ade_computer::permissions::CommandOutput {
                    status: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        }
        let service = ComputerUsePermissionsService::with_runner(None, Box::new(NoopRunner));
        let value = serde_json::to_value(reset_impl(&service)).unwrap();
        assert_eq!(value["bundleId"], "dev.itsuka.orcinus.computer-use");
        assert!(value["helperUnavailableReason"].is_string());
        assert_eq!(value["permissions"].as_array().unwrap().len(), 2);
    }
}
