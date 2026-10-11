//! TCC permission probe/setup/reset (spec §3.3). Mirrors
//! `orca:src/main/computer/macos-computer-use-permission-status.ts` and
//! `orca:src/main/computer/macos-computer-use-permissions.ts` without ever
//! touching the real helper: every external command goes through
//! [`CommandRunner`], and the result shapes serialize exactly like
//! `src/shared/computer-use-permissions-types.ts` (camelCase, nullable
//! `helperAppPath`/`nextStep`, omitted optional `permissionId`/`permissions`).

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths::{
    helper_bundle_id, helper_executable, DEFAULT_HELPER_BUNDLE_ID, HELPER_APP_NAME,
    HELPER_EXECUTABLE_NAME,
};

/// Status-poll budget used by [`probe_status`] (50 attempts × 100ms).
pub const STATUS_POLL_ATTEMPTS: u32 = 50;
/// Delay between status-file reads used by [`probe_status`].
pub const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(100);

const OPEN_PROGRAM: &str = "/usr/bin/open";
const PKILL_PROGRAM: &str = "/usr/bin/pkill";
const TCCUTIL_PROGRAM: &str = "/usr/bin/tccutil";
const STATUS_FILE_NAME: &str = "status.json";
const STATUS_DIR_PREFIX: &str = "orcinus-computer-use-permissions-";
const STATUS_DIR_ATTEMPTS: u32 = 64;
const STATUS_DIR_SUFFIX_LEN: usize = 6;
const TCCUTIL_SERVICES: [&str; 2] = ["Accessibility", "ScreenCapture"];
// Why: status probes use `--permission-status-file` and must not be killed
// while setup helpers are being replaced, so `--permission` only matches when
// followed by whitespace or end-of-line (same predicates as the oracle).
const PKILL_SETUP_PERMISSION: &str =
    "orcinus-computer-use-macos[[:space:]]+--permission([[:space:]]|$)";
const PKILL_SETUP_PERMISSIONS: &str =
    "orcinus-computer-use-macos[[:space:]]+--permissions([[:space:]]|$)";

/// TCC permission identifier (shared contract `accessibility|screenshots`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionId {
    Accessibility,
    Screenshots,
}

/// TCC permission state (shared contract `granted|not-granted|unsupported`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionState {
    #[serde(rename = "granted")]
    Granted,
    #[serde(rename = "not-granted")]
    NotGranted,
    #[serde(rename = "unsupported")]
    Unsupported,
}

/// One `{id, status}` permission pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatusEntry {
    pub id: PermissionId,
    pub status: PermissionState,
}

/// `ComputerUsePermissionStatusResult` (spec §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub platform: String,
    pub helper_app_path: Option<String>,
    pub helper_unavailable_reason: Option<String>,
    pub permissions: Vec<PermissionStatusEntry>,
}

/// `ComputerUsePermissionSetupResult` (spec §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSetupResult {
    pub platform: String,
    pub helper_app_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_id: Option<PermissionId>,
    pub opened_settings: bool,
    pub launched_helper: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<PermissionStatusEntry>>,
    pub next_step: Option<String>,
}

/// `ComputerUsePermissionResetResult`: status plus the reset `bundleId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetResult {
    pub platform: String,
    pub helper_app_path: Option<String>,
    pub helper_unavailable_reason: Option<String>,
    pub permissions: Vec<PermissionStatusEntry>,
    pub bundle_id: Option<String>,
}

/// Captured output of one external command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Injection seam for `open`/`pkill`/`tccutil` (tests capture argv; production
/// uses [`SystemRunner`]).
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[String]) -> io::Result<CommandOutput>;
}

/// Real process runner (`std::process::Command`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> io::Result<CommandOutput> {
        let output = std::process::Command::new(program).args(args).output()?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Node-compatible platform string used in every result (`darwin` on macOS),
/// matching `process.platform` in the shared TypeScript contract.
pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Probe both TCC permissions through the helper app's own identity: write a
/// temporary status path, launch `open -n <app> --args
/// --permission-status-file <path>`, poll for the JSON payload (50 × 100ms),
/// and default every unreadable entry to `not-granted`.
pub fn probe_status(runner: &dyn CommandRunner, helper: Option<&Path>) -> PermissionStatus {
    probe_status_with_poll(runner, helper, STATUS_POLL_ATTEMPTS, STATUS_POLL_INTERVAL)
}

/// [`probe_status`] with an injectable poll budget (tests shorten it).
pub fn probe_status_with_poll(
    runner: &dyn CommandRunner,
    helper: Option<&Path>,
    attempts: u32,
    interval: Duration,
) -> PermissionStatus {
    let Some(app) = helper else {
        return unavailable_status(None, helper_missing_reason());
    };
    if !helper_executable(app).exists() {
        return unavailable_status(
            Some(app),
            format!(
                "{}/Contents/MacOS/{} was not found",
                app.display(),
                HELPER_EXECUTABLE_NAME
            ),
        );
    }
    let status_dir = match create_status_directory() {
        Ok(directory) => directory,
        Err(_) => return permission_status(app, None),
    };
    let status_path = status_dir.join(STATUS_FILE_NAME);
    let payload = launch_and_read_status(runner, app, &status_path, attempts, interval);
    let _ = fs::remove_dir_all(&status_dir);
    permission_status(app, payload.as_ref())
}

/// Refresh the status, replace any stale setup windows, then open the helper
/// with `--permission <id>` (system-settings jump) or `--permissions` (helper
/// window only) and compute the oracle `nextStep` hint. Without a helper the
/// call is a no-op that reports the build/install hint.
pub fn open_setup(
    runner: &dyn CommandRunner,
    helper: Option<&Path>,
    permission_id: Option<PermissionId>,
) -> OpenSetupResult {
    let Some(app) = helper else {
        return OpenSetupResult {
            platform: platform().to_string(),
            helper_app_path: None,
            permission_id,
            opened_settings: false,
            launched_helper: false,
            permissions: None,
            next_step: Some(helper_missing_reason()),
        };
    };

    let status = probe_status(runner, helper);
    if let Some(reason) = status.helper_unavailable_reason {
        return OpenSetupResult {
            platform: status.platform,
            helper_app_path: status.helper_app_path,
            permission_id,
            opened_settings: false,
            launched_helper: false,
            permissions: None,
            next_step: Some(reason),
        };
    }

    let next_step = next_permission_step(&status.permissions);
    if permission_id.is_none() && next_step.is_none() {
        return OpenSetupResult {
            platform: status.platform,
            helper_app_path: status.helper_app_path,
            permission_id: None,
            opened_settings: false,
            launched_helper: false,
            permissions: Some(status.permissions),
            next_step: None,
        };
    }

    close_existing_setup_helpers(runner);
    let permission_args: Vec<&str> = match permission_id {
        Some(PermissionId::Accessibility) => vec!["--permission", "accessibility"],
        Some(PermissionId::Screenshots) => vec!["--permission", "screenshots"],
        None => vec!["--permissions"],
    };
    let launched_helper = matches!(
        runner.run(OPEN_PROGRAM, &launch_args(app, &permission_args)),
        Ok(output) if output.status == 0
    );

    OpenSetupResult {
        platform: status.platform,
        helper_app_path: status.helper_app_path,
        permission_id,
        opened_settings: permission_id.is_some(),
        launched_helper,
        permissions: Some(status.permissions),
        next_step,
    }
}

/// Reset both TCC services for the helper's bundle id (read from the vendored
/// Info.plist, falling back to the default id) and return the refreshed
/// status. Runs even when the helper is missing so stale rows can be cleared.
pub fn reset(runner: &dyn CommandRunner, helper: Option<&Path>) -> ResetResult {
    let bundle_id = helper
        .and_then(helper_bundle_id)
        .unwrap_or_else(|| DEFAULT_HELPER_BUNDLE_ID.to_string());
    for service in TCCUTIL_SERVICES {
        let args = vec!["reset".to_string(), service.to_string(), bundle_id.clone()];
        let _ = runner.run(TCCUTIL_PROGRAM, &args);
    }
    let status = probe_status(runner, helper);
    ResetResult {
        platform: status.platform,
        helper_app_path: status.helper_app_path,
        helper_unavailable_reason: status.helper_unavailable_reason,
        permissions: status.permissions,
        bundle_id: Some(bundle_id),
    }
}

/// Unavailable-helper copy from
/// `orca:src/main/computer/computer-provider-unavailable-message.ts` with Orca
/// renamed to Orcinus.
fn helper_missing_reason() -> String {
    match platform() {
        "darwin" => format!(
            "computer-use has no native provider for darwin because {HELPER_APP_NAME} was not found or this macOS version is unsupported. For local development, run pnpm build:computer-macos and restart Orcinus from this worktree."
        ),
        "linux" | "win32" => {
            format!("computer-use has no native provider for {}; the platform runtime file was not found", platform())
        }
        other => format!("computer-use has no native provider for {other}"),
    }
}

/// Oracle `nextPermissionStep`: first non-granted entry drives the hint.
fn next_permission_step(permissions: &[PermissionStatusEntry]) -> Option<String> {
    let missing = permissions
        .iter()
        .find(|entry| entry.status != PermissionState::Granted)?;
    let service = match missing.id {
        PermissionId::Accessibility => "Accessibility",
        PermissionId::Screenshots => "Screen Recording",
    };
    Some(format!(
        "Grant {service} to Orcinus Computer Use, then retry get-app-state."
    ))
}

fn unavailable_status(app: Option<&Path>, reason: String) -> PermissionStatus {
    PermissionStatus {
        platform: platform().to_string(),
        helper_app_path: app.map(|path| path.to_string_lossy().into_owned()),
        helper_unavailable_reason: Some(reason),
        permissions: vec![
            PermissionStatusEntry {
                id: PermissionId::Accessibility,
                status: PermissionState::NotGranted,
            },
            PermissionStatusEntry {
                id: PermissionId::Screenshots,
                status: PermissionState::NotGranted,
            },
        ],
    }
}

fn permission_status(app: &Path, payload: Option<&Value>) -> PermissionStatus {
    PermissionStatus {
        platform: platform().to_string(),
        helper_app_path: Some(app.to_string_lossy().into_owned()),
        helper_unavailable_reason: None,
        permissions: vec![
            permission_entry(
                PermissionId::Accessibility,
                payload.and_then(|value| value.get("accessibility")),
            ),
            permission_entry(
                PermissionId::Screenshots,
                payload.and_then(|value| value.get("screenshots")),
            ),
        ],
    }
}

fn permission_entry(id: PermissionId, value: Option<&Value>) -> PermissionStatusEntry {
    let status = match value.and_then(Value::as_str) {
        Some("granted") => PermissionState::Granted,
        _ => PermissionState::NotGranted,
    };
    PermissionStatusEntry { id, status }
}

fn launch_and_read_status(
    runner: &dyn CommandRunner,
    app: &Path,
    status_path: &Path,
    attempts: u32,
    interval: Duration,
) -> Option<Value> {
    let status_path_text = status_path.to_string_lossy().into_owned();
    let args = launch_args(
        app,
        &["--permission-status-file", status_path_text.as_str()],
    );
    match runner.run(OPEN_PROGRAM, &args) {
        Ok(output) if output.status == 0 => {}
        _ => return None,
    }
    for _ in 0..attempts {
        if let Ok(raw) = fs::read_to_string(status_path_text.as_str()) {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                return Some(value);
            }
        }
        if interval > Duration::ZERO {
            thread::sleep(interval);
        }
    }
    None
}

fn launch_args(app: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "-n".to_string(),
        app.to_string_lossy().into_owned(),
        "--args".to_string(),
    ];
    args.extend(extra.iter().map(|arg| (*arg).to_string()));
    args
}

fn close_existing_setup_helpers(runner: &dyn CommandRunner) {
    for pattern in [PKILL_SETUP_PERMISSION, PKILL_SETUP_PERMISSIONS] {
        let args = vec!["-f".to_string(), pattern.to_string()];
        let _ = runner.run(PKILL_PROGRAM, &args);
    }
}

fn create_status_directory() -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..STATUS_DIR_ATTEMPTS {
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(STATUS_DIR_SUFFIX_LEN)
            .map(char::from)
            .collect();
        let path = base.join(format!("{STATUS_DIR_PREFIX}{suffix}"));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique computer-use permission status directory",
    ))
}
