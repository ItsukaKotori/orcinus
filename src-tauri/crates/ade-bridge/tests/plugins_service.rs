//! T12a integration coverage: plugin service assembly, the bridge consent
//! adapter, the `PluginHostServices` implementation over fakes (no node, no
//! git, no pty), sink payload shapes, and ticker shutdown.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ade_bridge::events::{
    PluginOpenTabPayload, PluginsChangedPayload, PLUGINS_CHANGED, PLUGINS_OPEN_TAB,
};
use ade_bridge::plugins_service::{
    build_plugin_service, resolve_host_entry_path, BridgeHostServicesDeps,
    BridgePluginHostServices, BridgeServiceEventSink, EventEmitter, PluginServiceDeps,
    PluginTicker, PtySessionInfo, PtyTerminals, SettingsConsentState, WorktreeContexts,
};
use ade_fs::FsService;
use ade_plugins::consent::ConsentState;
use ade_plugins::host::services::{PluginHostServices, WorktreeContext};
use ade_plugins::secrets::{SecretStore, SECRET_VALUE_MAX_BYTES};
use ade_plugins::service::{
    OpenTabRequest, PluginService, ServiceConfig, ServiceEventSink, WorkerRuntimeConfig,
};
use ade_plugins::types::{PluginError, PluginScope};
use ade_store::projects_store::ProjectsStore;
use ade_store::settings_store::SettingsStore;
use ade_store::ui_state_store::UiStateStore;
use ade_store::worktree_meta_store::WorktreeMetaStore;
use serde_json::{json, Value};

const WORKTREE: &str = "r1::/tmp/repo/wt";

// ===== fakes =====

#[derive(Default)]
struct FakePty {
    sessions: Mutex<Vec<PtySessionInfo>>,
    writes: Mutex<Vec<(String, Vec<u8>)>>,
}

impl FakePty {
    fn with_sessions(sessions: Vec<PtySessionInfo>) -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::new(sessions),
            writes: Mutex::new(Vec::new()),
        })
    }
}

impl PtyTerminals for FakePty {
    fn sessions(&self) -> Vec<PtySessionInfo> {
        self.sessions.lock().unwrap().clone()
    }

    fn write(&self, terminal_id: &str, bytes: &[u8]) -> Result<(), String> {
        self.writes
            .lock()
            .unwrap()
            .push((terminal_id.to_string(), bytes.to_vec()));
        Ok(())
    }
}

#[derive(Default)]
struct FakeSecrets {
    entries: Mutex<Vec<(String, String)>>,
}

impl SecretStore for FakeSecrets {
    fn get(&self, _plugin_key: &str, _key: &str) -> Result<Option<String>, PluginError> {
        Ok(None)
    }

    fn set(&self, plugin_key: &str, key: &str, value: &str) -> Result<(), PluginError> {
        assert!(value.len() <= SECRET_VALUE_MAX_BYTES);
        self.entries
            .lock()
            .unwrap()
            .push((format!("{plugin_key}:{key}"), value.to_string()));
        Ok(())
    }

    fn delete(&self, _plugin_key: &str, _key: &str) -> Result<(), PluginError> {
        Ok(())
    }
}

struct FakeWorktrees;

impl WorktreeContexts for FakeWorktrees {
    fn resolve(&self, worktree_id: &str) -> Option<(String, String)> {
        (worktree_id == WORKTREE).then(|| ("feature/x".to_string(), "Feature X".to_string()))
    }
}

/// `(plugin_key, title, body)` triples recorded by the fake dispatcher.
type NotificationLog = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

#[derive(Default)]
struct RecordingEmitter {
    events: Mutex<Vec<(String, Value)>>,
}

impl RecordingEmitter {
    fn emitter(self: &Arc<Self>) -> EventEmitter {
        let this = Arc::clone(self);
        Arc::new(move |event: &str, payload: Value| {
            this.events
                .lock()
                .unwrap()
                .push((event.to_string(), payload));
        })
    }
}

#[derive(Default)]
struct RecordingSink {
    changes: Mutex<Vec<(bool, bool)>>,
    tabs: Mutex<Vec<OpenTabRequest>>,
}

impl ServiceEventSink for RecordingSink {
    fn plugins_changed(&self, content_packs_changed: bool, plugins_changed: bool) {
        self.changes
            .lock()
            .unwrap()
            .push((content_packs_changed, plugins_changed));
    }

    fn open_tab_requested(&self, request: OpenTabRequest) {
        self.tabs.lock().unwrap().push(request);
    }
}

// ===== fixtures =====

fn settings_store(root: &Path) -> Arc<Mutex<SettingsStore>> {
    Arc::new(Mutex::new(SettingsStore::load(
        root.join("settings.json"),
        json!({
            "pluginConsents": {},
            "disabledPlugins": [],
            "devPluginPaths": [],
            "pluginSystemEnabled": false
        }),
    )))
}

fn ui_store(root: &Path, last_active: Option<&str>) -> Arc<Mutex<UiStateStore>> {
    Arc::new(Mutex::new(UiStateStore::load(
        root.join("ui-state.json"),
        json!({ "lastActiveWorktreeId": last_active }),
    )))
}

fn projects_store(root: &Path) -> Arc<Mutex<ProjectsStore>> {
    Arc::new(Mutex::new(ProjectsStore::load(root.join("projects.json"))))
}

fn worktree_meta(root: &Path) -> Arc<WorktreeMetaStore> {
    Arc::new(WorktreeMetaStore::load(root.join("worktrees.json")))
}

fn host_services(
    root: &Path,
    ui: Arc<Mutex<UiStateStore>>,
    pty: Arc<dyn PtyTerminals>,
    dev_paths: Vec<PathBuf>,
    notifications: NotificationLog,
) -> BridgePluginHostServices {
    let notify = Arc::new(move |plugin_key: &str, title: &str, body: Option<&str>| {
        notifications.lock().unwrap().push((
            plugin_key.to_string(),
            title.to_string(),
            body.map(str::to_string),
        ));
        true
    });
    BridgePluginHostServices::new(BridgeHostServicesDeps {
        app_data_dir: root.join("app-data"),
        ui,
        projects: projects_store(root),
        worktree_meta: worktree_meta(root),
        fs: Arc::new(FsService::new()),
        dev_paths,
        pty,
        secrets: Arc::new(FakeSecrets::default()),
        notify,
        worktrees: Some(Arc::new(FakeWorktrees)),
    })
}

/// A valid dev plugin tree declaring one tab and one static event.
fn write_dev_plugin(root: &Path, id: &str) -> PathBuf {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("main.mjs"), "export function activate() {}\n").unwrap();
    fs::write(dir.join("tab.html"), "<!doctype html><title>board</title>").unwrap();
    fs::write(
        dir.join("ade-plugin.json"),
        serde_json::to_vec(&json!({
            "manifestVersion": 1,
            "id": id,
            "publisher": "orca-samples",
            "name": id,
            "version": "1.0.0",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "main": "main.mjs",
            "capabilities": [{ "kind": "events:subscribe" }],
            "contributes": {
                "tabs": [{ "id": "board", "title": "Board", "entry": "tab.html" }],
                "events": [{ "on": "worktree.created" }]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    dir
}

fn service_deps(root: &Path) -> PluginServiceDeps {
    let emitter = Arc::new(RecordingEmitter::default());
    PluginServiceDeps {
        app_data_dir: root.join("app-data"),
        resource_dir: Some(root.join("resources")),
        node_override: Some(root.join("missing-node").to_string_lossy().into_owned()),
        host_version: "0.0.1".to_string(),
        settings: settings_store(root),
        ui: ui_store(root, Some(WORKTREE)),
        projects: projects_store(root),
        worktree_meta: worktree_meta(root),
        fs: Arc::new(FsService::new()),
        pty: FakePty::with_sessions(Vec::new()),
        secrets: Arc::new(FakeSecrets::default()),
        notify: Arc::new(|_, _, _| true),
        emit: emitter.emitter(),
    }
}

// ===== consent adapter =====

#[test]
fn settings_consent_state_scopes_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let store = settings_store(root.path());
    let state = SettingsConsentState::new(Arc::clone(&store));

    state.set_consent("global:orca-samples.demo", "sha256-global");
    state.set_consent("project:r1:orca-samples.demo", "sha256-project");
    assert_eq!(
        state.consent_for("global:orca-samples.demo").as_deref(),
        Some("sha256-global")
    );
    assert_eq!(
        state.consent_for("project:r1:orca-samples.demo").as_deref(),
        Some("sha256-project")
    );
    assert_eq!(state.consent_for("project:r2:orca-samples.demo"), None);

    state.set_disabled("global:orca-samples.demo", true);
    assert!(state.is_disabled("global:orca-samples.demo"));
    assert!(!state.is_disabled("project:r1:orca-samples.demo"));
    state.set_disabled("global:orca-samples.demo", false);
    assert!(!state.is_disabled("global:orca-samples.demo"));

    // Main-owned fields are persisted (renderer writes strip them instead).
    let persisted: Value =
        serde_json::from_str(&fs::read_to_string(root.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        persisted["pluginConsents"]["project:r1:orca-samples.demo"],
        "sha256-project"
    );
    assert_eq!(persisted["disabledPlugins"], json!([]));
}

// ===== host services =====

#[test]
fn active_worktree_context_reads_ui_state_and_resolver() {
    let root = tempfile::tempdir().unwrap();
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let services = host_services(
        root.path(),
        ui_store(root.path(), Some(WORKTREE)),
        FakePty::with_sessions(Vec::new()),
        Vec::new(),
        notifications,
    );
    assert_eq!(
        services.active_worktree_context(),
        Some(WorktreeContext {
            worktree_id: WORKTREE.to_string(),
            branch: "feature/x".to_string(),
            display_name: "Feature X".to_string(),
        })
    );

    let empty = host_services(
        root.path(),
        ui_store(root.path(), None),
        FakePty::with_sessions(Vec::new()),
        Vec::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    assert_eq!(empty.active_worktree_context(), None);
}

#[test]
fn terminals_list_by_worktree_and_send_appends_enter() {
    let root = tempfile::tempdir().unwrap();
    let pty = FakePty::with_sessions(vec![
        PtySessionInfo {
            id: "p1".to_string(),
            worktree_id: Some(WORKTREE.to_string()),
        },
        PtySessionInfo {
            id: "p2".to_string(),
            worktree_id: Some("r1::/tmp/repo/other".to_string()),
        },
        PtySessionInfo {
            id: "p3".to_string(),
            worktree_id: None,
        },
    ]);
    let services = host_services(
        root.path(),
        ui_store(root.path(), Some(WORKTREE)),
        Arc::clone(&pty) as Arc<dyn PtyTerminals>,
        Vec::new(),
        Arc::new(Mutex::new(Vec::new())),
    );

    let terminals = services.list_worktree_terminals(WORKTREE);
    assert_eq!(
        terminals
            .iter()
            .map(|terminal| terminal.id.as_str())
            .collect::<Vec<_>>(),
        vec!["p1"]
    );

    // The plugin-visible terminal id round-trips into the write.
    assert!(services.send_terminal_text("p1", "echo hi", true).unwrap());
    assert!(services.send_terminal_text("p1", "raw", false).unwrap());
    assert_eq!(
        pty.writes.lock().unwrap().as_slice(),
        &[
            ("p1".to_string(), b"echo hi\n".to_vec()),
            ("p1".to_string(), b"raw".to_vec()),
        ]
    );

    let error = services
        .send_terminal_text("unknown", "x", false)
        .unwrap_err();
    assert_eq!(error.code(), "unavailable");
}

#[test]
fn notification_closure_receives_plugin_key_title_and_body() {
    let root = tempfile::tempdir().unwrap();
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let services = host_services(
        root.path(),
        ui_store(root.path(), None),
        FakePty::with_sessions(Vec::new()),
        Vec::new(),
        Arc::clone(&notifications),
    );
    assert!(services
        .dispatch_notification("orca-samples.demo", "Build", Some("done"),)
        .unwrap());
    assert!(services
        .dispatch_notification("orca-samples.demo", "Build", None)
        .unwrap());
    assert_eq!(
        notifications.lock().unwrap().as_slice(),
        &[
            (
                "orca-samples.demo".to_string(),
                "Build".to_string(),
                Some("done".to_string())
            ),
            ("orca-samples.demo".to_string(), "Build".to_string(), None),
        ]
    );
}

#[test]
fn subscribe_events_defaults_to_manifest_and_declared_tabs_resolve() {
    let root = tempfile::tempdir().unwrap();
    let dev = write_dev_plugin(root.path(), "events");
    let services = host_services(
        root.path(),
        ui_store(root.path(), None),
        FakePty::with_sessions(Vec::new()),
        vec![dev],
        Arc::new(Mutex::new(Vec::new())),
    );

    let accepted = services.subscribe_events(
        "orca-samples.events",
        &[
            "worktree.created".to_string(),
            "agent.status.changed".to_string(),
            "nope".to_string(),
        ],
    );
    assert_eq!(accepted, vec!["worktree.created".to_string()]);

    // A plugin without a readable manifest falls back to the known v0 set.
    let fallback = services.subscribe_events(
        "orca-samples.unknown",
        &["worktree.created".to_string(), "bogus".to_string()],
    );
    assert_eq!(fallback, vec!["worktree.created".to_string()]);

    assert_eq!(
        services.declared_tabs("orca-samples.events", PluginScope::Global, None),
        vec!["board".to_string()]
    );
    assert!(services
        .declared_tabs("orca-samples.events", PluginScope::Project, Some("r1"))
        .is_empty());
}

#[test]
fn open_tab_forwards_to_the_bound_service_and_records_the_sink() {
    let root = tempfile::tempdir().unwrap();
    let dev = write_dev_plugin(root.path(), "events");
    let host = Arc::new(host_services(
        root.path(),
        ui_store(root.path(), Some(WORKTREE)),
        FakePty::with_sessions(Vec::new()),
        vec![dev.clone()],
        Arc::new(Mutex::new(Vec::new())),
    ));
    let sink = Arc::new(RecordingSink::default());
    let service = PluginService::new(ServiceConfig {
        app_data_dir: root.path().join("app-data"),
        dev_paths: vec![dev],
        projects: Vec::new(),
        host_entry_path: root.path().join("missing-host-entry.mjs"),
        node_override: Some(
            root.path()
                .join("missing-node")
                .to_string_lossy()
                .into_owned(),
        ),
        consent_state: Arc::new(SettingsConsentState::new(settings_store(root.path()))),
        secrets: Arc::new(FakeSecrets::default()),
        host_services: Arc::clone(&host) as Arc<dyn PluginHostServices>,
        event_sink: Arc::clone(&sink) as Arc<dyn ServiceEventSink>,
        host_version: "0.0.1".to_string(),
        workers: WorkerRuntimeConfig::default(),
    });
    host.bind(Arc::clone(&service));

    let result = host
        .open_tab(
            "orca-samples.events",
            PluginScope::Global,
            None,
            "board",
            Some("Board"),
            Some(json!({ "view": "open" })),
            Some(WORKTREE),
        )
        .expect("declared tab opens without a worker");
    assert!(!result.instance_id.is_empty());
    assert_eq!(result.worktree_id, WORKTREE);

    // Dedup: the same (tabId, params) returns the tracked instance.
    let again = host
        .open_tab(
            "orca-samples.events",
            PluginScope::Global,
            None,
            "board",
            None,
            Some(json!({ "view": "open" })),
            Some(WORKTREE),
        )
        .unwrap();
    assert_eq!(again.instance_id, result.instance_id);

    let tabs = sink.tabs.lock().unwrap();
    assert_eq!(tabs.len(), 1, "only the new instance is emitted");
    assert_eq!(tabs[0].plugin_key, "orca-samples.events");
    assert_eq!(tabs[0].tab_id, "board");
    assert_eq!(tabs[0].instance_id, result.instance_id);

    let unknown = host
        .open_tab(
            "orca-samples.events",
            PluginScope::Global,
            None,
            "nope",
            None,
            None,
            Some(WORKTREE),
        )
        .unwrap_err();
    assert_eq!(unknown.code(), "invalid_params");
}

// ===== sink payloads =====

#[test]
fn bridge_sink_emits_both_plugin_events_with_ts_shapes() {
    let emitter = Arc::new(RecordingEmitter::default());
    let sink = BridgeServiceEventSink::new(emitter.emitter());
    sink.plugins_changed(false, true);
    sink.open_tab_requested(OpenTabRequest {
        plugin_key: "orca-samples.demo".to_string(),
        tab_id: "board".to_string(),
        instance_id: "inst-1".to_string(),
        title: Some("Board".to_string()),
        params: json!({ "view": "open" }),
        worktree_id: WORKTREE.to_string(),
        scope: PluginScope::Project,
        repo_id: Some("r1".to_string()),
    });

    let events = emitter.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, PLUGINS_CHANGED);
    assert_eq!(
        events[0].1,
        json!({ "contentPacksChanged": false, "pluginsChanged": true })
    );
    assert_eq!(events[1].0, PLUGINS_OPEN_TAB);
    assert_eq!(
        events[1].1,
        json!({
            "pluginKey": "orca-samples.demo",
            "tabId": "board",
            "instanceId": "inst-1",
            "title": "Board",
            "params": { "view": "open" },
            "worktreeId": WORKTREE,
            "scope": "project",
            "repoId": "r1"
        })
    );

    let payload = PluginsChangedPayload {
        content_packs_changed: true,
        plugins_changed: false,
    };
    assert_eq!(
        serde_json::to_value(payload).unwrap(),
        json!({ "contentPacksChanged": true, "pluginsChanged": false })
    );
    let tab = PluginOpenTabPayload::from(OpenTabRequest {
        plugin_key: "p".to_string(),
        tab_id: "t".to_string(),
        instance_id: "i".to_string(),
        title: None,
        params: Value::Null,
        worktree_id: "w".to_string(),
        scope: PluginScope::Global,
        repo_id: None,
    });
    let value = serde_json::to_value(tab).unwrap();
    assert_eq!(value["scope"], "global");
    assert!(value.get("title").is_none() && value.get("repoId").is_none());
}

// ===== assembly / ticker =====

#[test]
fn build_plugin_service_is_non_fatal_without_node_or_host_entry() {
    let root = tempfile::tempdir().unwrap();
    let deps = service_deps(root.path());
    assert!(!deps.app_data_dir.join("plugins").exists());
    let service = build_plugin_service(deps).expect("assembly survives missing node/host-entry");
    assert!(service.list().is_empty());
    service.shutdown();
}

#[test]
fn host_entry_resolution_prefers_an_existing_packaged_resource() {
    let root = tempfile::tempdir().unwrap();
    let resources = root.path().join("resources");
    fs::create_dir_all(resources.join("plugins")).unwrap();
    let packaged = resources.join("plugins/host-entry.mjs");
    fs::write(&packaged, b"// packaged").unwrap();
    assert_eq!(resolve_host_entry_path(Some(&resources)), packaged);
    // Dev fallback finds the checked-in source tree entry.
    let dev = resolve_host_entry_path(Some(&root.path().join("missing")));
    assert!(dev.is_file(), "dev fallback missing: {}", dev.display());
}

#[test]
fn ticker_stops_cleanly_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let service = build_plugin_service(service_deps(root.path())).unwrap();
    let ticker = PluginTicker::spawn_with_interval(Arc::clone(&service), Duration::from_millis(5));
    std::thread::sleep(Duration::from_millis(30));
    ticker.stop();
    ticker.stop();
    drop(ticker);
    assert!(service.list().is_empty());
    service.shutdown();
}
