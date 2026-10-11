//! Runtime computer-use assembly (spec §3.4/§3.5): resolves the vendored helper
//! app, owns the provider client (or the unavailable placeholder), starts the
//! runtime RPC server over the app data directory, and exposes the TCC
//! permission surface shared by the Tauri commands and the RPC server.
//!
//! The provider slot is shared with the RPC server through one `Arc`, so
//! [`ComputerService::shutdown`] can terminate the helper the server was using.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ade_computer::availability::ProviderUnavailable;
use ade_computer::client::ProviderClient;
use ade_computer::paths;
use ade_computer::permissions::{
    self, CommandRunner, OpenSetupResult, PermissionId, PermissionStatus, ResetResult, SystemRunner,
};
use ade_computer::protocol::ComputerError;
use ade_computer::provider::{ComputerProvider, UnavailableProvider};
use ade_computer::rpc::{
    FeatureRecorder, PermissionsApi, RpcServer, RpcServerConfig, RpcServerHandle,
};
use ade_computer::transport::helper_request_timeout;
use ade_store::ui_state_store::UiStateStore;
use rand::Rng;
use serde_json::Value;

use crate::state::{lock, WriteScheduler};

/// Env override for the app data directory, honored by the app and the
/// `orcinus` CLI so both agree on where `runtime.json` lives (spec §3.5).
pub const USER_DATA_PATH_ENV: &str = "ORCINUS_USER_DATA_PATH";
/// Runtime auth token size (24 random bytes rendered as lowercase hex).
pub const AUTH_TOKEN_BYTES: usize = 24;

/// Provider slot shared with the RPC server: the real client when the helper
/// resolves, the detailed `provider_unavailable` placeholder otherwise.
enum ProviderSlot {
    Client(Arc<Mutex<ProviderClient>>),
    Unavailable(UnavailableProvider),
}

/// `ade_computer::provider::ComputerProvider` adapter over the shared slot.
struct SharedProvider {
    slot: Arc<Mutex<ProviderSlot>>,
}

impl ComputerProvider for SharedProvider {
    fn capabilities(&mut self) -> Result<Value, ComputerError> {
        match &mut *lock(&self.slot) {
            ProviderSlot::Client(client) => lock(client).ensure_ready(),
            ProviderSlot::Unavailable(provider) => provider.capabilities(),
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ComputerError> {
        match &mut *lock(&self.slot) {
            ProviderSlot::Client(client) => lock(client).call(method, params),
            ProviderSlot::Unavailable(provider) => provider.request(method, params),
        }
    }
}

/// TCC permission surface (status/setup/reset) over `ade-computer::permissions`
/// with the production command runner. `helper_app` is `None` when the helper
/// was not found; every method then reports the build/install hint without
/// touching the helper.
pub struct ComputerUsePermissionsService {
    helper_app: Option<PathBuf>,
    runner: Box<dyn CommandRunner + Send + Sync>,
}

impl ComputerUsePermissionsService {
    pub fn new(helper_app: Option<PathBuf>) -> Self {
        Self::with_runner(helper_app, Box::new(SystemRunner))
    }

    /// Injection seam for tests (fake `open`/`pkill`/`tccutil` argv capture).
    pub fn with_runner(
        helper_app: Option<PathBuf>,
        runner: Box<dyn CommandRunner + Send + Sync>,
    ) -> Self {
        Self { helper_app, runner }
    }

    pub fn status_impl(&self) -> PermissionStatus {
        permissions::probe_status(&*self.runner, self.helper_app.as_deref())
    }

    pub fn open_setup_impl(&self, permission_id: Option<PermissionId>) -> OpenSetupResult {
        permissions::open_setup(&*self.runner, self.helper_app.as_deref(), permission_id)
    }

    pub fn reset_impl(&self) -> ResetResult {
        permissions::reset(&*self.runner, self.helper_app.as_deref())
    }
}

impl PermissionsApi for ComputerUsePermissionsService {
    fn status(&self) -> PermissionStatus {
        self.status_impl()
    }

    fn open_setup(&self, id: Option<PermissionId>) -> OpenSetupResult {
        self.open_setup_impl(id)
    }
}

/// `rpc::FeatureRecorder` backed by the UI state store: records the
/// interaction in memory and debounces the disk write through the shared UI
/// scheduler (spec §3.5).
pub struct StoreFeatureRecorder {
    ui: Arc<Mutex<UiStateStore>>,
    writer: Arc<WriteScheduler>,
}

impl StoreFeatureRecorder {
    pub fn new(ui: Arc<Mutex<UiStateStore>>, writer: Arc<WriteScheduler>) -> Self {
        Self { ui, writer }
    }
}

impl FeatureRecorder for StoreFeatureRecorder {
    fn record(&self, id: &str) {
        match lock(&self.ui).record_feature_interaction_in_memory(id) {
            Ok(_) => self.writer.schedule(),
            Err(error) => {
                eprintln!("[ade-bridge] failed to record feature interaction '{id}': {error}")
            }
        }
    }
}

/// App-level computer-use service: provider, permission surface and the
/// running runtime RPC server.
pub struct ComputerService {
    /// Provider slot shared with the RPC server (see [`SharedProvider`]).
    provider: Arc<Mutex<ProviderSlot>>,
    /// Permission surface shared with the RPC server and the Tauri commands.
    pub permissions: Arc<ComputerUsePermissionsService>,
    /// Running runtime RPC server; `None` once [`ComputerService::shutdown`] ran.
    rpc: Mutex<Option<RpcServerHandle>>,
    /// App data directory holding `runtime.json` / `runtime.sock`.
    user_data_dir: PathBuf,
    /// Resolved helper `.app` path (`None` when it was not built).
    helper_app: Option<PathBuf>,
}

impl ComputerService {
    /// Assemble the service and start the runtime RPC server. Returns even when
    /// the helper is missing (the permission surface stays usable, and every
    /// provider call answers `provider_unavailable`); only RPC startup failures
    /// (socket/metadata) surface as errors.
    pub fn initialize(
        user_data_dir: &Path,
        resource_dir: Option<&Path>,
        recorder: Arc<dyn FeatureRecorder>,
    ) -> io::Result<Self> {
        let helper_app = paths::helper_app_path(
            std::env::var(paths::HELPER_APP_PATH_ENV).ok().as_deref(),
            resource_dir,
        );
        let slot = match helper_app.as_deref() {
            Some(app) => ProviderSlot::Client(Arc::new(Mutex::new(ProviderClient::new(
                app.to_path_buf(),
                helper_request_timeout(),
            )))),
            // Brief carry-over: the unavailable message points at
            // `pnpm build:computer-macos`; no path exists to report here.
            None => ProviderSlot::Unavailable(UnavailableProvider::new(
                ProviderUnavailable::HelperMissing,
                None,
            )),
        };
        let provider = Arc::new(Mutex::new(slot));
        let rpc_provider: Arc<Mutex<dyn ComputerProvider>> = Arc::new(Mutex::new(SharedProvider {
            slot: Arc::clone(&provider),
        }));
        let permissions = Arc::new(ComputerUsePermissionsService::new(helper_app.clone()));
        let rpc = RpcServer::start(
            RpcServerConfig::limits(
                ade_core::ids::new_uuid(),
                user_data_dir,
                random_hex(AUTH_TOKEN_BYTES),
                env!("CARGO_PKG_VERSION"),
            ),
            rpc_provider,
            Arc::clone(&permissions) as Arc<dyn PermissionsApi>,
            recorder,
        )?;
        Ok(Self {
            provider,
            permissions,
            rpc: Mutex::new(Some(rpc)),
            user_data_dir: user_data_dir.to_path_buf(),
            helper_app,
        })
    }

    /// Stop the runtime RPC server (removing `runtime.sock`/`runtime.json`) and
    /// terminate the helper. Best effort on the provider: an in-flight call
    /// holds the client lock for up to the 60s request timeout, so exit must not
    /// wait for it (`try_lock`); the transport still terminates the helper when
    /// the outstanding call returns.
    pub fn shutdown(&self) {
        if let Some(rpc) = lock(&self.rpc).take() {
            rpc.shutdown();
        }
        let client = match &*lock(&self.provider) {
            ProviderSlot::Client(client) => Some(Arc::clone(client)),
            ProviderSlot::Unavailable(_) => None,
        };
        if let Some(client) = client {
            if let Ok(mut client) = client.try_lock() {
                client.shutdown();
            }
        }
    }

    /// App data directory holding `runtime.json` / `runtime.sock`.
    pub fn user_data_dir(&self) -> &Path {
        &self.user_data_dir
    }

    /// Resolved helper `.app` path (`None` when it was not built).
    pub fn helper_app(&self) -> Option<&Path> {
        self.helper_app.as_deref()
    }
}

/// `byte_count` cryptographically random bytes as lowercase hex (runtime auth
/// token).
fn random_hex(byte_count: usize) -> String {
    use std::fmt::Write;

    let mut bytes = vec![0u8; byte_count];
    rand::thread_rng().fill(&mut bytes[..]);
    let mut token = String::with_capacity(byte_count * 2);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let unique = format!(
                "ade-bridge-computer-{}-{}",
                std::process::id(),
                DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(unique);
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn recorder_in(
        dir: &TempDir,
    ) -> (
        StoreFeatureRecorder,
        Arc<Mutex<UiStateStore>>,
        Arc<WriteScheduler>,
    ) {
        let ui = Arc::new(Mutex::new(UiStateStore::load(
            dir.path.join("ui-state.json"),
            json!({}),
        )));
        let writer = Arc::new(WriteScheduler::new(
            Duration::from_secs(60),
            Duration::from_secs(120),
            {
                let ui = Arc::clone(&ui);
                move || lock(&ui).persist()
            },
        ));
        (
            StoreFeatureRecorder::new(Arc::clone(&ui), Arc::clone(&writer)),
            ui,
            writer,
        )
    }

    #[test]
    fn random_hex_renders_the_requested_byte_count_once() {
        let token = random_hex(AUTH_TOKEN_BYTES);
        assert_eq!(token.len(), AUTH_TOKEN_BYTES * 2);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(token, random_hex(AUTH_TOKEN_BYTES));
    }

    #[test]
    fn store_feature_recorder_records_in_memory_and_schedules_the_ui_write() {
        let dir = TempDir::new();
        let (recorder, ui, writer) = recorder_in(&dir);

        recorder.record("computer-use");
        recorder.record("computer-use");
        recorder.record("computer-use-setup");

        let in_memory = lock(&ui).get();
        assert_eq!(
            in_memory["featureInteractions"]["computer-use"]["interactionCount"],
            2
        );
        assert_eq!(
            in_memory["featureInteractions"]["computer-use-setup"]["interactionCount"],
            1
        );

        writer.flush().expect("flush scheduled write");
        let persisted: Value =
            serde_json::from_str(&fs::read_to_string(dir.path.join("ui-state.json")).unwrap())
                .unwrap();
        assert_eq!(
            persisted["featureInteractions"]["computer-use"]["interactionCount"],
            2
        );
    }

    #[test]
    fn initialize_starts_the_rpc_server_with_metadata_and_socket_then_shutdown_cleans_up() {
        let dir = TempDir::new();
        let (recorder, _, _) = recorder_in(&dir);

        let service =
            ComputerService::initialize(&dir.path, None, Arc::new(recorder)).expect("initialize");

        let metadata_path = dir.path.join("runtime.json");
        let socket_path = dir.path.join("runtime.sock");
        assert!(metadata_path.exists(), "runtime.json must exist");
        assert!(socket_path.exists(), "runtime.sock must exist");
        assert_eq!(
            fs::metadata(&metadata_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let metadata: Value =
            serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
        assert!(metadata["runtimeId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
        assert_eq!(metadata["pid"], std::process::id());
        assert_eq!(metadata["transports"][0]["kind"], "unix");
        assert_eq!(
            metadata["transports"][0]["endpoint"],
            socket_path.to_string_lossy().as_ref()
        );
        let token = metadata["authToken"].as_str().unwrap();
        assert_eq!(token.len(), AUTH_TOKEN_BYTES * 2);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(metadata["startedAt"].as_str().unwrap().ends_with('Z'));

        // The helper resolution matches the shared path resolver (whatever the
        // machine currently has built); no helper process is ever spawned here.
        assert_eq!(
            service.helper_app(),
            paths::helper_app_path(
                std::env::var(paths::HELPER_APP_PATH_ENV).ok().as_deref(),
                None
            )
            .as_deref()
        );
        assert_eq!(service.user_data_dir(), dir.path);

        service.shutdown();
        assert!(!metadata_path.exists(), "shutdown removes runtime.json");
        assert!(!socket_path.exists(), "shutdown removes runtime.sock");
        // Idempotent: a second shutdown must not panic.
        service.shutdown();
    }

    #[test]
    fn permission_service_reports_unavailable_without_running_commands() {
        struct NeverRunner;
        impl CommandRunner for NeverRunner {
            fn run(
                &self,
                program: &str,
                _args: &[String],
            ) -> io::Result<permissions::CommandOutput> {
                panic!("must not run {program} without a helper");
            }
        }
        let service = ComputerUsePermissionsService::with_runner(None, Box::new(NeverRunner));

        let status = service.status_impl();
        assert_eq!(status.platform, "darwin");
        assert!(status.helper_app_path.is_none());
        assert!(status
            .helper_unavailable_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("pnpm build:computer-macos")));
        assert_eq!(status.permissions.len(), 2);
        assert!(status
            .permissions
            .iter()
            .all(|entry| entry.status == permissions::PermissionState::NotGranted));

        let setup = service.open_setup_impl(None);
        assert_eq!(setup.platform, "darwin");
        assert!(!setup.opened_settings);
        assert!(!setup.launched_helper);
        assert!(setup.permissions.is_none());
        assert_eq!(setup.next_step, status.helper_unavailable_reason);
    }

    type RecordedCalls = Vec<(String, Vec<String>)>;

    #[derive(Clone, Default)]
    struct RecordingRunner {
        calls: Arc<Mutex<RecordedCalls>>,
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, program: &str, args: &[String]) -> io::Result<permissions::CommandOutput> {
            lock(&self.calls).push((program.to_string(), args.to_vec()));
            Ok(permissions::CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn reset_runs_tccutil_for_both_services_and_reports_the_default_bundle_id() {
        let runner = RecordingRunner::default();
        let service = ComputerUsePermissionsService::with_runner(None, Box::new(runner.clone()));

        let result = service.reset_impl();
        assert_eq!(
            result.bundle_id.as_deref(),
            Some(paths::DEFAULT_HELPER_BUNDLE_ID)
        );
        assert!(result.helper_unavailable_reason.is_some());

        let calls = lock(&runner.calls);
        assert_eq!(
            *calls,
            vec![
                (
                    "/usr/bin/tccutil".to_string(),
                    vec![
                        "reset".to_string(),
                        "Accessibility".to_string(),
                        paths::DEFAULT_HELPER_BUNDLE_ID.to_string(),
                    ],
                ),
                (
                    "/usr/bin/tccutil".to_string(),
                    vec![
                        "reset".to_string(),
                        "ScreenCapture".to_string(),
                        paths::DEFAULT_HELPER_BUNDLE_ID.to_string(),
                    ],
                ),
            ]
        );
    }
}
