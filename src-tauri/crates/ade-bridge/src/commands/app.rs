use serde::Serialize;

use crate::errors::BridgeError;

/// Superset of the renderer `AppIdentity` contract: the native identity also
/// carries the crate version (`docs/.../2026-09-23-phase1a-open-project-design.md` §5.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentityInfo {
    pub name: String,
    pub version: String,
    pub is_dev: bool,
    pub dev_label: Option<String>,
    pub dev_branch: Option<String>,
    pub dev_worktree_name: Option<String>,
    pub dev_repo_root: Option<String>,
    pub dock_badge_label: Option<String>,
}

pub fn app_identity() -> AppIdentityInfo {
    AppIdentityInfo {
        name: "Orcinus".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        is_dev: cfg!(debug_assertions),
        dev_label: None,
        dev_branch: None,
        dev_worktree_name: None,
        dev_repo_root: None,
        dock_badge_label: None,
    }
}

#[tauri::command]
#[specta::specta]
pub fn app_get_identity() -> Result<AppIdentityInfo, BridgeError> {
    Ok(app_identity())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_reports_orcinus_and_crate_version() {
        let identity = app_identity();
        assert_eq!(identity.name, "Orcinus");
        assert_eq!(identity.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn identity_serializes_camel_case_with_null_optionals() {
        let value = serde_json::to_value(app_identity()).unwrap();
        assert_eq!(value["name"], "Orcinus");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert!(value["devLabel"].is_null());
        assert!(value["dockBadgeLabel"].is_null());
    }
}
