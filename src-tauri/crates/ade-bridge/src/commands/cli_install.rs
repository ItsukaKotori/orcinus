//! `cli` command surface (spec §3.8): PATH install/status/remove for the
//! `orcinus` launcher (symlink), plus the WSL command triple that always
//! reports `unsupported`/`platform_not_supported` on this host.
//!
//! Simplified from oracle `src/main/cli/`: no admin prompt (osascript), no
//! shell-profile edits, no Windows branch. On macOS the install target is
//! `/usr/local/bin/orcinus` when that directory exists and is writable,
//! otherwise `~/.local/bin/orcinus`; `ORCINUS_CLI_INSTALL_PATH` overrides it.
//!
//! The DTO mirrors `src/shared/cli-install-types.ts` field for field; every
//! nullable field is materialized as `null` when absent (never omitted).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::commands::platform::map_platform;
use crate::errors::BridgeError;

/// The managed shell command name.
pub const COMMAND_NAME: &str = "orcinus";
/// The system install directory tried first on macOS.
pub const SYSTEM_INSTALL_DIR: &str = "/usr/local/bin";
/// Full-path install target override (spec §3.8).
pub const INSTALL_PATH_ENV: &str = "ORCINUS_CLI_INSTALL_PATH";
/// `detail` carried by every `platform_not_supported` result.
pub const UNSUPPORTED_DETAIL: &str = "CLI registration is not implemented on this platform.";

/// `CliInstallState` union (`'installed' | 'not_installed' | 'stale' |
/// 'conflict' | 'unsupported'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CliInstallState {
    Installed,
    NotInstalled,
    Stale,
    Conflict,
    Unsupported,
}

/// `CliInstallUnsupportedReason` union.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CliInstallUnsupportedReason {
    PlatformNotSupported,
    LauncherMissing,
    LaunchModeUnavailable,
}

/// `CliInstallMethod` union. This slice only ever installs `symlink`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CliInstallMethod {
    Symlink,
    Wrapper,
}

/// `CliInstallStatus` (`src/shared/cli-install-types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CliInstallStatusDto {
    pub platform: String,
    pub command_name: String,
    pub command_path: Option<String>,
    pub path_directory: Option<String>,
    /// `None` only when the persisted PATH could not be read.
    pub path_configured: Option<bool>,
    pub launcher_path: Option<String>,
    pub install_method: Option<CliInstallMethod>,
    pub supported: bool,
    pub state: CliInstallState,
    pub current_target: Option<String>,
    pub unsupported_reason: Option<CliInstallUnsupportedReason>,
    pub detail: Option<String>,
}

/// `cli.*Wsl` args (`{ distro? }`). The distro is accepted for IPC shape
/// parity and ignored by the unsupported stub.
#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CliWslInstallArgs {
    #[serde(default)]
    pub distro: Option<String>,
}

/// The injectable inputs one status/install/remove call resolves from.
#[derive(Debug, Clone)]
pub struct CliInstallContext {
    /// Raw `std::env::consts::OS` value (`macos` on the supported host).
    pub platform: String,
    /// The launcher the managed symlink must point at.
    pub launcher_path: PathBuf,
    /// System install directory candidate (`/usr/local/bin` in production).
    pub system_dir: PathBuf,
    /// User home directory (fallback install root).
    pub home: PathBuf,
    /// `ORCINUS_CLI_INSTALL_PATH` override, when set.
    pub env_override: Option<PathBuf>,
    /// `$PATH` as read by the process; `None` means it could not be read.
    pub path_env: Option<String>,
}

impl CliInstallContext {
    /// Production context: launcher next to the current executable, real
    /// `$HOME`/`$PATH`, `ORCINUS_CLI_INSTALL_PATH` when set.
    pub fn from_process() -> Self {
        Self {
            platform: std::env::consts::OS.to_string(),
            launcher_path: resolve_cli_launcher(),
            system_dir: PathBuf::from(SYSTEM_INSTALL_DIR),
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default(),
            env_override: std::env::var_os(INSTALL_PATH_ENV).map(PathBuf::from),
            path_env: std::env::var("PATH").ok(),
        }
    }
}

/// `current_exe().parent()/orcinus` (spec §3.8).
pub fn resolve_cli_launcher() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join(COMMAND_NAME)))
        .unwrap_or_else(|| PathBuf::from(COMMAND_NAME))
}

/// `override` → `/usr/local/bin/orcinus` when that directory exists and is
/// writable (`access` W_OK) → `~/.local/bin/orcinus`. No admin prompt.
pub fn install_target_path(system_dir: &Path, home: &Path, env_override: Option<&Path>) -> PathBuf {
    if let Some(override_path) = env_override {
        return override_path.to_path_buf();
    }
    if system_dir.is_dir() && directory_is_writable(system_dir) {
        return system_dir.join(COMMAND_NAME);
    }
    home.join(".local").join("bin").join(COMMAND_NAME)
}

#[cfg(unix)]
fn directory_is_writable(directory: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = CString::new(directory.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a NUL-terminated copy of the directory path and the
    // value is only probed for the calling user (no ownership transfer).
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn directory_is_writable(_directory: &Path) -> bool {
    false
}

/// `$PATH` contains `directory` (POSIX `:` split, trimmed, empty entries
/// skipped). A `None` path env stays `None`: callers must not offer mutations.
fn path_configured(path_env: Option<&str>, directory: &Path) -> Option<bool> {
    let value = path_env?;
    let directory = directory.to_string_lossy();
    Some(
        value
            .split(':')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .any(|entry| entry == directory),
    )
}

/// Current install status for the context's target (spec §3.8).
pub fn status_impl(context: &CliInstallContext) -> CliInstallStatusDto {
    if context.platform != "macos" {
        return unsupported_status(map_platform(&context.platform));
    }
    let target = install_target_path(
        &context.system_dir,
        &context.home,
        context.env_override.as_deref(),
    );
    let path_directory = target.parent().map(Path::to_path_buf);
    let configured = match path_directory.as_deref() {
        Some(directory) => path_configured(context.path_env.as_deref(), directory),
        None => Some(false),
    };
    let (state, current_target, detail) = inspect_target(&target, &context.launcher_path);
    CliInstallStatusDto {
        platform: map_platform(&context.platform).to_string(),
        command_name: COMMAND_NAME.to_string(),
        command_path: Some(target.to_string_lossy().into_owned()),
        path_directory: path_directory.map(|directory| directory.to_string_lossy().into_owned()),
        path_configured: configured,
        launcher_path: Some(context.launcher_path.to_string_lossy().into_owned()),
        install_method: Some(CliInstallMethod::Symlink),
        supported: true,
        state,
        current_target,
        unsupported_reason: None,
        detail: Some(detail),
    }
}

/// Create the symlink (repairing a stale one); refuse to overwrite anything
/// that is not ours. Idempotent when already `installed`.
pub fn install_impl(context: &CliInstallContext) -> Result<CliInstallStatusDto, BridgeError> {
    let status = status_impl(context);
    if !status.supported {
        return Ok(status);
    }
    if status.state == CliInstallState::Conflict {
        return Err(BridgeError::message(format!(
            "Refusing to replace non-{COMMAND_NAME} command at {}. Remove it and register again if it is no longer needed.",
            status.command_path.as_deref().unwrap_or_default(),
        )));
    }
    if status.state != CliInstallState::Installed {
        let target = PathBuf::from(status.command_path.as_deref().unwrap_or_default());
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if status.state == CliInstallState::Stale {
            std::fs::remove_file(&target)?;
        }
        std::os::unix::fs::symlink(&context.launcher_path, &target)?;
    }
    Ok(status_impl(context))
}

/// Delete the managed symlink after verifying it is ours; a foreign occupant
/// is refused, a missing target is a no-op.
pub fn remove_impl(context: &CliInstallContext) -> Result<CliInstallStatusDto, BridgeError> {
    let status = status_impl(context);
    if !status.supported {
        return Ok(status);
    }
    match status.state {
        CliInstallState::Conflict => Err(BridgeError::message(format!(
            "Refusing to remove non-{COMMAND_NAME} command at {}.",
            status.command_path.as_deref().unwrap_or_default(),
        ))),
        CliInstallState::Installed | CliInstallState::Stale => {
            let target = PathBuf::from(status.command_path.as_deref().unwrap_or_default());
            std::fs::remove_file(&target)?;
            Ok(status_impl(context))
        }
        CliInstallState::NotInstalled | CliInstallState::Unsupported => Ok(status),
    }
}

/// The `platform_not_supported` shape shared by non-macOS hosts and the WSL
/// stubs, mirroring oracle's `CliInstaller.getStatus` early return.
pub fn unsupported_status(platform: &str) -> CliInstallStatusDto {
    CliInstallStatusDto {
        platform: platform.to_string(),
        command_name: COMMAND_NAME.to_string(),
        command_path: None,
        path_directory: None,
        path_configured: Some(false),
        launcher_path: None,
        install_method: None,
        supported: false,
        state: CliInstallState::Unsupported,
        current_target: None,
        unsupported_reason: Some(CliInstallUnsupportedReason::PlatformNotSupported),
        detail: Some(UNSUPPORTED_DETAIL.to_string()),
    }
}

/// `cli_*_wsl` answer: this host has no WSL launcher story, so the triple
/// always returns the unsupported shape.
pub fn wsl_status_impl(platform: &str) -> CliInstallStatusDto {
    unsupported_status(map_platform(platform))
}

/// `installed` when the symlink resolves to the canonical launcher, `stale`
/// when it resolves to another `orcinus` launcher, `conflict` for every other
/// occupant (never overwritten by `install`).
fn inspect_target(target: &Path, launcher: &Path) -> (CliInstallState, Option<String>, String) {
    let Ok(metadata) = std::fs::symlink_metadata(target) else {
        return (
            CliInstallState::NotInstalled,
            None,
            format!(
                "Register {} to use {COMMAND_NAME} from the terminal.",
                target.display()
            ),
        );
    };
    if !metadata.file_type().is_symlink() {
        return (
            CliInstallState::Conflict,
            None,
            format!(
                "{} exists but is not an {COMMAND_NAME} symlink.",
                target.display()
            ),
        );
    }
    let resolved = std::fs::read_link(target)
        .ok()
        .map(|raw| resolve_link_target(target, &raw))
        .unwrap_or_else(|| target.to_path_buf());
    let current_target = resolved.to_string_lossy().into_owned();
    if canonical_or_self(&resolved) == canonical_or_self(launcher) {
        return (
            CliInstallState::Installed,
            Some(current_target),
            format!("Registered at {}.", target.display()),
        );
    }
    if resolved.file_name() == launcher.file_name() {
        return (
            CliInstallState::Stale,
            Some(current_target),
            format!(
                "{} points to an older {COMMAND_NAME} launcher.",
                target.display()
            ),
        );
    }
    (
        CliInstallState::Conflict,
        Some(current_target),
        format!(
            "{} points to a non-{COMMAND_NAME} launcher.",
            target.display()
        ),
    )
}

fn resolve_link_target(command_path: &Path, raw: &Path) -> PathBuf {
    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    command_path
        .parent()
        .map(|parent| parent.join(raw))
        .unwrap_or_else(|| raw.to_path_buf())
}

fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[tauri::command]
#[specta::specta]
pub fn cli_get_install_status() -> Result<CliInstallStatusDto, BridgeError> {
    Ok(status_impl(&CliInstallContext::from_process()))
}

#[tauri::command]
#[specta::specta]
pub fn cli_install() -> Result<CliInstallStatusDto, BridgeError> {
    install_impl(&CliInstallContext::from_process())
}

#[tauri::command]
#[specta::specta]
pub fn cli_remove() -> Result<CliInstallStatusDto, BridgeError> {
    remove_impl(&CliInstallContext::from_process())
}

#[tauri::command]
#[specta::specta]
pub fn cli_get_wsl_install_status(
    args: Option<CliWslInstallArgs>,
) -> Result<CliInstallStatusDto, BridgeError> {
    let _ = args;
    Ok(wsl_status_impl(std::env::consts::OS))
}

#[tauri::command]
#[specta::specta]
pub fn cli_install_wsl(
    args: Option<CliWslInstallArgs>,
) -> Result<CliInstallStatusDto, BridgeError> {
    let _ = args;
    Ok(wsl_status_impl(std::env::consts::OS))
}

#[tauri::command]
#[specta::specta]
pub fn cli_remove_wsl(args: Option<CliWslInstallArgs>) -> Result<CliInstallStatusDto, BridgeError> {
    let _ = args;
    Ok(wsl_status_impl(std::env::consts::OS))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};

    use super::*;

    struct Fixture {
        root: tempfile::TempDir,
        launcher: PathBuf,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let launcher = root.path().join("launcher").join(COMMAND_NAME);
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        fs::write(&launcher, b"#!/bin/sh\n").unwrap();
        Fixture { root, launcher }
    }

    fn context(fixture: &Fixture) -> CliInstallContext {
        CliInstallContext {
            platform: "macos".to_string(),
            launcher_path: fixture.launcher.clone(),
            system_dir: fixture.root.path().join("usr-local-bin"),
            home: fixture.root.path().join("home"),
            env_override: None,
            path_env: Some(String::new()),
        }
    }

    fn target_for(context: &CliInstallContext) -> PathBuf {
        install_target_path(
            &context.system_dir,
            &context.home,
            context.env_override.as_deref(),
        )
    }

    fn canonical(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    #[test]
    fn lifecycle_install_and_remove_round_trip() {
        let fixture = fixture();
        let context = context(&fixture);
        let target = target_for(&context);
        assert_eq!(
            target,
            fixture
                .root
                .path()
                .join("home")
                .join(".local/bin")
                .join(COMMAND_NAME)
        );

        let status = status_impl(&context);
        assert!(status.supported);
        assert_eq!(status.state, CliInstallState::NotInstalled);
        assert_eq!(status.install_method, Some(CliInstallMethod::Symlink));
        assert_eq!(status.command_path.as_deref(), target.to_str());
        assert_eq!(status.launcher_path.as_deref(), fixture.launcher.to_str());
        assert!(status.current_target.is_none());
        assert_eq!(
            status.path_directory.as_deref(),
            target.parent().unwrap().to_str()
        );

        let installed = install_impl(&context).unwrap();
        assert_eq!(installed.state, CliInstallState::Installed);
        assert_eq!(fs::read_link(&target).unwrap(), fixture.launcher);
        assert_eq!(
            canonical(Path::new(installed.current_target.as_deref().unwrap())),
            canonical(&fixture.launcher)
        );

        // Idempotent: a second install keeps the link and reports installed.
        let re_installed = install_impl(&context).unwrap();
        assert_eq!(re_installed.state, CliInstallState::Installed);
        assert_eq!(fs::read_link(&target).unwrap(), fixture.launcher);

        let removed = remove_impl(&context).unwrap();
        assert_eq!(removed.state, CliInstallState::NotInstalled);
        assert!(!target.exists());
    }

    #[test]
    fn foreign_file_occupant_conflicts_and_install_refuses_to_replace_it() {
        let fixture = fixture();
        let context = context(&fixture);
        let target = target_for(&context);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"#!/bin/sh\nforeign\n").unwrap();

        let status = status_impl(&context);
        assert_eq!(status.state, CliInstallState::Conflict);
        assert!(status
            .detail
            .as_deref()
            .unwrap()
            .contains("not an orcinus symlink"));
        assert!(status.current_target.is_none());

        let error = install_impl(&context).unwrap_err();
        assert!(error
            .to_string()
            .contains("Refusing to replace non-orcinus command"));
        assert_eq!(fs::read_to_string(&target).unwrap(), "#!/bin/sh\nforeign\n");
    }

    #[test]
    fn symlink_to_foreign_program_conflicts_and_remove_refuses() {
        let fixture = fixture();
        let context = context(&fixture);
        let target = target_for(&context);
        let foreign = fixture.root.path().join("other-tool");
        fs::write(&foreign, b"").unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        symlink(&foreign, &target).unwrap();

        let status = status_impl(&context);
        assert_eq!(status.state, CliInstallState::Conflict);
        assert_eq!(
            canonical(Path::new(status.current_target.as_deref().unwrap())),
            canonical(&foreign)
        );
        assert!(install_impl(&context).is_err());

        let error = remove_impl(&context).unwrap_err();
        assert!(error
            .to_string()
            .contains("Refusing to remove non-orcinus command"));
        assert!(target.symlink_metadata().unwrap().file_type().is_symlink());
    }

    #[test]
    fn symlink_to_older_orcinus_launcher_is_stale_and_install_repairs_it() {
        let fixture = fixture();
        let context = context(&fixture);
        let target = target_for(&context);
        let old_launcher = fixture.root.path().join("old-build").join(COMMAND_NAME);
        fs::create_dir_all(old_launcher.parent().unwrap()).unwrap();
        fs::write(&old_launcher, b"#!/bin/sh\n").unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        symlink(&old_launcher, &target).unwrap();

        let status = status_impl(&context);
        assert_eq!(status.state, CliInstallState::Stale);
        assert!(status
            .detail
            .as_deref()
            .unwrap()
            .contains("older orcinus launcher"));
        assert_eq!(
            canonical(Path::new(status.current_target.as_deref().unwrap())),
            canonical(&old_launcher)
        );

        let repaired = install_impl(&context).unwrap();
        assert_eq!(repaired.state, CliInstallState::Installed);
        assert_eq!(fs::read_link(&target).unwrap(), fixture.launcher);

        let removed = remove_impl(&context).unwrap();
        assert_eq!(removed.state, CliInstallState::NotInstalled);
    }

    #[test]
    fn install_target_prefers_a_writable_system_directory_and_falls_back_to_home() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");

        let missing = root.path().join("absent-bin");
        assert_eq!(
            install_target_path(&missing, &home, None),
            home.join(".local/bin").join(COMMAND_NAME)
        );

        let read_only = root.path().join("read-only-bin");
        fs::create_dir_all(&read_only).unwrap();
        fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).unwrap();
        assert_eq!(
            install_target_path(&read_only, &home, None),
            home.join(".local/bin").join(COMMAND_NAME)
        );

        fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            install_target_path(&read_only, &home, None),
            read_only.join(COMMAND_NAME)
        );

        let override_path = root.path().join("custom").join(COMMAND_NAME);
        assert_eq!(
            install_target_path(&read_only, &home, Some(&override_path)),
            override_path
        );
    }

    #[test]
    fn status_reports_path_configured_from_the_injected_path() {
        let fixture = fixture();
        let mut context = context(&fixture);
        let directory = target_for(&context).parent().unwrap().to_path_buf();

        context.path_env = Some(format!("/usr/bin:{}:/opt/bin", directory.display()));
        assert_eq!(status_impl(&context).path_configured, Some(true));

        context.path_env = Some("/usr/bin:/opt/bin".to_string());
        assert_eq!(status_impl(&context).path_configured, Some(false));

        context.path_env = None;
        assert_eq!(status_impl(&context).path_configured, None);
    }

    #[test]
    fn env_override_redirects_status_and_install() {
        let fixture = fixture();
        let mut context = context(&fixture);
        let override_path = fixture.root.path().join("custom-bin").join(COMMAND_NAME);
        context.env_override = Some(override_path.clone());

        let status = status_impl(&context);
        assert_eq!(status.state, CliInstallState::NotInstalled);
        assert_eq!(status.command_path.as_deref(), override_path.to_str());

        let installed = install_impl(&context).unwrap();
        assert_eq!(installed.state, CliInstallState::Installed);
        assert_eq!(fs::read_link(&override_path).unwrap(), fixture.launcher);
        assert_eq!(
            installed.path_directory.as_deref(),
            override_path.parent().unwrap().to_str()
        );
    }

    #[test]
    fn non_macos_desktop_reports_platform_not_supported() {
        let fixture = fixture();
        let mut context = context(&fixture);
        context.platform = "linux".to_string();

        let status = status_impl(&context);
        assert_eq!(status.platform, "linux");
        assert_eq!(status.state, CliInstallState::Unsupported);
        assert!(!status.supported);
        assert_eq!(
            status.unsupported_reason,
            Some(CliInstallUnsupportedReason::PlatformNotSupported)
        );
        assert!(status.command_path.is_none());
        assert!(status.path_directory.is_none());
        assert_eq!(status.path_configured, Some(false));
        assert!(status.launcher_path.is_none());
        assert!(status.install_method.is_none());
        assert!(status.current_target.is_none());
        assert_eq!(status.detail.as_deref(), Some(UNSUPPORTED_DETAIL));

        // Mutations on an unsupported platform are no-ops that echo the shape.
        assert_eq!(
            install_impl(&context).unwrap().state,
            CliInstallState::Unsupported
        );
        assert_eq!(
            remove_impl(&context).unwrap().state,
            CliInstallState::Unsupported
        );
    }

    #[test]
    fn wsl_status_shape_is_the_unsupported_shape_and_serializes_camel_case() {
        let value = serde_json::to_value(wsl_status_impl("darwin")).unwrap();
        assert_eq!(value["platform"], "darwin");
        assert_eq!(value["commandName"], COMMAND_NAME);
        assert!(value["commandPath"].is_null());
        assert!(value["pathDirectory"].is_null());
        assert_eq!(value["pathConfigured"], false);
        assert!(value["launcherPath"].is_null());
        assert!(value["installMethod"].is_null());
        assert_eq!(value["supported"], false);
        assert_eq!(value["state"], "unsupported");
        assert!(value["currentTarget"].is_null());
        assert_eq!(value["unsupportedReason"], "platform_not_supported");
        assert_eq!(value["detail"], UNSUPPORTED_DETAIL);
    }

    #[test]
    fn wsl_args_deserialize_distro_and_absence() {
        let args: CliWslInstallArgs =
            serde_json::from_value(serde_json::json!({ "distro": "Ubuntu" })).unwrap();
        assert_eq!(args.distro.as_deref(), Some("Ubuntu"));
        let empty: CliWslInstallArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(empty.distro.is_none());
    }

    #[test]
    fn supported_status_materializes_every_nullable_field() {
        let fixture = fixture();
        let context = context(&fixture);
        let value = serde_json::to_value(status_impl(&context)).unwrap();
        assert_eq!(value["platform"], "darwin");
        assert_eq!(value["commandName"], COMMAND_NAME);
        assert_eq!(value["installMethod"], "symlink");
        assert_eq!(value["supported"], true);
        assert_eq!(value["state"], "not_installed");
        assert_eq!(value["pathConfigured"], false);
        assert!(value["unsupportedReason"].is_null());
        assert!(value["detail"].is_string());
        assert!(value["commandPath"].is_string());
        assert!(value["pathDirectory"].is_string());
        assert!(value["launcherPath"].is_string());
        assert!(value["currentTarget"].is_null());
    }
}
