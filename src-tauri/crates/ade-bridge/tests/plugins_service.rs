//! T12a integration coverage: plugin service assembly, the bridge consent
//! adapter, the `PluginHostServices` implementation over fakes (no node, no
//! git, no pty), sink payload shapes, and ticker shutdown.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ade_bridge::events::{
    PluginOpenTabPayload, PluginPanelPushPayload, PluginsChangedPayload, PLUGINS_CHANGED,
    PLUGINS_OPEN_TAB, PLUGINS_PANEL_PUSH,
};
use ade_bridge::plugins_service::{
    build_plugin_service, notification_id, resolve_host_entry_path, BridgeHostServicesDeps,
    BridgePluginHostServices, BridgeServiceEventSink, EventEmitter, PluginServiceDeps,
    PluginTicker, PtySessionInfo, PtyTerminals, SettingsConsentState, WorktreeContexts,
};
use ade_fs::FsService;
use ade_plugins::consent::ConsentState;
use ade_plugins::host::services::{GitLogQuery, PluginHostServices, WorktreeContext};
use ade_plugins::secrets::{SecretStore, SECRET_VALUE_MAX_BYTES};
use ade_plugins::service::{
    OpenTabRequest, PanelPushRequest, PluginService, ServiceConfig, ServiceEventSink,
    WorkerRuntimeConfig,
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

impl FakeSecrets {
    /// Scope-qualified account mirroring the Keychain adapter.
    fn account(plugin_key: &str, scope: PluginScope, repo_id: Option<&str>, key: &str) -> String {
        match scope {
            PluginScope::Global => format!("global:{plugin_key}:{key}"),
            PluginScope::Project => {
                format!("project:{}:{plugin_key}:{key}", repo_id.unwrap_or_default())
            }
        }
    }
}

impl SecretStore for FakeSecrets {
    fn get(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        key: &str,
    ) -> Result<Option<String>, PluginError> {
        let account = Self::account(plugin_key, scope, repo_id, key);
        Ok(self
            .entries
            .lock()
            .unwrap()
            .iter()
            .find(|(stored, _)| stored == &account)
            .map(|(_, value)| value.clone()))
    }

    fn set(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        key: &str,
        value: &str,
    ) -> Result<(), PluginError> {
        assert!(value.len() <= SECRET_VALUE_MAX_BYTES);
        let account = Self::account(plugin_key, scope, repo_id, key);
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|(stored, _)| stored != &account);
        entries.push((account, value.to_string()));
        Ok(())
    }

    fn delete(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        key: &str,
    ) -> Result<(), PluginError> {
        let account = Self::account(plugin_key, scope, repo_id, key);
        self.entries
            .lock()
            .unwrap()
            .retain(|(stored, _)| stored != &account);
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
    pushes: Mutex<Vec<PanelPushRequest>>,
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

    fn panel_push_requested(&self, request: PanelPushRequest) {
        self.pushes.lock().unwrap().push(request);
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
    host_services_with_projects(
        root,
        projects_store(root),
        ui,
        pty,
        dev_paths,
        notifications,
    )
}

fn host_services_with_projects(
    root: &Path,
    projects: Arc<Mutex<ProjectsStore>>,
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
        projects,
        worktree_meta: worktree_meta(root),
        fs: Arc::new(FsService::new()),
        dev_paths,
        pty,
        secrets: Arc::new(FakeSecrets::default()),
        notify,
        worktrees: Some(Arc::new(FakeWorktrees)),
    })
}

/// A projects store with the given `(repo_id, path)` registry entries, so
/// project-scope store resolution can find repo roots.
fn projects_store_with_repos(root: &Path, repos: &[(&str, &Path)]) -> Arc<Mutex<ProjectsStore>> {
    let value = json!({
        "schemaVersion": 1,
        "repos": repos
            .iter()
            .map(|(id, path)| json!({
                "id": id,
                "path": path.to_string_lossy(),
                "kind": "git"
            }))
            .collect::<Vec<_>>(),
        "projectGroups": [],
        "folderWorkspaces": []
    });
    fs::write(root.join("projects.json"), value.to_string()).unwrap();
    projects_store(root)
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

/// A `WorktreeContexts` whose `path_for` points at a real git repository.
struct RepoWorktrees {
    path: String,
}

impl WorktreeContexts for RepoWorktrees {
    fn resolve(&self, worktree_id: &str) -> Option<(String, String)> {
        (worktree_id == WORKTREE).then(|| ("main".to_string(), "main".to_string()))
    }
    fn path_for(&self, worktree_id: &str) -> Option<String> {
        (worktree_id == WORKTREE).then(|| self.path.clone())
    }
}

fn git_host_services(
    root: &Path,
    ui: Arc<Mutex<UiStateStore>>,
    worktrees: Arc<dyn WorktreeContexts>,
) -> BridgePluginHostServices {
    let notifications: NotificationLog = Arc::new(Mutex::new(Vec::new()));
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
        dev_paths: Vec::new(),
        pty: FakePty::with_sessions(Vec::new()),
        secrets: Arc::new(FakeSecrets::default()),
        notify,
        worktrees: Some(worktrees),
    })
}

/// `git init` + two commits (config isolated from user/global/system); returns `(head, parent)` oids.
fn init_git_repo(dir: &Path) -> (String, String) {
    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "core.hooksPath=",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "color.ui=false",
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.com",
            ])
            .args(args)
            .output()
            .expect("invoke git");
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-b", "main", "."]);
    fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "first"]);
    fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "second"]);
    (git(&["rev-parse", "HEAD"]), git(&["rev-parse", "HEAD~1"]))
}

#[test]
fn git_log_reads_real_history_and_commit_files() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (head, parent) = init_git_repo(repo.path());
    let worktrees = Arc::new(RepoWorktrees {
        path: repo.path().to_string_lossy().into_owned(),
    });
    let services = git_host_services(root.path(), ui_store(root.path(), Some(WORKTREE)), worktrees);

    let value = services
        .git_log(Some(WORKTREE), &GitLogQuery { limit: 50, ..Default::default() })
        .expect("git.log ok");
    assert_eq!(value["items"].as_array().map(|items| items.len()), Some(2));
    assert_eq!(value["items"][0]["id"].as_str(), Some(head.as_str()));
    assert_eq!(
        value["items"][0]["parentIds"].as_array().map(|p| p.len()),
        Some(1)
    );
    assert_eq!(value["items"][1]["id"].as_str(), Some(parent.as_str()));
    assert_eq!(
        value["items"][1]["parentIds"].as_array().map(|p| p.len()),
        Some(0)
    );
    assert_eq!(value["currentRef"]["name"].as_str(), Some("main")); // camelCase contract
    assert_eq!(value["hasMore"].as_bool(), Some(false));

    // No explicit hint → falls back to the UI active worktree.
    let fallback = services.git_log(None, &GitLogQuery::default()).expect("git.log fallback ok");
    assert_eq!(fallback["items"].as_array().map(|items| items.len()), Some(2));

    let files = services
        .git_commit_files(Some(WORKTREE), &head)
        .expect("commitFiles ok");
    let entries = files.as_array().expect("array");
    assert!(entries.iter().any(|e| e["path"].as_str() == Some("a.txt")));

    let err = services
        .git_commit_files(Some(WORKTREE), &"f".repeat(40))
        .unwrap_err();
    assert_eq!(err.code(), "unknown-oid");
}

#[test]
fn git_refs_and_query_scopes_bridge_through() {
    let root = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (head, _) = init_git_repo(repo.path());
    let worktrees = Arc::new(RepoWorktrees {
        path: repo.path().to_string_lossy().into_owned(),
    });
    let services = git_host_services(root.path(), ui_store(root.path(), Some(WORKTREE)), worktrees);

    // git.refs lists the checked-out branch.
    let refs = services.git_refs(Some(WORKTREE)).expect("git.refs ok");
    let arr = refs.as_array().expect("array");
    assert!(arr.iter().any(|r| r["name"].as_str() == Some("main")));

    // all scope returns the branch's commits.
    let all = services
        .git_log(
            Some(WORKTREE),
            &GitLogQuery { refs: Some("all".into()), ..Default::default() },
        )
        .expect("all log ok");
    assert_eq!(all["items"].as_array().map(|v| v.len()), Some(2));

    // Rev scope resolves a valid revision...
    let ok = services
        .git_log(
            Some(WORKTREE),
            &GitLogQuery { refs: Some(head.clone()), ..Default::default() },
        )
        .expect("rev log ok");
    assert!(ok["items"].as_array().map(|v| !v.is_empty()).unwrap_or(false));

    // ...and an unknown one maps to the stable unknown-oid code.
    let err = services
        .git_log(
            Some(WORKTREE),
            &GitLogQuery { refs: Some("no-such-ref".into()), ..Default::default() },
        )
        .unwrap_err();
    assert_eq!(err.code(), "unknown-oid");
}

#[test]
fn git_log_requires_resolvable_worktree_path() {
    let root = tempfile::tempdir().unwrap();
    // FakeWorktrees.resolve works but path_for defaults to None → unresolvable.
    let services = host_services(
        root.path(),
        ui_store(root.path(), Some(WORKTREE)),
        FakePty::with_sessions(Vec::new()),
        Vec::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    let err = services.git_log(Some(WORKTREE), &GitLogQuery::default()).unwrap_err();
    assert_eq!(err.code(), "unavailable");
}

#[test]
fn git_log_rejects_non_git_workspace() {
    let root = tempfile::tempdir().unwrap();
    let not_a_repo = tempfile::tempdir().unwrap(); // plain empty dir, never `git init`
    let worktrees = Arc::new(RepoWorktrees {
        path: not_a_repo.path().to_string_lossy().into_owned(),
    });
    let services = git_host_services(root.path(), ui_store(root.path(), Some(WORKTREE)), worktrees);
    let err = services.git_log(Some(WORKTREE), &GitLogQuery::default()).unwrap_err();
    assert_eq!(err.code(), "not-a-git-repo");
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

// ===== per-scope store isolation =====

#[test]
fn storage_and_settings_are_isolated_between_scopes_and_repos() {
    let root = tempfile::tempdir().unwrap();
    let repo_one = root.path().join("repo-one");
    let repo_two = root.path().join("repo-two");
    let projects = projects_store_with_repos(root.path(), &[("r1", &repo_one), ("r2", &repo_two)]);
    let services = host_services_with_projects(
        root.path(),
        projects,
        ui_store(root.path(), None),
        FakePty::with_sessions(Vec::new()),
        Vec::new(),
        Arc::new(Mutex::new(Vec::new())),
    );
    let key = "orca-samples.demo";
    let (global, project) = (PluginScope::Global, PluginScope::Project);

    services
        .storage()
        .set(key, global, None, "token", json!("global"))
        .unwrap();
    services
        .storage()
        .set(key, project, Some("r1"), "token", json!("repo-1"))
        .unwrap();
    services
        .storage()
        .set(key, project, Some("r2"), "token", json!("repo-2"))
        .unwrap();

    // Same key, three scopes: each read sees only its own bytes.
    assert_eq!(
        services.storage().get(key, global, None, "token").unwrap(),
        Some(json!("global"))
    );
    assert_eq!(
        services
            .storage()
            .get(key, project, Some("r1"), "token")
            .unwrap(),
        Some(json!("repo-1"))
    );
    assert_eq!(
        services
            .storage()
            .get(key, project, Some("r2"), "token")
            .unwrap(),
        Some(json!("repo-2"))
    );

    // Bytes land in the spec §3.3 locations, not in one shared store.
    let global_file = root
        .path()
        .join("app-data/plugin-data/orca-samples.demo/storage.json");
    let repo_one_file = repo_one.join(".ade/plugin-data/orca-samples.demo/storage.json");
    assert!(global_file.is_file() && repo_one_file.is_file());
    assert_eq!(
        serde_json::from_str::<Value>(&fs::read_to_string(&global_file).unwrap()).unwrap()["token"],
        json!("global")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&fs::read_to_string(&repo_one_file).unwrap()).unwrap()
            ["token"],
        json!("repo-1")
    );

    // Settings resolve the same way.
    services
        .settings()
        .set(key, global, None, "theme", json!("dark"))
        .unwrap();
    services
        .settings()
        .set(key, project, Some("r1"), "theme", json!("light"))
        .unwrap();
    assert_eq!(
        services
            .settings()
            .get_all(key, global, None)
            .unwrap()
            .get("theme"),
        Some(&json!("dark"))
    );
    assert_eq!(
        services
            .settings()
            .get_all(key, project, Some("r1"))
            .unwrap()
            .get("theme"),
        Some(&json!("light"))
    );
    assert!(services
        .settings()
        .get_all(key, project, Some("r2"))
        .unwrap()
        .get("theme")
        .is_none());

    // A project scope without a registered repo never touches another store.
    let unknown = services
        .storage()
        .get(key, project, Some("missing"), "token")
        .unwrap_err();
    assert_eq!(unknown.code(), "unavailable");
    let missing_repo = services
        .storage()
        .get(key, project, None, "token")
        .unwrap_err();
    assert_eq!(missing_repo.code(), "invalid_params");
    assert!(services
        .settings()
        .get_all(key, project, Some("missing"))
        .is_err());
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

#[test]
fn bridge_sink_emits_panel_push_with_camel_case_payload() {
    let emitter = Arc::new(RecordingEmitter::default());
    let sink = BridgeServiceEventSink::new(emitter.emitter());
    sink.panel_push_requested(PanelPushRequest {
        session_token: "t".repeat(32),
        topic: Some("state".to_string()),
        payload: json!({ "n": 1 }),
    });
    sink.panel_push_requested(PanelPushRequest {
        session_token: "t".repeat(32),
        topic: None,
        payload: json!(null),
    });

    let events = emitter.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, PLUGINS_PANEL_PUSH);
    assert_eq!(
        events[0].1,
        json!({ "sessionToken": "t".repeat(32), "topic": "state", "payload": { "n": 1 } })
    );
    assert_eq!(events[1].0, PLUGINS_PANEL_PUSH);
    assert_eq!(
        events[1].1,
        json!({ "sessionToken": "t".repeat(32), "payload": null })
    );
    assert!(events[1].1.get("topic").is_none());

    let payload = PluginPanelPushPayload::from(PanelPushRequest {
        session_token: "t".repeat(32),
        topic: Some("state".to_string()),
        payload: json!({ "n": 1 }),
    });
    assert_eq!(
        serde_json::to_value(payload).unwrap(),
        json!({ "sessionToken": "t".repeat(32), "topic": "state", "payload": { "n": 1 } })
    );
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
fn ticker_stop_wakes_immediately_even_with_a_long_interval() {
    let root = tempfile::tempdir().unwrap();
    let service = build_plugin_service(service_deps(root.path())).unwrap();
    // A stop that only took effect at the interval boundary would block this
    // test for an hour; the channel close must wake the parked thread at once.
    let ticker = Arc::new(PluginTicker::spawn_with_interval(
        Arc::clone(&service),
        Duration::from_secs(3600),
    ));
    std::thread::sleep(Duration::from_millis(20));
    let (done, finished) = std::sync::mpsc::channel();
    {
        let ticker = Arc::clone(&ticker);
        std::thread::spawn(move || {
            ticker.stop();
            let _ = done.send(());
        });
    }
    assert!(
        finished.recv_timeout(Duration::from_secs(2)).is_ok(),
        "stop() must wake the ticker immediately, not at the interval boundary"
    );
    ticker.stop();
    drop(ticker);
    assert!(service.list().is_empty());
    service.shutdown();
}

#[test]
fn notification_ids_are_unique_within_a_millisecond() {
    let ids: Vec<String> = (0..1000)
        .map(|_| notification_id("orca-samples.demo"))
        .collect();
    let unique: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), ids.len());
    assert!(ids[0].starts_with("plugin:orca-samples.demo:"));
}
