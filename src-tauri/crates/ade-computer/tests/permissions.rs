//! Permission anchors (task brief step 1): `open`/`pkill`/`tccutil` argv,
//! status-file mapping (and the missing-file default), the helper-missing
//! branches (probe/setup/reset), Info.plist bundle-id fallback, and the serde
//! shapes shared with `src/shared/computer-use-permissions-types.ts`.
//!
//! A fake [`CommandRunner`] captures argv and plants status-file fixtures; no
//! Swift helper and no real `open`/`pkill`/`tccutil` process is ever launched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use ade_computer::permissions::{
    open_setup, platform, probe_status, probe_status_with_poll, reset, CommandOutput,
    CommandRunner, OpenSetupResult, PermissionId, PermissionState, PermissionStatus,
    PermissionStatusEntry, ResetResult,
};
use serde_json::json;

const OPEN: &str = "/usr/bin/open";
const PKILL: &str = "/usr/bin/pkill";
const TCCUTIL: &str = "/usr/bin/tccutil";
const PKILL_PERMISSION: &str = "orcinus-computer-use-macos[[:space:]]+--permission([[:space:]]|$)";
const PKILL_PERMISSIONS: &str =
    "orcinus-computer-use-macos[[:space:]]+--permissions([[:space:]]|$)";
const DEFAULT_BUNDLE_ID: &str = "dev.itsuka.orcinus.computer-use";
const UNAVAILABLE_REASON: &str = "computer-use has no native provider for darwin because Orcinus Computer Use.app was not found or this macOS version is unsupported. For local development, run pnpm build:computer-macos and restart Orcinus from this worktree.";

#[derive(Debug, Clone)]
struct Call {
    program: String,
    args: Vec<String>,
}

#[derive(Default)]
struct FakeRunner {
    calls: Mutex<Vec<Call>>,
    status_payload: Mutex<Option<String>>,
}

impl FakeRunner {
    fn with_status_payload(payload: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            status_payload: Mutex::new(Some(payload.to_string())),
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[String]) -> io::Result<CommandOutput> {
        self.calls.lock().unwrap().push(Call {
            program: program.to_string(),
            args: args.to_vec(),
        });
        if program == OPEN {
            if let Some(payload) = self.status_payload.lock().unwrap().clone() {
                if let Some(position) = args
                    .iter()
                    .position(|arg| arg == "--permission-status-file")
                {
                    let path = args
                        .get(position + 1)
                        .expect("status file path follows flag");
                    fs::write(path, payload)?;
                }
            }
        }
        Ok(CommandOutput {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "ade-computer-permissions-{}-{}",
            std::process::id(),
            DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn helper_app(&self, bundle_id: Option<&str>) -> PathBuf {
        let app = self.path.join("Orcinus Computer Use.app");
        fs::create_dir_all(app.join("Contents/MacOS")).expect("create app bundle");
        fs::write(
            app.join("Contents/MacOS/orcinus-computer-use-macos"),
            "stub",
        )
        .expect("create helper executable");
        if let Some(bundle_id) = bundle_id {
            fs::write(
                app.join("Contents/Info.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <plist version=\"1.0\"><dict>\n\
                     \t<key>CFBundleIdentifier</key>\n\
                     \t<string>{bundle_id}</string>\n\
                     </dict></plist>\n"
                ),
            )
            .expect("write Info.plist");
        }
        app
    }

    fn app_without_executable(&self) -> PathBuf {
        let app = self.path.join("Orcinus Computer Use.app");
        fs::create_dir_all(app.join("Contents/MacOS")).expect("create app bundle");
        app
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn entry(id: PermissionId, status: PermissionState) -> PermissionStatusEntry {
    PermissionStatusEntry { id, status }
}

#[test]
fn probe_status_launches_helper_with_status_file_and_maps_payload() {
    let temp = TempDir::new();
    let app = temp.helper_app(None);
    let runner = FakeRunner::with_status_payload(
        r#"{"accessibility":"granted","screenshots":"not-granted"}"#,
    );

    let status = probe_status(&runner, Some(&app));

    assert_eq!(status.platform, platform());
    assert_eq!(status.helper_app_path.as_deref(), app.to_str());
    assert_eq!(status.helper_unavailable_reason, None);
    assert_eq!(
        status.permissions,
        vec![
            entry(PermissionId::Accessibility, PermissionState::Granted),
            entry(PermissionId::Screenshots, PermissionState::NotGranted),
        ]
    );

    let calls = runner.calls();
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].program, OPEN);
    assert_eq!(calls[0].args.len(), 5, "args: {:?}", calls[0].args);
    assert_eq!(calls[0].args[0], "-n");
    assert_eq!(calls[0].args[1], app.to_str().unwrap());
    assert_eq!(calls[0].args[2], "--args");
    assert_eq!(calls[0].args[3], "--permission-status-file");
    let status_path = Path::new(&calls[0].args[4]);
    assert_eq!(
        status_path.file_name().unwrap().to_string_lossy(),
        "status.json"
    );
    let status_dir = status_path.parent().expect("status file has a parent");
    assert!(
        status_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("orcinus-computer-use-permissions-"),
        "unexpected status dir: {}",
        status_dir.display()
    );
}

#[test]
fn probe_status_missing_status_file_defaults_both_permissions_to_not_granted() {
    let temp = TempDir::new();
    let app = temp.helper_app(None);
    let runner = FakeRunner::default();

    let status = probe_status_with_poll(&runner, Some(&app), 3, Duration::from_millis(1));

    assert_eq!(status.helper_unavailable_reason, None);
    assert_eq!(
        status.permissions,
        vec![
            entry(PermissionId::Accessibility, PermissionState::NotGranted),
            entry(PermissionId::Screenshots, PermissionState::NotGranted),
        ]
    );
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn probe_status_without_helper_reports_unavailable_without_running_commands() {
    let runner = FakeRunner::default();

    let status = probe_status(&runner, None);

    assert_eq!(status.platform, platform());
    assert_eq!(status.helper_app_path, None);
    assert_eq!(
        status.helper_unavailable_reason.as_deref(),
        Some(UNAVAILABLE_REASON)
    );
    assert_eq!(
        status.permissions,
        vec![
            entry(PermissionId::Accessibility, PermissionState::NotGranted),
            entry(PermissionId::Screenshots, PermissionState::NotGranted),
        ]
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn probe_status_missing_executable_reports_unavailable_reason() {
    let temp = TempDir::new();
    let app = temp.app_without_executable();
    let runner = FakeRunner::default();

    let status = probe_status(&runner, Some(&app));

    assert_eq!(status.helper_app_path.as_deref(), app.to_str());
    assert_eq!(
        status.helper_unavailable_reason.as_deref(),
        Some(
            format!(
                "{}/Contents/MacOS/orcinus-computer-use-macos was not found",
                app.display()
            )
            .as_str()
        )
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn open_setup_with_permission_id_refreshes_replaces_and_launches() {
    let temp = TempDir::new();
    let app = temp.helper_app(None);
    let runner = FakeRunner::with_status_payload(
        r#"{"accessibility":"not-granted","screenshots":"granted"}"#,
    );

    let result = open_setup(&runner, Some(&app), Some(PermissionId::Accessibility));

    assert_eq!(result.platform, platform());
    assert_eq!(result.helper_app_path.as_deref(), app.to_str());
    assert_eq!(result.permission_id, Some(PermissionId::Accessibility));
    assert!(result.opened_settings);
    assert!(result.launched_helper);
    assert_eq!(
        result.permissions,
        Some(vec![
            entry(PermissionId::Accessibility, PermissionState::NotGranted),
            entry(PermissionId::Screenshots, PermissionState::Granted),
        ])
    );
    assert_eq!(
        result.next_step.as_deref(),
        Some("Grant Accessibility to Orcinus Computer Use, then retry get-app-state.")
    );

    let calls = runner.calls();
    assert_eq!(calls.len(), 4, "calls: {calls:?}");
    assert_eq!(calls[0].program, OPEN);
    assert_eq!(calls[0].args[3], "--permission-status-file");
    assert_eq!(calls[1].program, PKILL);
    assert_eq!(calls[1].args, ["-f", PKILL_PERMISSION]);
    assert_eq!(calls[2].program, PKILL);
    assert_eq!(calls[2].args, ["-f", PKILL_PERMISSIONS]);
    assert_eq!(calls[3].program, OPEN);
    assert_eq!(
        calls[3].args,
        [
            "-n",
            app.to_str().unwrap(),
            "--args",
            "--permission",
            "accessibility"
        ]
    );
}

#[test]
fn open_setup_without_id_opens_permissions_window() {
    let temp = TempDir::new();
    let app = temp.helper_app(None);
    let runner = FakeRunner::with_status_payload(
        r#"{"accessibility":"not-granted","screenshots":"granted"}"#,
    );

    let result = open_setup(&runner, Some(&app), None);

    assert_eq!(result.permission_id, None);
    assert!(!result.opened_settings);
    assert!(result.launched_helper);
    assert_eq!(
        result.next_step.as_deref(),
        Some("Grant Accessibility to Orcinus Computer Use, then retry get-app-state.")
    );

    let calls = runner.calls();
    assert_eq!(calls.len(), 4, "calls: {calls:?}");
    assert_eq!(calls[3].program, OPEN);
    assert_eq!(
        calls[3].args,
        ["-n", app.to_str().unwrap(), "--args", "--permissions"]
    );
}

#[test]
fn open_setup_without_id_and_all_granted_does_not_launch() {
    let temp = TempDir::new();
    let app = temp.helper_app(None);
    let runner =
        FakeRunner::with_status_payload(r#"{"accessibility":"granted","screenshots":"granted"}"#);

    let result = open_setup(&runner, Some(&app), None);

    assert_eq!(result.permission_id, None);
    assert!(!result.opened_settings);
    assert!(!result.launched_helper);
    assert_eq!(result.next_step, None);
    assert_eq!(
        result.permissions,
        Some(vec![
            entry(PermissionId::Accessibility, PermissionState::Granted),
            entry(PermissionId::Screenshots, PermissionState::Granted),
        ])
    );
    assert_eq!(runner.calls().len(), 1, "only the status refresh ran");
}

#[test]
fn open_setup_without_helper_returns_build_hint_without_running_commands() {
    let runner = FakeRunner::default();

    let result = open_setup(&runner, None, Some(PermissionId::Screenshots));

    assert_eq!(result.platform, platform());
    assert_eq!(result.helper_app_path, None);
    assert_eq!(result.permission_id, Some(PermissionId::Screenshots));
    assert!(!result.opened_settings);
    assert!(!result.launched_helper);
    assert_eq!(result.permissions, None);
    assert_eq!(result.next_step.as_deref(), Some(UNAVAILABLE_REASON));
    assert!(runner.calls().is_empty());
}

#[test]
fn reset_resets_both_services_with_plist_bundle_id_then_refreshes_status() {
    let temp = TempDir::new();
    let app = temp.helper_app(Some("com.example.fixture.computer-use"));
    let runner = FakeRunner::with_status_payload(
        r#"{"accessibility":"not-granted","screenshots":"not-granted"}"#,
    );

    let result = reset(&runner, Some(&app));

    assert_eq!(result.platform, platform());
    assert_eq!(result.helper_app_path.as_deref(), app.to_str());
    assert_eq!(result.helper_unavailable_reason, None);
    assert_eq!(
        result.permissions,
        vec![
            entry(PermissionId::Accessibility, PermissionState::NotGranted),
            entry(PermissionId::Screenshots, PermissionState::NotGranted),
        ]
    );
    assert_eq!(
        result.bundle_id.as_deref(),
        Some("com.example.fixture.computer-use")
    );

    let calls = runner.calls();
    assert_eq!(calls.len(), 3, "calls: {calls:?}");
    assert_eq!(calls[0].program, TCCUTIL);
    assert_eq!(
        calls[0].args,
        ["reset", "Accessibility", "com.example.fixture.computer-use"]
    );
    assert_eq!(calls[1].program, TCCUTIL);
    assert_eq!(
        calls[1].args,
        ["reset", "ScreenCapture", "com.example.fixture.computer-use"]
    );
    assert_eq!(
        calls[2].program, OPEN,
        "status is refreshed after the reset"
    );
}

#[test]
fn reset_falls_back_to_default_bundle_id_when_plist_is_missing() {
    let temp = TempDir::new();
    let app = temp.helper_app(None);
    let runner = FakeRunner::with_status_payload(
        r#"{"accessibility":"granted","screenshots":"not-granted"}"#,
    );

    let result = reset(&runner, Some(&app));

    assert_eq!(result.bundle_id.as_deref(), Some(DEFAULT_BUNDLE_ID));
    let calls = runner.calls();
    assert_eq!(calls[0].args, ["reset", "Accessibility", DEFAULT_BUNDLE_ID]);
    assert_eq!(calls[1].args, ["reset", "ScreenCapture", DEFAULT_BUNDLE_ID]);
}

#[test]
fn reset_without_helper_still_resets_with_fallback_bundle_id() {
    let runner = FakeRunner::default();

    let result = reset(&runner, None);

    assert_eq!(result.platform, platform());
    assert_eq!(result.helper_app_path, None);
    assert_eq!(
        result.helper_unavailable_reason.as_deref(),
        Some(UNAVAILABLE_REASON)
    );
    assert_eq!(
        result.permissions,
        vec![
            entry(PermissionId::Accessibility, PermissionState::NotGranted),
            entry(PermissionId::Screenshots, PermissionState::NotGranted),
        ]
    );
    assert_eq!(result.bundle_id.as_deref(), Some(DEFAULT_BUNDLE_ID));

    let calls = runner.calls();
    assert_eq!(calls.len(), 2, "calls: {calls:?}");
    assert_eq!(calls[0].program, TCCUTIL);
    assert_eq!(calls[0].args, ["reset", "Accessibility", DEFAULT_BUNDLE_ID]);
    assert_eq!(calls[1].program, TCCUTIL);
    assert_eq!(calls[1].args, ["reset", "ScreenCapture", DEFAULT_BUNDLE_ID]);
}

#[test]
fn serde_shapes_match_the_shared_typescript_contract() {
    let setup = OpenSetupResult {
        platform: "darwin".to_string(),
        helper_app_path: Some("/tmp/Orcinus Computer Use.app".to_string()),
        permission_id: Some(PermissionId::Screenshots),
        opened_settings: true,
        launched_helper: true,
        permissions: Some(vec![entry(
            PermissionId::Screenshots,
            PermissionState::Granted,
        )]),
        next_step: Some("step".to_string()),
    };
    assert_eq!(
        serde_json::to_value(&setup).unwrap(),
        json!({
            "platform": "darwin",
            "helperAppPath": "/tmp/Orcinus Computer Use.app",
            "permissionId": "screenshots",
            "openedSettings": true,
            "launchedHelper": true,
            "permissions": [{"id": "screenshots", "status": "granted"}],
            "nextStep": "step",
        })
    );

    let minimal = OpenSetupResult {
        platform: "darwin".to_string(),
        helper_app_path: None,
        permission_id: None,
        opened_settings: false,
        launched_helper: false,
        permissions: None,
        next_step: None,
    };
    assert_eq!(
        serde_json::to_value(&minimal).unwrap(),
        json!({
            "platform": "darwin",
            "helperAppPath": null,
            "openedSettings": false,
            "launchedHelper": false,
            "nextStep": null,
        })
    );

    let status = PermissionStatus {
        platform: "darwin".to_string(),
        helper_app_path: None,
        helper_unavailable_reason: None,
        permissions: vec![],
    };
    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        json!({
            "platform": "darwin",
            "helperAppPath": null,
            "helperUnavailableReason": null,
            "permissions": [],
        })
    );

    let reset = ResetResult {
        platform: "darwin".to_string(),
        helper_app_path: None,
        helper_unavailable_reason: Some("missing".to_string()),
        permissions: vec![entry(
            PermissionId::Accessibility,
            PermissionState::NotGranted,
        )],
        bundle_id: Some("dev.itsuka.orcinus.computer-use".to_string()),
    };
    assert_eq!(
        serde_json::to_value(&reset).unwrap(),
        json!({
            "platform": "darwin",
            "helperAppPath": null,
            "helperUnavailableReason": "missing",
            "permissions": [{"id": "accessibility", "status": "not-granted"}],
            "bundleId": "dev.itsuka.orcinus.computer-use",
        })
    );

    assert_eq!(
        serde_json::from_value::<PermissionId>(json!("accessibility")).unwrap(),
        PermissionId::Accessibility
    );
    assert_eq!(
        serde_json::from_value::<PermissionState>(json!("not-granted")).unwrap(),
        PermissionState::NotGranted
    );
    assert_eq!(
        serde_json::to_value(PermissionState::Unsupported).unwrap(),
        json!("unsupported")
    );
}
