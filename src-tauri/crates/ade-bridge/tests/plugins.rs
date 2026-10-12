//! T12b integration coverage: the plugin command surface through the
//! state-free `*_impl` helpers over a tempdir-installed local fixture.
//!
//! No node, git, pty, or Tauri runtime: the service is assembled with a
//! missing node override (worker activation is never exercised here) and fake
//! pty/secrets/emitter seams, mirroring `tests/plugins_service.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ade_bridge::commands::plugins::{
    plugins_consent_impl, plugins_get_logs_impl, plugins_get_project_lock_state_impl,
    plugins_install_impl, plugins_install_marketplace_plugin_impl,
    plugins_install_missing_from_lockfile_impl, plugins_list_impl,
    plugins_list_language_packs_impl, plugins_list_marketplace_plugins_impl,
    plugins_list_marketplaces_impl, plugins_read_panel_entry_impl, plugins_refresh_impl,
    plugins_remove_impl, plugins_set_enabled_impl, PluginConsentDecisionDto,
    PluginHostInstallSourceDto, PluginInstallResultDto, PluginScopeDto, PluginsConsentArgs,
    PluginsGetLogsArgs, PluginsInstallMarketplacePluginArgs, PluginsPluginRefArgs,
    PluginsProjectLockArgs, PluginsReadPanelEntryArgs, PluginsSetEnabledArgs,
};
use ade_bridge::plugins_service::{
    build_plugin_service, EventEmitter, PluginServiceDeps, PtySessionInfo, PtyTerminals,
};
use ade_fs::FsService;
use ade_plugins::secrets::{SecretStore, SECRET_VALUE_MAX_BYTES};
use ade_plugins::service::PluginService;
use ade_plugins::types::{PluginError, PluginScope};
use ade_store::projects_store::ProjectsStore;
use ade_store::settings_store::SettingsStore;
use ade_store::ui_state_store::UiStateStore;
use ade_store::worktree_meta_store::WorktreeMetaStore;
use serde_json::{json, Value};

const HOST_VERSION: &str = "1.4.0";
const PLUGIN_KEY: &str = "orca-samples.demo";
const WORKTREE: &str = "r1::/tmp/repo/wt";

// ===== fakes =====

#[derive(Default)]
struct FakePty;

impl PtyTerminals for FakePty {
    fn sessions(&self) -> Vec<PtySessionInfo> {
        Vec::new()
    }

    fn write(&self, _terminal_id: &str, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct FakeSecrets;

impl SecretStore for FakeSecrets {
    fn get(
        &self,
        _plugin_key: &str,
        _scope: PluginScope,
        _repo_id: Option<&str>,
        _key: &str,
    ) -> Result<Option<String>, PluginError> {
        Ok(None)
    }

    fn set(
        &self,
        _plugin_key: &str,
        _scope: PluginScope,
        _repo_id: Option<&str>,
        _key: &str,
        value: &str,
    ) -> Result<(), PluginError> {
        assert!(value.len() <= SECRET_VALUE_MAX_BYTES);
        Ok(())
    }

    fn delete(
        &self,
        _plugin_key: &str,
        _scope: PluginScope,
        _repo_id: Option<&str>,
        _key: &str,
    ) -> Result<(), PluginError> {
        Ok(())
    }
}

// ===== fixtures =====

fn settings_store(root: &Path, plugin_system_enabled: bool) -> Arc<Mutex<SettingsStore>> {
    Arc::new(Mutex::new(SettingsStore::load(
        root.join("settings.json"),
        json!({
            "pluginConsents": {},
            "disabledPlugins": [],
            "devPluginPaths": [],
            "pluginSystemEnabled": plugin_system_enabled
        }),
    )))
}

fn projects_store(root: &Path, repos: &[(&str, &Path)]) -> Arc<Mutex<ProjectsStore>> {
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
    Arc::new(Mutex::new(ProjectsStore::load(root.join("projects.json"))))
}

/// A complete local plugin tree: worker `main.mjs`, one panel, one tab, and
/// two commands (one worker, one built-in alias action).
fn write_fixture_plugin(root: &Path) -> PathBuf {
    let dir = root.join("fixture");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("main.mjs"), "export function activate() {}\n").unwrap();
    fs::write(
        dir.join("panel.html"),
        "<!doctype html><title>demo panel</title>",
    )
    .unwrap();
    fs::write(
        dir.join("board.html"),
        "<!doctype html><title>board</title>",
    )
    .unwrap();
    fs::write(
        dir.join("ade-plugin.json"),
        serde_json::to_vec(&json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo Plugin",
            "version": "1.2.3",
            "description": "fixture plugin",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "main": "main.mjs",
            "capabilities": [{ "kind": "events:subscribe" }],
            "contributes": {
                "panels": [{ "id": "main-panel", "title": "Main Panel", "entry": "panel.html" }],
                "tabs": [{ "id": "board", "title": "Board", "entry": "board.html" }],
                "commands": [
                    { "id": "demo.hello", "title": "Hello" },
                    { "id": "demo.open-board", "title": "Open Board", "action": "workspace.openBoard" }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    dir
}

fn service(
    root: &Path,
    repos: &[(&str, &Path)],
) -> (Arc<PluginService>, Arc<Mutex<SettingsStore>>) {
    let settings = settings_store(root, true);
    let emitter = Arc::new(|_: &str, _: Value| {}) as EventEmitter;
    let deps = PluginServiceDeps {
        app_data_dir: root.join("app-data"),
        resource_dir: Some(root.join("resources")),
        node_override: Some(root.join("missing-node").to_string_lossy().into_owned()),
        host_version: HOST_VERSION.to_string(),
        settings: Arc::clone(&settings),
        ui: Arc::new(Mutex::new(UiStateStore::load(
            root.join("ui-state.json"),
            json!({ "lastActiveWorktreeId": WORKTREE }),
        ))),
        projects: projects_store(root, repos),
        worktree_meta: Arc::new(WorktreeMetaStore::load(root.join("worktrees.json"))),
        fs: Arc::new(FsService::new()),
        pty: Arc::new(FakePty),
        secrets: Arc::new(FakeSecrets),
        notify: Arc::new(|_, _, _| true),
        emit: emitter,
    };
    (build_plugin_service(deps).unwrap(), settings)
}

fn global_ref() -> PluginsPluginRefArgs {
    PluginsPluginRefArgs {
        plugin_key: PLUGIN_KEY.to_string(),
        scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
        repo_id: None,
    }
}

fn install_fixture(
    service: &PluginService,
    fixture: &Path,
) -> Vec<ade_bridge::commands::plugins::PluginListEntryDto> {
    let result = plugins_install_impl(
        service,
        PluginHostInstallSourceDto::LocalPath {
            path: fixture.to_string_lossy().into_owned(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
            repo_id: None,
        },
    );
    assert!(result.ok, "install failed: {:?}", result.error);
    assert_eq!(result.plugin_key.as_deref(), Some(PLUGIN_KEY));
    assert_eq!(result.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        result.content_hash.as_deref().map(str::len),
        Some(64),
        "content hash is a full sha256"
    );
    assert!(result.resolved_commit.is_none());
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(
        value["resolvedCommit"],
        Value::Null,
        "local installs still serialize resolvedCommit as null"
    );
    plugins_list_impl(service)
}

// ===== tests =====

#[test]
fn install_then_list_projects_the_full_ts_shape() {
    let root = tempfile::tempdir().unwrap();
    let fixture = write_fixture_plugin(root.path());
    let (service, _) = service(root.path(), &[]);
    let entries = install_fixture(&service, &fixture);

    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.plugin_key, PLUGIN_KEY);
    assert_eq!(
        entry.scope,
        ade_bridge::commands::plugins::PluginScopeDto::Global
    );
    assert_eq!(entry.repo_id, None);
    assert_eq!(entry.name, "Demo Plugin");
    assert_eq!(entry.version, "1.2.3");
    assert_eq!(entry.publisher, "orca-samples");
    assert_eq!(entry.description.as_deref(), Some("fixture plugin"));
    assert_eq!(
        entry.status,
        ade_bridge::commands::plugins::PluginStatusDto::Pending,
        "installed but unapproved"
    );
    assert!(
        !entry.needs_reconsent,
        "a first-time pending plugin has no prior consent to re-request"
    );
    assert!(entry.error.is_none());
    assert!(entry.consent_fingerprint.is_some());
    assert!(!entry.is_dev);
    assert!(!entry.official);
    assert!(!entry.bundled);
    assert!(entry.has_worker);
    assert_eq!(entry.restarts, 0);

    assert_eq!(entry.capabilities.len(), 1);
    assert_eq!(entry.capabilities[0].kind, "events:subscribe");

    assert_eq!(entry.panels.len(), 1);
    assert_eq!(entry.panels[0].id, "main-panel");
    assert_eq!(entry.panels[0].tab_key, format!("plugin:{PLUGIN_KEY}"));

    assert_eq!(entry.tabs.len(), 1);
    assert_eq!(entry.tabs[0].id, "board");
    assert_eq!(entry.tabs[0].title, "Board");

    // Command inventory: stable ids in manifest order plus handler kinds.
    let commands: Vec<(&str, bool)> = entry
        .commands
        .iter()
        .map(|command| {
            (
                command.id.as_str(),
                matches!(
                    command.handler,
                    ade_bridge::commands::plugins::PluginCommandHandlerDto::BuiltIn { .. }
                ),
            )
        })
        .collect();
    assert_eq!(
        commands,
        vec![("demo.hello", false), ("demo.open-board", true)]
    );
    assert_eq!(
        entry.commands[0].context,
        ade_bridge::commands::plugins::PluginCommandContextDto::Global
    );
    assert_eq!(entry.commands[0].keybindings.len(), 0);
    assert_eq!(entry.source.kind, "local-path");
    assert_eq!(
        entry.install_hash.as_deref(),
        entry.source.content_hash.as_deref()
    );
}

#[test]
fn consent_then_set_enabled_flips_status_both_ways() {
    let root = tempfile::tempdir().unwrap();
    let fixture = write_fixture_plugin(root.path());
    let (service, settings) = service(root.path(), &[]);
    install_fixture(&service, &fixture);

    let reviewed = plugins_list_impl(&service)[0]
        .consent_fingerprint
        .clone()
        .expect("fingerprint before consent");

    let declined = plugins_consent_impl(
        &service,
        PluginsConsentArgs {
            plugin_key: PLUGIN_KEY.to_string(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
            repo_id: None,
            reviewed_fingerprint: reviewed.clone(),
            decision: PluginConsentDecisionDto::KeepDisabled,
        },
    )
    .unwrap();
    assert_eq!(
        declined[0].status,
        ade_bridge::commands::plugins::PluginStatusDto::Disabled,
        "a declined consent records the disable bit"
    );

    let approved = plugins_consent_impl(
        &service,
        PluginsConsentArgs {
            plugin_key: PLUGIN_KEY.to_string(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
            repo_id: None,
            reviewed_fingerprint: reviewed,
            decision: PluginConsentDecisionDto::Approve,
        },
    )
    .unwrap();
    assert_eq!(
        approved[0].status,
        ade_bridge::commands::plugins::PluginStatusDto::Idle
    );
    assert!(!approved[0].needs_reconsent);

    let disabled = plugins_set_enabled_impl(
        &service,
        PluginsSetEnabledArgs {
            plugin_key: PLUGIN_KEY.to_string(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
            repo_id: None,
            enabled: false,
        },
    )
    .unwrap();
    assert_eq!(
        disabled[0].status,
        ade_bridge::commands::plugins::PluginStatusDto::Disabled
    );

    let enabled = plugins_set_enabled_impl(
        &service,
        PluginsSetEnabledArgs {
            plugin_key: PLUGIN_KEY.to_string(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
            repo_id: None,
            enabled: true,
        },
    )
    .unwrap();
    assert_eq!(
        enabled[0].status,
        ade_bridge::commands::plugins::PluginStatusDto::Idle
    );

    // The main-owned settings keys are the consent record (global:<key>).
    let persisted: Value =
        serde_json::from_str(&fs::read_to_string(root.path().join("settings.json")).unwrap())
            .unwrap();
    assert!(persisted["pluginConsents"]["global:orca-samples.demo"].is_string());
    assert_eq!(persisted["disabledPlugins"], json!([]));
    drop(settings);
}

#[test]
fn read_panel_entry_requires_consent_then_returns_html_and_token() {
    let root = tempfile::tempdir().unwrap();
    let fixture = write_fixture_plugin(root.path());
    let (service, _) = service(root.path(), &[]);
    install_fixture(&service, &fixture);

    let args = PluginsReadPanelEntryArgs {
        plugin_key: PLUGIN_KEY.to_string(),
        panel_id: "main-panel".to_string(),
        scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
        repo_id: None,
        owner_window: None,
    };
    let error = plugins_read_panel_entry_impl(&service, args.clone()).unwrap_err();
    assert_eq!(error.code(), "consent_required");

    let reviewed = plugins_list_impl(&service)[0]
        .consent_fingerprint
        .clone()
        .unwrap();
    plugins_consent_impl(
        &service,
        PluginsConsentArgs {
            plugin_key: PLUGIN_KEY.to_string(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
            repo_id: None,
            reviewed_fingerprint: reviewed,
            decision: PluginConsentDecisionDto::Approve,
        },
    )
    .unwrap();

    let entry = plugins_read_panel_entry_impl(&service, args)
        .unwrap()
        .expect("panel entry after approval");
    assert!(entry.html.contains("demo panel"));
    assert!(entry.html.contains("Content-Security-Policy"));
    assert!(entry.session_token.len() >= 32);

    // Unknown panels come back as `null`, not an error.
    let missing = plugins_read_panel_entry_impl(
        &service,
        PluginsReadPanelEntryArgs {
            plugin_key: PLUGIN_KEY.to_string(),
            panel_id: "nope".to_string(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Global,
            repo_id: None,
            owner_window: None,
        },
    )
    .unwrap();
    assert!(missing.is_none());
}

#[test]
fn project_lock_state_tracks_install_and_refresh_lists_it() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let fixture = write_fixture_plugin(root.path());
    let (service, _) = service(root.path(), &[("r1", repo.as_path())]);

    let empty = plugins_get_project_lock_state_impl(
        &service,
        PluginsProjectLockArgs {
            repo_id: "r1".to_string(),
        },
    );
    assert_eq!(empty.repo_id, "r1");
    assert!(empty.lockfile_path.ends_with("plugins.lock.json"));
    assert!(empty.entries.is_empty());

    let result = plugins_install_impl(
        &service,
        PluginHostInstallSourceDto::LocalPath {
            path: fixture.to_string_lossy().into_owned(),
            scope: ade_bridge::commands::plugins::PluginScopeDto::Project,
            repo_id: Some("r1".to_string()),
        },
    );
    assert!(result.ok, "project install failed: {:?}", result.error);

    let state = plugins_get_project_lock_state_impl(
        &service,
        PluginsProjectLockArgs {
            repo_id: "r1".to_string(),
        },
    );
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].plugin_key, PLUGIN_KEY);
    assert_eq!(
        state.entries[0].status,
        ade_bridge::commands::plugins::PluginProjectLockStatusDto::Installed
    );
    assert_eq!(
        state.entries[0].source.clone().into_inner()["kind"],
        "local-path"
    );

    // The project instance is visible in the list next to the (absent) global.
    let entries = plugins_list_impl(&service);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].repo_id.as_deref(), Some("r1"));

    // Batch restore: an installed entry is skipped, so the result set is empty.
    let lock_args = PluginsProjectLockArgs {
        repo_id: "r1".to_string(),
    };
    let results = plugins_install_missing_from_lockfile_impl(&service, lock_args.clone());
    assert!(results.is_empty());

    // Dropping the installed tree classifies the entry `missing`; the batch
    // reinstall restores it from the recorded local source.
    let installed_dir = repo.join(".ade").join("plugins").join(PLUGIN_KEY);
    fs::remove_dir_all(&installed_dir).unwrap();
    let state = plugins_get_project_lock_state_impl(&service, lock_args.clone());
    assert_eq!(
        state.entries[0].status,
        ade_bridge::commands::plugins::PluginProjectLockStatusDto::Missing
    );
    let results = plugins_install_missing_from_lockfile_impl(&service, lock_args.clone());
    assert_eq!(results.len(), 1);
    assert!(results[0].ok, "restore failed: {:?}", results[0].error);
    assert_eq!(
        plugins_get_project_lock_state_impl(&service, lock_args.clone()).entries[0].status,
        ade_bridge::commands::plugins::PluginProjectLockStatusDto::Installed
    );

    // Without the installed tree AND the local source, the entry is
    // `source-unavailable` and the restore reports `ok:false`.
    fs::remove_dir_all(&installed_dir).unwrap();
    fs::remove_dir_all(&fixture).unwrap();
    let state = plugins_get_project_lock_state_impl(&service, lock_args.clone());
    assert_eq!(
        state.entries[0].status,
        ade_bridge::commands::plugins::PluginProjectLockStatusDto::SourceUnavailable
    );
    let results = plugins_install_missing_from_lockfile_impl(&service, lock_args);
    assert_eq!(results.len(), 1);
    assert!(!results[0].ok);
    assert!(results[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("unavailable")));
}

#[test]
fn remove_refresh_logs_and_language_packs_are_workerless() {
    let root = tempfile::tempdir().unwrap();
    let fixture = write_fixture_plugin(root.path());
    let (service, _) = service(root.path(), &[]);
    install_fixture(&service, &fixture);

    assert!(plugins_list_language_packs_impl(&service).is_empty());

    let logs = plugins_get_logs_impl(
        &service,
        PluginsGetLogsArgs {
            plugin_key: PLUGIN_KEY.to_string(),
        },
    );
    assert!(logs
        .iter()
        .any(|line| line.line.contains("plugins.install")));

    let refreshed = plugins_refresh_impl(&service);
    assert_eq!(refreshed.len(), 1);

    let removed = plugins_remove_impl(&service, global_ref()).unwrap();
    assert!(removed.is_empty());
}

#[test]
fn marketplace_surface_starts_empty_without_a_runtime() {
    let root = tempfile::tempdir().unwrap();
    let (service, _) = service(root.path(), &[]);
    // The official source is seeded by the UI flow, not assembly; an unseeded
    // data dir reports empty sources/listings.
    let sources = plugins_list_marketplaces_impl(&service).unwrap();
    let listings = plugins_list_marketplace_plugins_impl(&service).unwrap();
    for source in &sources {
        assert!(!source.stale);
    }
    assert!(listings.is_empty());
}

#[test]
fn marketplace_install_args_carry_the_chosen_scope_into_the_install_source() {
    let root = tempfile::tempdir().unwrap();
    let (service, _) = service(root.path(), &[]);
    let project: PluginsInstallMarketplacePluginArgs = serde_json::from_value(json!({
        "marketplaceSourceId": "a".repeat(32),
        "marketplaceCommit": "b".repeat(40),
        "pluginKey": PLUGIN_KEY,
        "resolvedCommit": "c".repeat(40),
        "scope": "project",
        "repoId": "r1"
    }))
    .unwrap();
    match project.clone().into_install_source() {
        PluginHostInstallSourceDto::Marketplace {
            source_id,
            plugin_key,
            scope,
            repo_id,
        } => {
            assert_eq!(source_id, "a".repeat(32));
            assert_eq!(plugin_key, PLUGIN_KEY);
            assert_eq!(scope, PluginScopeDto::Project);
            assert_eq!(repo_id.as_deref(), Some("r1"));
        }
        other => panic!("expected a marketplace install source, got {other:?}"),
    }

    let global: PluginsInstallMarketplacePluginArgs = serde_json::from_value(json!({
        "marketplaceSourceId": "a".repeat(32),
        "marketplaceCommit": "b".repeat(40),
        "pluginKey": PLUGIN_KEY,
        "resolvedCommit": "c".repeat(40),
        "scope": "global"
    }))
    .unwrap();
    match global.into_install_source() {
        PluginHostInstallSourceDto::Marketplace { scope, repo_id, .. } => {
            assert_eq!(scope, PluginScopeDto::Global);
            assert_eq!(repo_id, None);
        }
        other => panic!("expected a marketplace install source, got {other:?}"),
    }

    // The command impl accepts the scoped args; without a seeded marketplace
    // it fails at listing resolution and never fabricates a resolved commit.
    let result = plugins_install_marketplace_plugin_impl(&service, project);
    assert!(!result.ok);
    assert!(result.resolved_commit.is_none());

    let missing_scope = serde_json::from_value::<PluginsInstallMarketplacePluginArgs>(json!({
        "marketplaceSourceId": "a".repeat(32),
        "marketplaceCommit": "b".repeat(40),
        "pluginKey": PLUGIN_KEY,
        "resolvedCommit": "c".repeat(40)
    }));
    assert!(
        missing_scope.is_err(),
        "the marketplace install scope is required"
    );
}

#[test]
fn install_result_carries_the_resolved_commit_into_the_dto() {
    let commit = "d".repeat(40);
    let dto = PluginInstallResultDto::from(ade_plugins::install::InstallResult {
        plugin_key: PLUGIN_KEY.to_string(),
        version: "1.2.3".to_string(),
        content_hash: "a".repeat(64),
        consent_fingerprint: "sha256-consent".to_string(),
        install_hash: "a".repeat(64),
        scope: PluginScope::Global,
        resolved_commit: Some(commit.clone()),
    });
    assert_eq!(dto.resolved_commit.as_deref(), Some(commit.as_str()));
    let value = serde_json::to_value(&dto).unwrap();
    assert_eq!(value["resolvedCommit"], json!(commit));
}

#[test]
fn command_inventory_and_bindings_are_fresh() {
    const COMMANDS: [&str; 22] = [
        "plugins_list",
        "plugins_list_language_packs",
        "plugins_consent",
        "plugins_set_enabled",
        "plugins_read_panel_entry",
        "plugins_invoke_command",
        "plugins_panel_action",
        "plugins_install",
        "plugins_list_marketplaces",
        "plugins_add_marketplace",
        "plugins_remove_marketplace",
        "plugins_refresh_marketplaces",
        "plugins_list_marketplace_plugins",
        "plugins_preview_marketplace_plugin",
        "plugins_install_marketplace_plugin",
        "plugins_preview_marketplace_update",
        "plugins_rollback_marketplace_plugin",
        "plugins_remove",
        "plugins_get_logs",
        "plugins_refresh",
        "plugins_get_project_lock_state",
        "plugins_install_missing_from_lockfile",
    ];
    let bindings = ade_bridge::specta_export::export_bindings();
    for command in COMMANDS {
        assert!(
            bindings.contains(&format!("\"{command}\"")),
            "bindings are missing command {command}"
        );
    }
    assert_eq!(
        bindings.matches("\"plugins_").count(),
        COMMANDS.len(),
        "the plugins command inventory is exactly these 22 names"
    );
    assert_eq!(
        bindings,
        include_str!("../../../../src/bridge/real/generated/tauri-bindings.ts"),
        "regenerate with: cargo run -p ade-bridge --bin export-bindings"
    );
}

#[test]
fn disabled_system_gate_shape_is_stable() {
    let outcome = ade_bridge::commands::plugins::disabled_system_outcome();
    assert!(!outcome.ok);
    assert_eq!(
        outcome.code.as_deref(),
        Some(ade_bridge::commands::plugins::DISABLED_SYSTEM)
    );
    assert!(!ade_bridge::commands::plugins::DISABLED_SYSTEM.is_empty());
}
