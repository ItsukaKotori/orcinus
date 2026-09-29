use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use ade_core::models::repo::{new_repo, now_ms, RepoKind};
use ade_core::path_compare::normalize_for_comparison;
use ade_fs::FsService;
use ade_git::base_ref::BaseRefSearchResult;
use ade_store::projects_store::ProjectsStore;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tauri::{Manager, State};

use crate::commands::project_groups::revoke_root_if_unused;
use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::{lock, AppState};

/// Fields `repos_update` accepts (the renderer contract Pick list). Everything
/// else — including the SSH-only fields — is dropped.
pub const REPO_UPDATE_FIELDS: &[&str] = &[
    "displayName",
    "badgeColor",
    "repoIcon",
    "upstream",
    "hookSettings",
    "worktreeBaseRef",
    "worktreeBasePath",
    "kind",
    "issueSourcePreference",
    "forkSyncMode",
    "externalWorktreeVisibilityPromptDismissedAt",
    "externalWorktreeInboxBaselinePaths",
    "importedExternalWorktreePaths",
    "customWorktreeVisibilitySources",
    "worktreeVisibilitySourcePreferences",
    "projectGroupId",
    "projectGroupOrder",
    "externalWorktreeVisibility",
    "agentWorktreeVisibility",
    "sourceControlAi",
    "externalWorktreeDiscoverySuppressedAt",
];

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReposAddArgs {
    pub path: String,
    #[serde(default)]
    pub kind: Option<RepoKind>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReposUpdateArgs {
    pub repo_id: String,
    /// Accepted for contract parity; A has only local repos, so it is ignored.
    #[serde(default)]
    pub host_id: Option<String>,
    pub updates: Json,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReposRemoveArgs {
    pub repo_id: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReposReorderForHostArgs {
    pub ordered_ids: Vec<String>,
    pub host_id: String,
}

/// `repos:create` payload (`repo-creation-handlers.ts:132-136`); an absent or
/// unknown kind coerces to `git`, exactly like the oracle's narrow union.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReposCreateArgs {
    pub parent_path: String,
    pub name: String,
    #[serde(default)]
    pub kind: Option<RepoKind>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GetBaseRefDefaultArgs {
    pub repo_id: String,
    #[serde(default)]
    pub host_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchBaseRefsArgs {
    pub repo_id: String,
    pub query: String,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub host_id: Option<String>,
}

/// `git init`/`git commit` budget for `repos_create`.
const REPO_CREATE_GIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Verbatim setup hint (`repo-creation-handlers.ts:247-248`).
const IDENTITY_SETUP_HINT: &str = "Git author identity is not configured. Run `git config --global user.name \"Your Name\"` and `git config --global user.email \"you@example.com\"`, then try again.";

/// Outcome of a successful `repos_add` (spec §5.2).
#[derive(Debug, Clone, PartialEq)]
pub struct AddRepoOutcome {
    pub repo: Value,
    pub already_existed: bool,
}

fn repo_id(repo: &Value) -> &str {
    repo.get("id").and_then(Value::as_str).unwrap_or_default()
}

fn repo_path(repo: &Value) -> Option<&str> {
    repo.get("path").and_then(Value::as_str)
}

fn find_repo(store: &ProjectsStore, repo_id_value: &str) -> Option<Value> {
    store
        .repos()
        .into_iter()
        .find(|repo| repo_id(repo) == repo_id_value)
}

/// `getRepoKind` (`orca:src/shared/repo-kind.ts:3-5`): only an explicit
/// `folder` kind is a folder; absent/unknown kinds read as git.
fn repo_kind_of(repo: &Value) -> RepoKind {
    repo.get("kind")
        .and_then(Value::as_str)
        .and_then(RepoKind::parse)
        .unwrap_or(RepoKind::Git)
}

/// `getRepoExecutionHostId` (`orca:src/shared/execution-host.ts:158-167`): an
/// explicit `executionHostId` wins, then a connection id becomes `ssh:<id>`,
/// otherwise the repo is local.
fn repo_execution_host(repo: &Value) -> String {
    if let Some(host) = repo
        .get("executionHostId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|host| !host.is_empty())
    {
        return host.to_string();
    }
    if let Some(connection) = repo
        .get("connectionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|connection| !connection.is_empty())
    {
        return format!("ssh:{connection}");
    }
    "local".to_string()
}

/// `getRepoForExecutionHost` (`base-ref-query-handlers.ts:190-204`): without a
/// host id, match the id alone; with one, the repo must belong to that host.
fn repo_for_host(
    store: &ProjectsStore,
    repo_id_value: &str,
    host_id: Option<&str>,
) -> Option<Value> {
    store.repos().into_iter().find(|repo| {
        repo_id(repo) == repo_id_value
            && host_id.is_none_or(|host| repo_execution_host(repo) == host)
    })
}

/// Resolve the registry path for `repos_add`: git roots come from
/// `git rev-parse --show-toplevel` (canonical, so `/repo/` and a subdirectory
/// both resolve to `/repo`); folder kinds use the path exactly as picked
/// (spec §5.2 — no realpath).
pub fn resolve_add_path(path: &str, kind: RepoKind) -> Result<String, BridgeError> {
    match kind {
        RepoKind::Folder => Ok(path.to_string()),
        RepoKind::Git => Ok(ade_git::rev_parse_toplevel(path)?),
    }
}

/// Register a repo already resolved by [`resolve_add_path`]. Dedup compares
/// `normalize_for_comparison(path)` against every persisted repo, so a trailing
/// separator cannot produce a duplicate row; a duplicate returns the existing
/// repo with `already_existed=true` instead of an error (spec §4.3).
pub fn add_repo(
    store: &mut ProjectsStore,
    fs: &FsService,
    path: &str,
    kind: RepoKind,
    display_name: Option<&str>,
    added_at_ms: u64,
) -> Result<AddRepoOutcome, BridgeError> {
    let key = normalize_for_comparison(path);
    if let Some(existing) = store
        .repos()
        .into_iter()
        .find(|repo| repo_path(repo).is_some_and(|path| normalize_for_comparison(path) == key))
    {
        authorize_repo_root(fs, repo_path(&existing).unwrap_or(path));
        return Ok(AddRepoOutcome {
            repo: existing,
            already_existed: true,
        });
    }

    let repo = new_repo(
        &ade_core::ids::new_uuid(),
        path,
        display_name,
        kind,
        added_at_ms,
    );
    store.mutate_repos(|repos| repos.push(repo.clone()))?;
    authorize_repo_root(fs, path);
    Ok(AddRepoOutcome {
        repo,
        already_existed: false,
    })
}

/// Create a repo/folder from scratch and register it.
///
/// Mirrors `repos:create` (`orca:src/main/ipc/repos/repo-creation-handlers.ts:132-296`):
/// trimmed name/parent validation in oracle order, empty pre-existing
/// directories are reused, a `git` kind runs `git init` plus an empty
/// "Initial commit", and a failure cleans up exactly what this call created.
/// Domain failures answer the `{error}` contract union instead of rejecting.
pub fn create_repo(
    store: &mut ProjectsStore,
    fs: &FsService,
    args: &ReposCreateArgs,
    added_at_ms: u64,
) -> Value {
    let name = args.name.trim();
    let parent_path = args.parent_path.trim();
    // Why: IPC input is untrusted — coerce to the narrow union so a bogus kind
    // can't skip git init yet persist in the store.
    let kind = match args.kind {
        Some(RepoKind::Folder) => RepoKind::Folder,
        _ => RepoKind::Git,
    };

    if name.is_empty() {
        return json!({ "error": "Name cannot be empty" });
    }
    // Block slashes and ./.. so the name can't escape the chosen parent.
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return json!({ "error": "Name cannot contain slashes or be \".\" / \"..\"" });
    }
    if parent_path.is_empty() {
        return json!({ "error": "Parent directory is required" });
    }
    // Block CWD-relative paths at the IPC boundary — keeps targetPath stable
    // across process cwd changes.
    if !Path::new(parent_path).is_absolute() {
        return json!({ "error": "Parent directory must be an absolute path" });
    }

    let target = Path::new(parent_path).join(name);
    let target_path = target.to_string_lossy().into_owned();
    let find_exact = |store: &ProjectsStore| {
        store
            .repos()
            .into_iter()
            .find(|repo| repo_path(repo) == Some(target_path.as_str()))
    };

    // Dedup by path so a double-click on Create doesn't make two entries for
    // one folder.
    if let Some(existing) = find_exact(store) {
        return json!({ "repo": existing });
    }

    // The default parent may not exist on a fresh install; create only the
    // parent before probing the target.
    if let Err(error) = std::fs::create_dir_all(parent_path) {
        return json!({ "error": format!("Cannot access target path: {error}") });
    }

    // Empty pre-existing dirs are allowed (e.g. made in Finder first);
    // non-empty ones are rejected so we don't overwrite files.
    let mut created_dir = false;
    match std::fs::metadata(&target) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return json!({ "error": "Failed to read directory: Not a directory" });
            }
            match std::fs::read_dir(&target) {
                Ok(mut entries) => {
                    if entries.next().is_some() {
                        return json!({
                            "error": format!(
                                "\"{name}\" already exists at this location and is not empty."
                            )
                        });
                    }
                }
                Err(error) => {
                    return json!({ "error": format!("Failed to read directory: {error}") });
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(&target) {
                Ok(()) => created_dir = true,
                // EEXIST means a concurrent create won the mkdir race; return
                // its store entry instead of a confusing error.
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(winner) = find_exact(store) {
                        return json!({ "repo": winner });
                    }
                    return json!({ "error": format!("Failed to create directory: {error}") });
                }
                Err(error) => {
                    return json!({ "error": format!("Failed to create directory: {error}") });
                }
            }
        }
        Err(error) => return json!({ "error": format!("Cannot access target path: {error}") }),
    }

    if kind == RepoKind::Git {
        // Track which git step ran so the failure can attribute the error and
        // the identity hint only applies during commit.
        let mut step_commit = false;
        let failure = match run_create_git_step(&target_path, &["init"]) {
            Some(message) => Some(message),
            None => {
                step_commit = true;
                run_create_git_step(
                    &target_path,
                    &["commit", "--allow-empty", "-m", "Initial commit"],
                )
            }
        };
        if let Some(message) = failure {
            // Only rm the dir if we made it (pre-existing folders must survive
            // retry); otherwise strip just the `.git/` that `git init` created.
            if created_dir {
                let _ = std::fs::remove_dir_all(&target);
            } else if step_commit {
                let _ = std::fs::remove_dir_all(target.join(".git"));
            }
            if step_commit && looks_like_identity_error(&message) {
                return json!({ "error": IDENTITY_SETUP_HINT });
            }
            let step_label = if step_commit {
                "Failed to create initial commit"
            } else {
                "Failed to initialize git repository"
            };
            return json!({ "error": format!("{step_label}: {message}") });
        }
    }

    // Why: command invocations don't serialize, so re-check dedup here to close
    // the race between the first check and `add_repo`.
    if let Some(winner) = find_exact(store) {
        // Don't rm even if we made the dir — the race winner owns it.
        return json!({ "repo": winner });
    }

    match add_repo(store, fs, &target_path, kind, Some(name), added_at_ms) {
        Ok(outcome) => json!({ "repo": outcome.repo }),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

/// Run one `repos_create` git step; `Some(message)` on any failure, where the
/// message is the stderr text (or the process error).
fn run_create_git_step(repo_path: &str, args: &[&str]) -> Option<String> {
    match ade_git::run_git_in(repo_path, args, REPO_CREATE_GIT_TIMEOUT, None) {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Some(error.to_string()),
    }
}

/// `/Please tell me who you are|user\.name|user\.email/i`
/// (`repo-creation-handlers.ts:244`).
fn looks_like_identity_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("please tell me who you are")
        || lower.contains("user.name")
        || lower.contains("user.email")
}

/// A failed grant is logged, not fatal: startup re-authorizes every persisted
/// root, so one bad entry cannot strand the whole registry (spec §5.1).
fn authorize_repo_root(fs: &FsService, path: &str) {
    if let Err(error) = fs.authorize_root(path) {
        eprintln!("[ade-bridge] failed to authorize repo root '{path}': {error}");
    }
}

pub fn list_repos(store: &ProjectsStore) -> Vec<Value> {
    store.repos()
}

/// Apply a renderer partial to one repo. Returns `None` when the repo is gone.
pub fn update_repo(
    store: &mut ProjectsStore,
    repo_id_value: &str,
    updates: &Value,
) -> Result<Option<Value>, BridgeError> {
    if find_repo(store, repo_id_value).is_none() {
        return Ok(None);
    }
    let sanitized = sanitize_repo_updates(updates);
    let repos = store.mutate_repos(|repos| {
        if let Some(repo) = repos
            .iter_mut()
            .find(|repo| repo_id(repo) == repo_id_value)
        {
            apply_repo_updates(repo, &sanitized);
        }
    })?;
    Ok(repos
        .into_iter()
        .find(|repo| repo_id(repo) == repo_id_value))
}

/// Remove one repo and revoke its fs root when no other repo or folder
/// workspace still points at it. Does **not** cascade to project groups or
/// folder workspaces (spec §5.2); the renderer decides follow-ups.
pub fn remove_repo(
    store: &mut ProjectsStore,
    fs: &FsService,
    repo_id_value: &str,
) -> Result<Option<Value>, BridgeError> {
    let Some(repo) = find_repo(store, repo_id_value) else {
        return Ok(None);
    };
    store.mutate_repos(|repos| repos.retain(|repo| repo_id(repo) != repo_id_value))?;
    if let Some(path) = repo_path(&repo) {
        revoke_root_if_unused(store, fs, path);
    }
    Ok(Some(repo))
}

/// `repos:getBaseRefDefault`: folder repos (and unknown ids) answer
/// `{defaultBaseRef: null, remoteCount: 0}` so the renderer skips a fabricated
/// default; git repos resolve the short default ref and count remotes.
pub fn base_ref_default(
    store: &ProjectsStore,
    repo_id_value: &str,
    host_id: Option<&str>,
) -> Value {
    base_ref_default_for_repo(repo_for_host(store, repo_id_value, host_id).as_ref())
}

fn base_ref_default_for_repo(repo: Option<&Value>) -> Value {
    let Some(repo) = repo else {
        return json!({ "defaultBaseRef": null, "remoteCount": 0 });
    };
    if repo_kind_of(repo) == RepoKind::Folder {
        return json!({ "defaultBaseRef": null, "remoteCount": 0 });
    }
    let path = repo_path(repo).unwrap_or_default();
    json!({
        "defaultBaseRef": ade_git::base_ref::resolve_default_base_ref_short(path),
        "remoteCount": ade_git::base_ref::remote_count(path),
    })
}

/// `repos:searchBaseRefs`: the `refName` list of [`search_base_ref_details`].
pub fn search_base_refs(
    store: &ProjectsStore,
    repo_id_value: &str,
    query: &str,
    limit: Option<u32>,
    host_id: Option<&str>,
) -> Vec<String> {
    search_base_ref_results_for_repo(
        repo_for_host(store, repo_id_value, host_id).as_ref(),
        query,
        limit,
    )
    .into_iter()
    .map(|entry| entry.ref_name)
    .collect()
}

/// `repos:searchBaseRefDetails`: `[{refName, localBranchName}]`; folder repos
/// and unknown ids answer `[]`.
pub fn search_base_ref_details(
    store: &ProjectsStore,
    repo_id_value: &str,
    query: &str,
    limit: Option<u32>,
    host_id: Option<&str>,
) -> Vec<Value> {
    search_base_ref_results_for_repo(
        repo_for_host(store, repo_id_value, host_id).as_ref(),
        query,
        limit,
    )
    .into_iter()
    .map(|entry| {
        json!({
            "refName": entry.ref_name,
            "localBranchName": entry.local_branch_name,
        })
    })
    .collect()
}

fn search_base_ref_results_for_repo(
    repo: Option<&Value>,
    query: &str,
    limit: Option<u32>,
) -> Vec<BaseRefSearchResult> {
    let Some(repo) = repo else {
        return Vec::new();
    };
    if repo_kind_of(repo) == RepoKind::Folder {
        return Vec::new();
    }
    let requested = limit.unwrap_or(ade_git::base_ref::SEARCH_REFS_DEFAULT_LIMIT);
    // `isRepoSearchRefsRequestLimit`: zero is not a positive request.
    if requested == 0 {
        return Vec::new();
    }
    let path = repo_path(repo).unwrap_or_default();
    ade_git::base_ref::search_base_ref_details(path, query, requested)
}

/// Persist `projectGroupOrder = index` for the given permutation. Only the
/// local host is supported in A: any other `host_id`, a stale/non-permutation
/// ordering, or duplicate ids is rejected without touching the store.
pub fn reorder_repos_for_host(
    store: &mut ProjectsStore,
    ordered_ids: &[String],
    host_id: &str,
) -> Result<bool, BridgeError> {
    if host_id != "local" {
        return Ok(false);
    }
    let repos = store.repos();
    if ordered_ids.len() != repos.len() {
        return Ok(false);
    }
    let mut seen = HashSet::new();
    for id in ordered_ids {
        if !seen.insert(id.as_str()) {
            return Ok(false);
        }
    }
    if !ordered_ids
        .iter()
        .all(|id| repos.iter().any(|repo| repo_id(repo) == id.as_str()))
    {
        return Ok(false);
    }
    store.mutate_repos(|repos| {
        for (index, id) in ordered_ids.iter().enumerate() {
            if let Some(repo) = repos.iter_mut().find(|repo| repo_id(repo) == id.as_str()) {
                if let Some(map) = repo.as_object_mut() {
                    map.insert("projectGroupOrder".to_string(), Value::from(index as u64));
                }
            }
        }
    })?;
    Ok(true)
}

/// Where the "Create new project" Location field starts (oracle semantics,
/// Orcinus branding): the effective local `defaultWorktreeLocation` when the
/// user changed it, otherwise `{{HOME}}/orcinus/projects`. An untouched
/// `workspaceDir` is not a choice — treating it as one would relocate projects
/// into the worktree root.
pub fn default_create_project_parent(settings: &Value, home: &str) -> String {
    let configured = settings
        .get("hostSettingOverrides")
        .and_then(|overrides| overrides.get("local"))
        .and_then(|local| local.get("defaultWorktreeLocation"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            settings
                .get("workspaceDir")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let untouched_default =
        normalize_for_comparison(&configured)
            == normalize_for_comparison(&ade_core::defaults::default_workspace_dir(home));
    if !configured.is_empty() && !untouched_default {
        return configured;
    }
    let separator = if home.contains('\\') { '\\' } else { '/' };
    let trimmed_home = home.trim_end_matches(['\\', '/']);
    format!("{trimmed_home}{separator}orcinus{separator}projects")
}

/// Keep only contract fields with a plausible value. `null` survives only where
/// the renderer uses it as a clearing sentinel or as a real value
/// (`projectGroupId`); [`apply_repo_updates`] turns the rest into removals.
pub fn sanitize_repo_updates(updates: &Value) -> Map<String, Value> {
    let mut sanitized = Map::new();
    let Some(input) = updates.as_object() else {
        return sanitized;
    };
    for field in REPO_UPDATE_FIELDS {
        let Some(value) = input.get(*field) else {
            continue;
        };
        let accepted = match *field {
            "displayName" => non_empty_trimmed(value),
            "badgeColor" => normalize_badge_color(value),
            "worktreeBaseRef" | "worktreeBasePath" => clearable_trimmed(value),
            "kind" => value
                .as_str()
                .and_then(RepoKind::parse)
                .map(|kind| Value::String(kind.as_str().to_string())),
            "issueSourcePreference" => enum_value(value, &["upstream", "origin", "auto"]),
            "forkSyncMode" => enum_value(value, &["ask", "safe-auto", "off"]),
            "externalWorktreeVisibility" | "agentWorktreeVisibility" => {
                if value.is_null() {
                    Some(Value::Null)
                } else {
                    enum_value(value, &["hide", "show"])
                }
            }
            "sourceControlAi" => {
                if value.is_null() || value.is_object() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "externalWorktreeDiscoverySuppressedAt"
            | "externalWorktreeVisibilityPromptDismissedAt" => {
                if value.is_null() && *field == "externalWorktreeDiscoverySuppressedAt" {
                    Some(Value::Null)
                } else if value.is_number() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "externalWorktreeInboxBaselinePaths" | "importedExternalWorktreePaths" => {
                if string_array(value) {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "customWorktreeVisibilitySources" => {
                if value.is_array() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "worktreeVisibilitySourcePreferences" => {
                if value.is_object() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "projectGroupId" => {
                if value.is_null() || value.is_string() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "projectGroupOrder" => {
                if value.is_number() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            "repoIcon" | "upstream" | "hookSettings" => {
                if value.is_null() || value.is_object() {
                    Some(value.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(accepted) = accepted {
            sanitized.insert((*field).to_string(), accepted);
        }
    }
    sanitized
}

fn non_empty_trimmed(value: &Value) -> Option<Value> {
    value
        .as_str()
        .map(str::trim)
        .filter(|trimmed| !trimmed.is_empty())
        .map(|trimmed| Value::String(trimmed.to_string()))
}

/// Trimmed string, or the `null` removal sentinel for `null`/blank input. Tauri
/// IPC strips `undefined`, so the renderer maps its "Use Global"/"Use primary"
/// clears to `null` (or `''`) and both must clear the field here.
fn clearable_trimmed(value: &Value) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    value.as_str().map(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            Value::Null
        } else {
            Value::String(trimmed.to_string())
        }
    })
}

fn enum_value(value: &Value, allowed: &[&str]) -> Option<Value> {
    value
        .as_str()
        .filter(|candidate| allowed.contains(candidate))
        .map(|candidate| Value::String(candidate.to_string()))
}

fn string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
}

/// Normalize a badge color to `#rrggbb`; invalid values are dropped (oracle
/// `normalizeRepoBadgeColor`).
fn normalize_badge_color(value: &Value) -> Option<Value> {
    let raw = value.as_str()?.trim().trim_start_matches('#');
    if !matches!(raw.len(), 3 | 6) || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let expanded = if raw.len() == 3 {
        raw.chars()
            .flat_map(|c| [c, c])
            .collect::<String>()
    } else {
        raw.to_string()
    };
    Some(Value::String(format!("#{}", expanded.to_lowercase())))
}

/// Merge sanitized updates into a repo row. A `null` value removes the field
/// except for `projectGroupId`, where null is the real "ungrouped" value.
pub fn apply_repo_updates(repo: &mut Value, updates: &Map<String, Value>) -> bool {
    let Some(map) = repo.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for (key, value) in updates {
        if value.is_null() && key != "projectGroupId" {
            changed |= map.remove(key).is_some();
        } else if map.get(key) != Some(value) {
            map.insert(key.clone(), value.clone());
            changed = true;
        }
    }
    changed
}

/// Shared command epilogue: registry mutations broadcast `repos:changed` plus
/// one `worktrees:changed` per affected repo (spec §5.2/§5.3).
fn emit_repo_mutation(app: &tauri::AppHandle, repo_id_value: &str) {
    events::emit_repos_changed(app);
    events::emit_worktrees_changed(app, repo_id_value);
}

/// Read the projects registry (spec §5.2).
#[tauri::command]
#[specta::specta]
pub async fn repos_list(state: State<'_, AppState>) -> Result<Json, BridgeError> {
    Ok(Json::new(Value::Array(list_repos(&lock(&state.projects)))))
}

/// Add a repo; invalid git paths answer the `{error}` contract union instead of
/// rejecting, and duplicates answer the existing repo with `alreadyExisted`.
#[tauri::command]
#[specta::specta]
pub async fn repos_add(
    state: State<'_, AppState>,
    args: ReposAddArgs,
) -> Result<Json, BridgeError> {
    let kind = args.kind.unwrap_or(RepoKind::Git);
    let path = args.path.clone();
    let resolved = match run_blocking(move || resolve_add_path(&path, kind)).await {
        Ok(resolved) => resolved,
        Err(BridgeError::Core(error @ ade_core::errors::CoreError::NotAGitRepository(_))) => {
            return Ok(Json::new(json!({ "error": error.to_string() })));
        }
        Err(error) => return Err(error),
    };

    let outcome = {
        let mut projects = lock(&state.projects);
        add_repo(
            &mut projects,
            &state.fs,
            &resolved,
            kind,
            args.display_name.as_deref(),
            now_ms(),
        )?
    };
    emit_repo_mutation(&state.app, repo_id(&outcome.repo));
    Ok(Json::new(json!({
        "repo": outcome.repo,
        "alreadyExisted": outcome.already_existed,
    })))
}

/// Update the contract-allowed fields of one repo and return the updated row.
#[tauri::command]
#[specta::specta]
pub async fn repos_update(
    state: State<'_, AppState>,
    args: ReposUpdateArgs,
) -> Result<Json, BridgeError> {
    let updated = {
        let mut projects = lock(&state.projects);
        update_repo(&mut projects, &args.repo_id, &args.updates.into_inner())?
    };
    let Some(repo) = updated else {
        return Err(BridgeError::message(format!(
            "Repo not found: {}",
            args.repo_id
        )));
    };
    emit_repo_mutation(&state.app, &args.repo_id);
    Ok(Json::new(repo))
}

/// Remove a repo and revoke its fs root; no cascade (spec §5.2).
#[tauri::command]
#[specta::specta]
pub async fn repos_remove(
    state: State<'_, AppState>,
    args: ReposRemoveArgs,
) -> Result<(), BridgeError> {
    let removed = {
        let mut projects = lock(&state.projects);
        remove_repo(&mut projects, &state.fs, &args.repo_id)?
    };
    if let Some(repo) = removed {
        emit_repo_mutation(&state.app, repo_id(&repo));
    }
    Ok(())
}

/// Persist one host's repo order; non-local hosts are rejected in A.
#[tauri::command]
#[specta::specta]
pub async fn repos_reorder_for_host(
    state: State<'_, AppState>,
    args: ReposReorderForHostArgs,
) -> Result<Json, BridgeError> {
    let applied = {
        let mut projects = lock(&state.projects);
        reorder_repos_for_host(&mut projects, &args.ordered_ids, &args.host_id)?
    };
    if applied {
        events::emit_repos_changed(&state.app);
        for repo_id_value in &args.ordered_ids {
            events::emit_worktrees_changed(&state.app, repo_id_value);
        }
    }
    Ok(Json::new(json!({
        "status": if applied { "applied" } else { "rejected" },
    })))
}

/// First selected path of a single-selection picker; cancel is `None`.
pub fn first_selected_path(selection: Option<std::path::PathBuf>) -> Option<String> {
    selection.map(|path| path.to_string_lossy().into_owned())
}

/// Every selected path of a multi-selection picker; cancel is `[]`.
pub fn selected_paths(selection: Option<Vec<std::path::PathBuf>>) -> Vec<String> {
    selection
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Pick one folder for the add-project flow (system dialog, no JS dialog plugin).
#[tauri::command]
#[specta::specta]
pub async fn repos_pick_folder() -> Result<Option<String>, BridgeError> {
    let selection = rfd::AsyncFileDialog::new().pick_folder().await;
    Ok(first_selected_path(selection.map(|handle| handle.path().to_path_buf())))
}

/// Pick several folders for the add-project flow.
#[tauri::command]
#[specta::specta]
pub async fn repos_pick_folders() -> Result<Vec<String>, BridgeError> {
    let selection = rfd::AsyncFileDialog::new().pick_folders().await;
    Ok(selected_paths(selection.map(|handles| {
        handles
            .into_iter()
            .map(|handle| handle.path().to_path_buf())
            .collect()
    })))
}

/// Pick a clone/create destination (same dialog as `pick_folder`, separate
/// renderer entry point).
#[tauri::command]
#[specta::specta]
pub async fn repos_pick_directory() -> Result<Option<String>, BridgeError> {
    let selection = rfd::AsyncFileDialog::new().pick_folder().await;
    Ok(first_selected_path(selection.map(|handle| handle.path().to_path_buf())))
}

/// `git --version` with the 1.5s budget from `ade_git`.
#[tauri::command]
#[specta::specta]
pub async fn repos_is_git_available() -> Result<bool, BridgeError> {
    run_blocking(|| Ok(ade_git::is_available())).await
}

/// Effective local default parent for "Create new project".
#[tauri::command]
#[specta::specta]
pub async fn repos_get_default_create_project_parent(
    state: State<'_, AppState>,
) -> Result<String, BridgeError> {
    let settings = state.settings_store().get();
    let home = state
        .app
        .path()
        .home_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(default_create_project_parent(&settings, &home))
}

/// Create a repo/folder from scratch; validation and git failures answer the
/// `{error}` contract union instead of rejecting. A successful create
/// broadcasts `repos:changed` plus `worktrees:changed` for the new repo.
#[tauri::command]
#[specta::specta]
pub async fn repos_create(
    state: State<'_, AppState>,
    args: ReposCreateArgs,
) -> Result<Json, BridgeError> {
    let value = {
        let mut projects = lock(&state.projects);
        create_repo(&mut projects, &state.fs, &args, now_ms())
    };
    if let Some(id) = value
        .get("repo")
        .and_then(|repo| repo.get("id"))
        .and_then(Value::as_str)
    {
        emit_repo_mutation(&state.app, id);
    }
    Ok(Json::new(value))
}

/// `repos:getBaseRefDefault`: folder repos (and unknown ids) answer
/// `{defaultBaseRef: null, remoteCount: 0}`; git repos resolve the short
/// default base ref and count configured remotes.
#[tauri::command]
#[specta::specta]
pub async fn repos_get_base_ref_default(
    state: State<'_, AppState>,
    args: GetBaseRefDefaultArgs,
) -> Result<Json, BridgeError> {
    let repo = {
        let projects = lock(&state.projects);
        repo_for_host(&projects, &args.repo_id, args.host_id.as_deref())
    };
    run_blocking(move || Ok(Json::new(base_ref_default_for_repo(repo.as_ref())))).await
}

/// `repos:searchBaseRefs`: short ref names matching `query`.
#[tauri::command]
#[specta::specta]
pub async fn repos_search_base_refs(
    state: State<'_, AppState>,
    args: SearchBaseRefsArgs,
) -> Result<Json, BridgeError> {
    let repo = {
        let projects = lock(&state.projects);
        repo_for_host(&projects, &args.repo_id, args.host_id.as_deref())
    };
    let names = run_blocking(move || {
        Ok(
            search_base_ref_results_for_repo(repo.as_ref(), &args.query, args.limit)
                .into_iter()
                .map(|entry| entry.ref_name)
                .collect::<Vec<_>>(),
        )
    })
    .await?;
    Ok(Json::new(json!(names)))
}

/// `repos:searchBaseRefDetails`: `[{refName, localBranchName}]`.
#[tauri::command]
#[specta::specta]
pub async fn repos_search_base_ref_details(
    state: State<'_, AppState>,
    args: SearchBaseRefsArgs,
) -> Result<Json, BridgeError> {
    let repo = {
        let projects = lock(&state.projects);
        repo_for_host(&projects, &args.repo_id, args.host_id.as_deref())
    };
    let details = run_blocking(move || {
        Ok(
            search_base_ref_results_for_repo(repo.as_ref(), &args.query, args.limit)
                .into_iter()
                .map(|entry| {
                    json!({
                        "refName": entry.ref_name,
                        "localBranchName": entry.local_branch_name,
                    })
                })
                .collect::<Vec<_>>(),
        )
    })
    .await?;
    Ok(Json::new(json!(details)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ade_store::projects_store::ProjectsStore;
    use serde_json::json;

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ade-bridge-repos-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn store(dir: &TestDir) -> ProjectsStore {
        ProjectsStore::load(dir.path.join("projects.json"))
    }

    #[test]
    fn kind_defaults_to_git_and_parses_known_values() {
        assert_eq!(RepoKind::parse("folder"), Some(RepoKind::Folder));
        assert_eq!(RepoKind::parse("nonsense").unwrap_or(RepoKind::Git), RepoKind::Git);
    }

    #[test]
    fn resolve_add_path_keeps_folder_paths_verbatim() {
        assert_eq!(
            resolve_add_path("/some/folder/", RepoKind::Folder).unwrap(),
            "/some/folder/"
        );
    }

    #[test]
    fn sanitize_drops_unknown_and_ssh_only_fields() {
        let sanitized = sanitize_repo_updates(&json!({
            "displayName": "  Renamed  ",
            "connectionId": "ssh:box",
            "executionHostId": "ssh:box",
            "projectGroupOrder": 3,
            "totallyUnknown": true
        }));
        assert_eq!(sanitized.get("displayName"), Some(&json!("Renamed")));
        assert_eq!(sanitized.get("projectGroupOrder"), Some(&json!(3)));
        assert_eq!(sanitized.len(), 2);
    }

    #[test]
    fn sanitize_validates_enums_and_normalizes_badge_color() {
        let sanitized = sanitize_repo_updates(&json!({
            "badgeColor": "  #ABC ",
            "issueSourcePreference": "elsewhere",
            "forkSyncMode": "safe-auto",
            "kind": "folder",
            "externalWorktreeVisibility": "hide",
            "agentWorktreeVisibility": "sometimes",
            "worktreeBaseRef": " main "
        }));
        assert_eq!(sanitized.get("badgeColor"), Some(&json!("#aabbcc")));
        assert_eq!(sanitized.get("forkSyncMode"), Some(&json!("safe-auto")));
        assert_eq!(sanitized.get("kind"), Some(&json!("folder")));
        assert_eq!(
            sanitized.get("externalWorktreeVisibility"),
            Some(&json!("hide"))
        );
        assert_eq!(sanitized.get("worktreeBaseRef"), Some(&json!("main")));
        assert_eq!(sanitized.get("issueSourcePreference"), None);
        assert_eq!(sanitized.get("agentWorktreeVisibility"), None);
    }

    #[test]
    fn worktree_base_fields_accept_null_and_blank_as_clear_sentinels() {
        let sanitized = sanitize_repo_updates(&json!({
            "worktreeBasePath": null,
            "worktreeBaseRef": "   "
        }));
        assert_eq!(sanitized.get("worktreeBasePath"), Some(&Value::Null));
        assert_eq!(sanitized.get("worktreeBaseRef"), Some(&Value::Null));

        let invalid = sanitize_repo_updates(&json!({
            "worktreeBasePath": 42,
            "worktreeBaseRef": { "not": "a string" }
        }));
        assert_eq!(invalid.get("worktreeBasePath"), None);
        assert_eq!(invalid.get("worktreeBaseRef"), None);
    }

    #[test]
    fn clearing_worktree_base_fields_removes_them_from_the_row() {
        let mut repo = json!({
            "id": "r1",
            "worktreeBasePath": "/custom/worktrees",
            "worktreeBaseRef": "main"
        });
        let updates = sanitize_repo_updates(&json!({
            "worktreeBasePath": null,
            "worktreeBaseRef": ""
        }));
        assert!(apply_repo_updates(&mut repo, &updates));
        assert!(repo.get("worktreeBasePath").is_none());
        assert!(repo.get("worktreeBaseRef").is_none());
    }

    #[test]
    fn apply_treats_null_as_clear_except_for_project_group() {
        let mut repo = json!({
            "id": "r1",
            "repoIcon": { "type": "emoji", "emoji": "x" },
            "projectGroupId": "g1"
        });
        let updates = sanitize_repo_updates(&json!({
            "repoIcon": null,
            "projectGroupId": null
        }));
        assert!(apply_repo_updates(&mut repo, &updates));
        assert!(repo.get("repoIcon").is_none());
        assert_eq!(repo["projectGroupId"], Value::Null);
    }

    #[test]
    fn update_repo_reports_missing_ids() {
        let dir = TestDir::new("update-missing");
        let mut store = store(&dir);
        let updated = update_repo(&mut store, "nope", &json!({ "displayName": "x" })).unwrap();
        assert!(updated.is_none());
    }

    #[test]
    fn update_repo_persists_only_sanitized_fields() {
        let dir = TestDir::new("update-persist");
        let mut store = store(&dir);
        store
            .mutate_repos(|repos| {
                repos.push(json!({ "id": "r1", "path": "/repo", "displayName": "Old" }))
            })
            .unwrap();
        let updated = update_repo(
            &mut store,
            "r1",
            &json!({
                "displayName": "New",
                "connectionId": "ssh:box",
                "badgeColor": "not-a-color"
            }),
        )
        .unwrap()
        .expect("repo exists");
        assert_eq!(updated["displayName"], "New");
        assert!(updated.get("connectionId").is_none());
        assert_eq!(store.repos()[0]["displayName"], "New");
    }

    #[test]
    fn reorder_rejects_non_local_hosts_and_non_permutations() {
        let dir = TestDir::new("reorder-reject");
        let mut store = store(&dir);
        store
            .mutate_repos(|repos| {
                repos.push(json!({ "id": "r1", "path": "/a" }));
                repos.push(json!({ "id": "r2", "path": "/b" }));
            })
            .unwrap();

        assert!(!reorder_repos_for_host(
            &mut store,
            &["r2".to_string(), "r1".to_string()],
            "ssh:box"
        )
        .unwrap());
        assert!(!reorder_repos_for_host(&mut store, &["r1".to_string()], "local").unwrap());
        assert!(!reorder_repos_for_host(
            &mut store,
            &["r1".to_string(), "r1".to_string()],
            "local"
        )
        .unwrap());
        assert!(store.repos()[0].get("projectGroupOrder").is_none());
    }

    #[test]
    fn reorder_writes_project_group_order_for_local() {
        let dir = TestDir::new("reorder-apply");
        let mut store = store(&dir);
        store
            .mutate_repos(|repos| {
                repos.push(json!({ "id": "r1", "path": "/a" }));
                repos.push(json!({ "id": "r2", "path": "/b" }));
            })
            .unwrap();
        assert!(reorder_repos_for_host(
            &mut store,
            &["r2".to_string(), "r1".to_string()],
            "local"
        )
        .unwrap());
        let repos = store.repos();
        let r1 = repos.iter().find(|repo| repo["id"] == "r1").unwrap();
        let r2 = repos.iter().find(|repo| repo["id"] == "r2").unwrap();
        assert_eq!(r1["projectGroupOrder"], 1);
        assert_eq!(r2["projectGroupOrder"], 0);
    }

    #[test]
    fn default_parent_uses_untouched_default_check() {
        let home = "/Users/tester";
        let default_workspace = ade_core::defaults::default_workspace_dir(home);
        let untouched = json!({ "workspaceDir": default_workspace });
        assert_eq!(
            default_create_project_parent(&untouched, home),
            "/Users/tester/orcinus/projects"
        );

        let configured = json!({ "workspaceDir": "/custom/worktrees" });
        assert_eq!(
            default_create_project_parent(&configured, home),
            "/custom/worktrees"
        );

        let override_settings = json!({
            "workspaceDir": default_workspace,
            "hostSettingOverrides": { "local": { "defaultWorktreeLocation": "/override" } }
        });
        assert_eq!(
            default_create_project_parent(&override_settings, home),
            "/override"
        );

        let blank_override = json!({
            "workspaceDir": "/custom/worktrees",
            "hostSettingOverrides": { "local": { "defaultWorktreeLocation": "   " } }
        });
        assert_eq!(
            default_create_project_parent(&blank_override, home),
            "/custom/worktrees"
        );
    }

    #[test]
    fn default_parent_ignores_trailing_separator_spelling_of_the_default() {
        let home = "/Users/tester";
        let settings = json!({ "workspaceDir": "/Users/tester/orca/workspaces/" });
        assert_eq!(
            default_create_project_parent(&settings, home),
            "/Users/tester/orcinus/projects"
        );
    }

    #[test]
    fn default_parent_joins_with_the_host_separator() {
        let home = "C:\\Users\\tester\\";
        let settings = json!({ "workspaceDir": ade_core::defaults::default_workspace_dir(home) });
        assert_eq!(
            default_create_project_parent(&settings, home),
            "C:\\Users\\tester\\orcinus\\projects"
        );
    }

    #[test]
    fn picker_helpers_map_cancel_and_selection() {
        assert_eq!(first_selected_path(None), None);
        assert_eq!(
            first_selected_path(Some(std::path::PathBuf::from("/a"))),
            Some("/a".to_string())
        );
        assert_eq!(selected_paths(None), Vec::<String>::new());
        assert_eq!(
            selected_paths(Some(vec![
                std::path::PathBuf::from("/a"),
                std::path::PathBuf::from("/b")
            ])),
            vec!["/a".to_string(), "/b".to_string()]
        );
    }

    #[test]
    fn remove_repo_revokes_a_root_no_other_row_uses() {
        let dir = TestDir::new("remove-unused-root");
        let fs = FsService::new();
        let mut store = store(&dir);
        let repo_dir = dir.path.join("only");
        std::fs::create_dir_all(&repo_dir).unwrap();
        let path = repo_dir.to_str().unwrap();
        store
            .mutate_repos(|repos| repos.push(json!({ "id": "r1", "path": path })))
            .unwrap();
        fs.authorize_root(path).unwrap();

        assert!(remove_repo(&mut store, &fs, "r1").unwrap().is_some());
        assert!(store.repos().is_empty());
        assert!(fs.resolve(path).is_err());
    }

    #[test]
    fn remove_repo_keeps_a_root_another_row_still_uses() {
        let dir = TestDir::new("remove-shared-root");
        let fs = FsService::new();
        let mut store = store(&dir);
        let repo_dir = dir.path.join("shared");
        std::fs::create_dir_all(&repo_dir).unwrap();
        let path = repo_dir.to_str().unwrap();
        store
            .mutate_repos(|repos| {
                repos.push(json!({ "id": "r1", "path": path }));
                repos.push(json!({ "id": "r2", "path": path }));
            })
            .unwrap();
        fs.authorize_root(path).unwrap();

        assert!(remove_repo(&mut store, &fs, "r1").unwrap().is_some());
        assert!(
            fs.resolve(path).is_ok(),
            "a root still referenced by another repo must stay authorized"
        );
    }

    #[test]
    fn list_repos_reads_the_store() {
        let dir = TestDir::new("list");
        let mut store = store(&dir);
        store
            .mutate_repos(|repos| repos.push(json!({ "id": "r1", "path": "/a" })))
            .unwrap();
        assert_eq!(list_repos(&store), store.repos());
        assert_eq!(list_repos(&store).len(), 1);
    }

    #[test]
    fn is_git_available_matches_the_host() {
        // The integration suite shells out to git, so the host must have it.
        assert!(ade_git::is_available());
    }
}
