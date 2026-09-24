use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use ade_core::models::project_group::{
    is_ignored_nested_repo_directory, new_project_group, next_project_group_order, next_tab_order,
    normalize_nested_repo_scan_options, normalize_project_group_name,
    parse_nested_repo_gitignore_rules, project_group_subtree_ids, trim_path_separators,
    NestedRepoCandidate, NestedRepoIgnoreRule, NestedRepoScanOptions, NestedRepoScanResult,
    NestedRepoSelectedPathKind, ProjectGroupCreatedFrom, DEFAULT_PROJECT_GROUP_NAME,
};
use ade_core::models::repo::{basename, now_ms, RepoKind};
use ade_core::path_compare::normalize_for_comparison;
use ade_fs::FsService;
use ade_store::projects_store::ProjectsStore;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::State;

use crate::commands::repos::{add_repo, resolve_add_path};
use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::{lock, AppState};

/// Fields `project_groups_update` accepts (oracle `ProjectGroupUpdateArgs`).
pub const PROJECT_GROUP_UPDATE_FIELDS: &[&str] = &["name", "isCollapsed", "tabOrder", "color"];

/// Nested import scan fallback timeout (oracle `importNested` uses 15s when no
/// completed scan is cached).
pub const IMPORT_NESTED_SCAN_TIMEOUT_MS: u64 = 15_000;
/// Completed scans kept for `importNested` lookup (oracle cap).
pub const MAX_COMPLETED_NESTED_SCANS: usize = 50;

fn group_id(group: &Value) -> &str {
    group.get("id").and_then(Value::as_str).unwrap_or_default()
}

fn group_name(group: &Value) -> &str {
    group
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn repo_id(repo: &Value) -> &str {
    repo.get("id").and_then(Value::as_str).unwrap_or_default()
}

fn repo_path(repo: &Value) -> Option<&str> {
    repo.get("path").and_then(Value::as_str)
}

fn json_integer(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(integer) = value.as_i64() {
        return Some(integer);
    }
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .map(|number| number.floor() as i64)
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupsCreateArgs {
    pub name: String,
    #[serde(default)]
    pub parent_path: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub parent_group_id: Option<String>,
    #[serde(default)]
    pub created_from: Option<ProjectGroupCreatedFrom>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupsUpdateArgs {
    pub group_id: String,
    pub updates: Json,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupsDeleteArgs {
    pub group_id: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupsMoveProjectArgs {
    pub project_id: String,
    pub group_id: Option<String>,
    #[serde(default)]
    pub order: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupsScanNestedArgs {
    pub path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub scan_id: Option<String>,
    #[serde(default)]
    pub options: Option<Json>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupsCancelNestedScanArgs {
    pub scan_id: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupsImportNestedArgs {
    pub parent_path: String,
    #[serde(default)]
    pub group_name: Option<String>,
    pub project_paths: Vec<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub scan_id: Option<String>,
    pub mode: String,
}

/// Outcome status of one imported nested repo (oracle
/// `ProjectGroupImportProjectResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(specta::Type)]
pub enum NestedRepoImportStatus {
    Imported,
    AlreadyKnown,
    Failed,
}

/// One imported nested repo (oracle `ProjectGroupImportProjectResult`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(specta::Type)]
pub struct NestedRepoImportProjectResult {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub status: NestedRepoImportStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `project_groups_import_nested` result (oracle
/// `ProjectGroupImportResult`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(specta::Type)]
pub struct NestedRepoImportResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Json>,
    pub projects: Vec<NestedRepoImportProjectResult>,
    pub imported_count: u64,
    pub already_known_count: u64,
    pub failed_count: u64,
}

/// `projectGroups.list`: groups sorted by `tabOrder` then name (oracle
/// `getProjectGroups`).
pub fn list_groups(store: &ProjectsStore) -> Vec<Value> {
    let mut groups = store.project_groups();
    groups.sort_by(|left, right| {
        let left_order = json_integer(left.get("tabOrder")).unwrap_or(0);
        let right_order = json_integer(right.get("tabOrder")).unwrap_or(0);
        left_order
            .cmp(&right_order)
            .then_with(|| group_name(left).cmp(group_name(right)))
    });
    groups
}

/// `projectGroups.create`: `id=new_uuid`, `isCollapsed=false`, `color=null`,
/// `tabOrder=max+1`, timestamps (spec §5.2).
pub fn create_group(
    store: &mut ProjectsStore,
    args: &ProjectGroupsCreateArgs,
    now: u64,
) -> Result<Value, BridgeError> {
    if args.name.is_empty() {
        return Err(BridgeError::message("invalid_project_group_create_args"));
    }
    let tab_order = next_tab_order(&store.project_groups());
    let group = new_project_group(
        &ade_core::ids::new_uuid(),
        &args.name,
        args.parent_path.as_deref(),
        args.connection_id.as_deref(),
        args.parent_group_id.as_deref(),
        args.created_from.unwrap_or(ProjectGroupCreatedFrom::Manual),
        tab_order,
        now,
    );
    store.mutate_groups(|groups| groups.push(group.clone()))?;
    Ok(group)
}

/// Keep only contract fields; a wrong-typed field rejects the whole update
/// (oracle zod `ProjectGroupUpdateArgs`). `color: null` and non-string colors
/// both clear the field.
pub fn sanitize_group_updates(updates: &Value) -> Result<Map<String, Value>, BridgeError> {
    let mut sanitized = Map::new();
    let Some(input) = updates.as_object() else {
        return Err(BridgeError::message("invalid_project_group_update_args"));
    };
    for field in PROJECT_GROUP_UPDATE_FIELDS {
        let Some(value) = input.get(*field) else {
            continue;
        };
        let accepted = match *field {
            "name" => value.as_str().map(|name| Value::String(name.to_string())),
            "isCollapsed" => value.as_bool().map(Value::Bool),
            "tabOrder" => json_integer(Some(value)).map(Value::from),
            "color" => Some(if value.is_string() {
                value.clone()
            } else {
                Value::Null
            }),
            _ => None,
        };
        let Some(accepted) = accepted else {
            return Err(BridgeError::message("invalid_project_group_update_args"));
        };
        sanitized.insert((*field).to_string(), accepted);
    }
    Ok(sanitized)
}

/// Merge sanitized updates into one group row (oracle `updateProjectGroup`).
pub fn apply_group_updates(group: &mut Value, updates: &Map<String, Value>) {
    let Some(map) = group.as_object_mut() else {
        return;
    };
    if let Some(name) = updates.get("name").and_then(Value::as_str) {
        let fallback = map
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_PROJECT_GROUP_NAME)
            .to_string();
        map.insert(
            "name".to_string(),
            Value::String(normalize_project_group_name(name, &fallback)),
        );
    }
    if let Some(is_collapsed) = updates.get("isCollapsed").and_then(Value::as_bool) {
        map.insert("isCollapsed".to_string(), Value::Bool(is_collapsed));
    }
    if let Some(tab_order) = updates.get("tabOrder") {
        map.insert("tabOrder".to_string(), tab_order.clone());
    }
    if let Some(color) = updates.get("color") {
        map.insert("color".to_string(), color.clone());
    }
}

/// `projectGroups.update`: `None` when the group is gone, otherwise the updated
/// row with a fresh `updatedAt`.
pub fn update_group(
    store: &mut ProjectsStore,
    group_id_value: &str,
    updates: &Value,
    now: u64,
) -> Result<Option<Value>, BridgeError> {
    let sanitized = sanitize_group_updates(updates)?;
    if !store
        .project_groups()
        .iter()
        .any(|group| group_id(group) == group_id_value)
    {
        return Ok(None);
    }
    let groups = store.mutate_groups(|groups| {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group_id(group) == group_id_value)
        {
            apply_group_updates(group, &sanitized);
            if let Some(map) = group.as_object_mut() {
                map.insert("updatedAt".to_string(), Value::from(now));
            }
        }
    })?;
    Ok(groups
        .into_iter()
        .find(|group| group_id(group) == group_id_value))
}

/// Revoke a folder root only when no repo or other folder workspace still
/// points at it; startup re-authorizes every persisted root anyway.
pub(crate) fn revoke_root_if_unused(store: &ProjectsStore, fs: &FsService, path: &str) {
    let key = normalize_for_comparison(path);
    let in_use =
        store.repos().iter().any(|repo| {
            repo_path(repo).is_some_and(|value| normalize_for_comparison(value) == key)
        }) || store.folder_workspaces().iter().any(|workspace| {
            workspace
                .get("folderPath")
                .and_then(Value::as_str)
                .is_some_and(|value| normalize_for_comparison(value) == key)
        });
    if in_use {
        return;
    }
    if let Err(error) = fs.revoke_root(path) {
        eprintln!("[ade-bridge] failed to revoke folder root '{path}': {error}");
    }
}

/// `projectGroups.delete`: removes the group subtree, ungroups its repos (they
/// are kept), and deletes its folder workspaces (oracle `deleteProjectGroup`).
/// Returns `false` when the group does not exist.
pub fn delete_group(
    store: &mut ProjectsStore,
    fs: &FsService,
    group_id_value: &str,
) -> Result<bool, BridgeError> {
    let groups = store.project_groups();
    if !groups.iter().any(|group| group_id(group) == group_id_value) {
        return Ok(false);
    }
    let subtree = project_group_subtree_ids(&groups, group_id_value);
    store.mutate_groups(|groups| groups.retain(|group| !subtree.contains(group_id(group))))?;
    // Why: groups are sidebar organization only, so deleting one ungroups its
    // repos rather than deleting them (oracle `deleteProjectGroup`).
    store.mutate_repos(|repos| {
        for repo in repos.iter_mut() {
            let in_subtree = repo
                .get("projectGroupId")
                .and_then(Value::as_str)
                .is_some_and(|group| subtree.contains(group));
            if in_subtree {
                if let Some(map) = repo.as_object_mut() {
                    map.insert("projectGroupId".to_string(), Value::Null);
                }
            }
        }
    })?;
    let removed_paths: Vec<String> = store
        .folder_workspaces()
        .iter()
        .filter(|workspace| {
            workspace
                .get("projectGroupId")
                .and_then(Value::as_str)
                .is_some_and(|group| subtree.contains(group))
        })
        .filter_map(|workspace| {
            workspace
                .get("folderPath")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    store.mutate_folder_workspaces(|workspaces| {
        workspaces.retain(|workspace| {
            !workspace
                .get("projectGroupId")
                .and_then(Value::as_str)
                .is_some_and(|group| subtree.contains(group))
        })
    })?;
    for path in removed_paths {
        revoke_root_if_unused(store, fs, &path);
    }
    Ok(true)
}

/// `projectGroups.moveProject`: sets `projectGroupId`/`projectGroupOrder` on
/// one repo. An unknown group (or `null`) means "ungrouped"; an unknown repo
/// returns `None` (oracle `moveProjectToGroup`).
pub fn move_project_to_group(
    store: &mut ProjectsStore,
    project_id: &str,
    group_id_value: Option<&str>,
    order: Option<f64>,
) -> Result<Option<Value>, BridgeError> {
    let repos = store.repos();
    if !repos.iter().any(|repo| repo_id(repo) == project_id) {
        return Ok(None);
    }
    let groups = store.project_groups();
    let requested_group = group_id_value.filter(|value| !value.is_empty());
    let normalized_group = match requested_group {
        Some(value) if groups.iter().any(|group| group_id(group) == value) => {
            Some(value.to_string())
        }
        _ => None,
    };
    let siblings: Vec<Value> = repos
        .iter()
        .filter(|repo| repo_id(repo) != project_id)
        .cloned()
        .collect();
    let resolved_order = order
        .filter(|value| value.is_finite())
        .map(|value| value.floor() as i64)
        .unwrap_or_else(|| next_project_group_order(&siblings, normalized_group.as_deref()) as i64);
    let updated = store.mutate_repos(|repos| {
        if let Some(repo) = repos.iter_mut().find(|repo| repo_id(repo) == project_id) {
            if let Some(map) = repo.as_object_mut() {
                map.insert(
                    "projectGroupId".to_string(),
                    match &normalized_group {
                        Some(group) => Value::String(group.clone()),
                        None => Value::Null,
                    },
                );
                map.insert("projectGroupOrder".to_string(), Value::from(resolved_order));
            }
        }
    })?;
    Ok(updated.into_iter().find(|repo| repo_id(repo) == project_id))
}

/// Directories already recorded for one in-flight `scanId`.
fn active_scans() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static ACTIVE: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

struct CompletedScan {
    scan: NestedRepoScanResult,
    connection_id: Option<String>,
}

fn completed_scans() -> &'static Mutex<VecDeque<(String, CompletedScan)>> {
    static COMPLETED: OnceLock<Mutex<VecDeque<(String, CompletedScan)>>> = OnceLock::new();
    COMPLETED.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Register a new scan; a previous scan under the same id is cancelled first
/// (oracle `runNestedRepoScanForIpc`).
pub fn register_scan(scan_id: &str) -> Arc<AtomicBool> {
    let mut scans = lock(active_scans());
    if let Some(previous) = scans.get(scan_id) {
        previous.store(true, Ordering::SeqCst);
    }
    let flag = Arc::new(AtomicBool::new(false));
    scans.insert(scan_id.to_string(), Arc::clone(&flag));
    flag
}

/// `projectGroups.cancelNestedScan`: `false` when the scan already finished.
pub fn cancel_scan(scan_id: &str) -> bool {
    match lock(active_scans()).get(scan_id) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Drop one scan registration once it has produced its result.
pub fn finish_scan(scan_id: &str, flag: Option<&Arc<AtomicBool>>) {
    let mut scans = lock(active_scans());
    if let Some(flag) = flag {
        if scans
            .get(scan_id)
            .is_some_and(|active| Arc::ptr_eq(active, flag))
        {
            scans.remove(scan_id);
        }
    } else {
        scans.remove(scan_id);
    }
}

/// Remember a finished scan so `importNested` can reuse it (oracle
/// `rememberCompletedNestedRepoScan`).
pub fn remember_completed_scan(
    scan_id: &str,
    scan: &NestedRepoScanResult,
    connection_id: Option<&str>,
) {
    let mut scans = lock(completed_scans());
    scans.retain(|(id, _)| id != scan_id);
    scans.push_back((
        scan_id.to_string(),
        CompletedScan {
            scan: scan.clone(),
            connection_id: connection_id.map(str::to_string),
        },
    ));
    while scans.len() > MAX_COMPLETED_NESTED_SCANS {
        scans.pop_front();
    }
}

/// A completed scan is reusable only for the same scan id, parent path and
/// connection (oracle `getCompletedNestedRepoScan`).
pub fn completed_scan(
    scan_id: Option<&str>,
    parent_path: &str,
    connection_id: Option<&str>,
) -> Option<NestedRepoScanResult> {
    let scan_id = scan_id.filter(|value| !value.is_empty())?;
    let scans = lock(completed_scans());
    scans
        .iter()
        .find(|(id, record)| {
            id == scan_id
                && record.connection_id.as_deref() == connection_id
                && normalize_for_comparison(&record.scan.selected_path)
                    == normalize_for_comparison(parent_path)
        })
        .map(|(_, record)| record.scan.clone())
}

struct ScanDirEntry {
    name: String,
    is_dir: bool,
    is_symlink: bool,
}

fn read_scan_directory(path: &Path) -> std::io::Result<Vec<ScanDirEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        entries.push(ScanDirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: file_type.is_dir(),
            is_symlink: file_type.is_symlink(),
        });
    }
    Ok(entries)
}

/// `.git` directory/file, or the bare-repo marker trio HEAD + objects + refs
/// (oracle `hasGitMarker`).
pub fn has_git_marker(path: &Path) -> bool {
    if let Ok(marker) = std::fs::metadata(path.join(".git")) {
        if marker.is_dir() || marker.is_file() {
            return true;
        }
    }
    let head = std::fs::metadata(path.join("HEAD")).is_ok_and(|metadata| metadata.is_file());
    let objects = std::fs::metadata(path.join("objects")).is_ok_and(|metadata| metadata.is_dir());
    let refs = std::fs::metadata(path.join("refs")).is_ok_and(|metadata| metadata.is_dir());
    head && objects && refs
}

/// Oracle `isSelectedPathGitRepo`: `isGitRepo(path) || hasGitMarker(path)`.
fn is_selected_path_git_repo(path: &str) -> bool {
    ade_git::is_inside_work_tree(path) || has_git_marker(Path::new(path))
}

fn read_gitignore_rules(
    dir: &Path,
    entries: &[ScanDirEntry],
    base_segments: &[String],
) -> Vec<NestedRepoIgnoreRule> {
    if !entries.iter().any(|entry| entry.name == ".gitignore") {
        return Vec::new();
    }
    match std::fs::read_to_string(dir.join(".gitignore")) {
        Ok(content) => parse_nested_repo_gitignore_rules(&content, base_segments),
        Err(_) => Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_snapshot(
    path: &str,
    selected_path_kind: NestedRepoSelectedPathKind,
    repos: &[NestedRepoCandidate],
    truncated: bool,
    timed_out: bool,
    stopped: bool,
    duration_ms: u64,
    options: &NestedRepoScanOptions,
) -> NestedRepoScanResult {
    NestedRepoScanResult {
        selected_path: path.to_string(),
        selected_path_kind,
        repos: repos.to_vec(),
        truncated,
        timed_out,
        stopped,
        duration_ms,
        max_depth: options.max_depth,
        max_repos: options.max_repos,
        timeout_ms: options.timeout_ms,
    }
}

struct TraversalFolder {
    path: PathBuf,
    depth: u64,
    segments: Vec<String>,
    ignore_rules: Vec<NestedRepoIgnoreRule>,
}

/// Progress callback: immutable scan snapshot plus directories read so far.
pub type ScanProgressCallback<'a> = &'a mut dyn FnMut(&NestedRepoScanResult, u64);

/// Bounded breadth-first nested-repo scan (oracle `scanNestedRepos`): symlinked
/// directories are never followed, repos are not descended into, and
/// `maxRepos`/`maxDepth`/`timeoutMs` bound the walk. `on_progress` receives the
/// immutable snapshot plus the number of directories read so far.
pub fn scan_nested_repos(
    path: &str,
    options: &NestedRepoScanOptions,
    cancel: Option<&AtomicBool>,
    mut on_progress: Option<ScanProgressCallback<'_>>,
) -> NestedRepoScanResult {
    let started = Instant::now();
    let elapsed_ms = |started: &Instant| started.elapsed().as_millis() as u64;
    let timed_out_now = |started: &Instant| {
        options
            .timeout_ms
            .is_some_and(|limit| elapsed_ms(started) > limit)
    };
    let aborted =
        |cancel: Option<&AtomicBool>| cancel.is_some_and(|flag| flag.load(Ordering::SeqCst));

    let mut repos: Vec<NestedRepoCandidate> = Vec::new();
    let mut truncated = false;
    let mut timed_out = false;
    let mut stopped = false;
    let mut scanned_dirs: u64 = 0;

    if is_selected_path_git_repo(path) {
        return scan_snapshot(
            path,
            NestedRepoSelectedPathKind::GitRepo,
            &repos,
            truncated,
            timed_out,
            stopped,
            elapsed_ms(&started),
            options,
        );
    }
    if aborted(cancel) {
        return scan_snapshot(
            path,
            NestedRepoSelectedPathKind::NonGitFolder,
            &repos,
            truncated,
            timed_out,
            true,
            elapsed_ms(&started),
            options,
        );
    }

    let mut folders: VecDeque<TraversalFolder> = VecDeque::new();
    folders.push_back(TraversalFolder {
        path: PathBuf::from(path),
        depth: 0,
        segments: Vec::new(),
        ignore_rules: Vec::new(),
    });

    while let Some(current) = folders.pop_front() {
        if repos.len() as u64 >= options.max_repos {
            truncated = true;
            break;
        }
        if timed_out_now(&started) {
            timed_out = true;
            break;
        }
        if aborted(cancel) {
            stopped = true;
            break;
        }
        if current.depth > options.max_depth {
            continue;
        }

        let Ok(entries) = read_scan_directory(&current.path) else {
            continue;
        };
        scanned_dirs += 1;
        if aborted(cancel) {
            stopped = true;
            break;
        }

        let mut ignore_rules = current.ignore_rules.clone();
        ignore_rules.extend(read_gitignore_rules(
            &current.path,
            &entries,
            &current.segments,
        ));

        let mut dirs: Vec<ScanDirEntry> = entries
            .into_iter()
            .filter(|entry| entry.is_dir && !entry.is_symlink)
            .collect();
        dirs.sort_by(|left, right| left.name.cmp(&right.name));

        for entry in dirs {
            if repos.len() as u64 >= options.max_repos {
                truncated = true;
                break;
            }
            if timed_out_now(&started) {
                timed_out = true;
                break;
            }
            if aborted(cancel) {
                stopped = true;
                break;
            }
            let mut child_segments = current.segments.clone();
            child_segments.push(entry.name.clone());
            if is_ignored_nested_repo_directory(&entry.name, &child_segments, &ignore_rules) {
                continue;
            }
            let child_path = current.path.join(&entry.name);
            if aborted(cancel) {
                stopped = true;
                break;
            }
            if has_git_marker(&child_path) {
                repos.push(NestedRepoCandidate {
                    path: child_path.to_string_lossy().into_owned(),
                    display_name: entry.name.clone(),
                    depth: current.depth + 1,
                });
                if let Some(callback) = on_progress.as_deref_mut() {
                    let snapshot = scan_snapshot(
                        path,
                        NestedRepoSelectedPathKind::NonGitFolder,
                        &repos,
                        truncated,
                        timed_out,
                        stopped,
                        elapsed_ms(&started),
                        options,
                    );
                    callback(&snapshot, scanned_dirs);
                }
                // Project Groups organize sibling repos; nested repos stay
                // hidden until a later UI can explain submodule-style layouts.
                continue;
            }
            if current.depth < options.max_depth {
                folders.push_back(TraversalFolder {
                    path: child_path,
                    depth: current.depth + 1,
                    segments: child_segments,
                    ignore_rules: ignore_rules.clone(),
                });
            }
        }
    }

    scan_snapshot(
        path,
        NestedRepoSelectedPathKind::NonGitFolder,
        &repos,
        truncated,
        timed_out,
        stopped,
        elapsed_ms(&started),
        options,
    )
}

/// Validate `project_groups_scan_nested` inputs and return the normalized
/// options (oracle `ProjectGroupScanNestedArgs` + `validateNestedRepoScanRoot`).
pub fn validate_scan_args(
    args: &ProjectGroupsScanNestedArgs,
) -> Result<NestedRepoScanOptions, BridgeError> {
    if args.path.is_empty() {
        return Err(BridgeError::message(
            "invalid_project_group_scan_nested_args",
        ));
    }
    if args
        .scan_id
        .as_deref()
        .is_some_and(|scan_id| scan_id.is_empty())
    {
        return Err(BridgeError::message(
            "invalid_project_group_scan_nested_args",
        ));
    }
    if args
        .connection_id
        .as_deref()
        .is_some_and(|connection_id| !connection_id.is_empty())
    {
        // A has no SSH transport; the oracle would resolve an SSH provider.
        return Err(BridgeError::message("ssh_connection_unavailable"));
    }
    if !Path::new(&args.path).is_absolute() {
        return Err(BridgeError::message("Repo path must be an absolute path"));
    }
    let options = args
        .options
        .as_ref()
        .map(|options| options.0.clone())
        .unwrap_or(Value::Null);
    Ok(normalize_nested_repo_scan_options(&options))
}

/// Validate `project_groups_import_nested` inputs (oracle
/// `ProjectGroupImportNestedArgs`).
pub fn validate_import_args(args: &ProjectGroupsImportNestedArgs) -> Result<(), BridgeError> {
    if args.parent_path.is_empty() || !matches!(args.mode.as_str(), "group" | "separate") {
        return Err(BridgeError::message(
            "invalid_project_group_import_nested_args",
        ));
    }
    if args
        .scan_id
        .as_deref()
        .is_some_and(|scan_id| scan_id.is_empty())
    {
        return Err(BridgeError::message(
            "invalid_project_group_import_nested_args",
        ));
    }
    if args
        .connection_id
        .as_deref()
        .is_some_and(|connection_id| !connection_id.is_empty())
    {
        return Err(BridgeError::message("ssh_connection_unavailable"));
    }
    Ok(())
}

/// Keep only paths that the scan actually found; duplicates collapse, unrelated
/// paths are rejected (oracle `resolveNestedRepoSelection`).
pub fn resolve_import_selection(
    scan: &NestedRepoScanResult,
    project_paths: &[String],
) -> (Vec<String>, Vec<String>) {
    let candidates: HashMap<String, &str> = scan
        .repos
        .iter()
        .map(|repo| (normalize_for_comparison(&repo.path), repo.path.as_str()))
        .collect();
    let mut selected = Vec::new();
    let mut rejected = Vec::new();
    let mut seen = HashSet::new();
    for path in project_paths {
        let key = normalize_for_comparison(path);
        if !seen.insert(key.clone()) {
            continue;
        }
        match candidates.get(&key) {
            Some(canonical) => selected.push((*canonical).to_string()),
            None => rejected.push(path.clone()),
        }
    }
    (selected, rejected)
}

/// Import scanned nested repos: every selected path goes through `repos_add`
/// semantics, then lands in the root group when `mode == "group"`.
pub fn import_nested(
    store: &mut ProjectsStore,
    fs: &FsService,
    args: &ProjectGroupsImportNestedArgs,
    scan: &NestedRepoScanResult,
    now: u64,
) -> Result<NestedRepoImportResult, BridgeError> {
    let (selected_paths, rejected_paths) = resolve_import_selection(scan, &args.project_paths);
    let mut results: Vec<NestedRepoImportProjectResult> = rejected_paths
        .iter()
        .map(|path| NestedRepoImportProjectResult {
            path: path.clone(),
            project_id: None,
            status: NestedRepoImportStatus::Failed,
            error: Some("Repository was not found in the nested repo scan result".to_string()),
        })
        .collect();

    let root_group = if args.mode == "group" {
        let parent_path = trim_path_separators(&scan.selected_path);
        let fallback_name = basename(&parent_path);
        let fallback_name = if fallback_name.is_empty() {
            DEFAULT_PROJECT_GROUP_NAME.to_string()
        } else {
            fallback_name
        };
        let name = normalize_project_group_name(
            args.group_name.as_deref().unwrap_or_default(),
            &fallback_name,
        );
        let tab_order = next_tab_order(&store.project_groups());
        let group = new_project_group(
            &ade_core::ids::new_uuid(),
            &name,
            Some(&parent_path),
            args.connection_id.as_deref(),
            None,
            ProjectGroupCreatedFrom::FolderScan,
            tab_order,
            now,
        );
        store.mutate_groups(|groups| groups.push(group.clone()))?;
        Some(group)
    } else {
        None
    };
    let root_group_id = root_group.as_ref().map(group_id).map(str::to_string);

    let mut imported_project_ids: HashMap<String, String> = HashMap::new();
    for (order, candidate_path) in selected_paths.iter().enumerate() {
        let resolved = match resolve_add_path(candidate_path, RepoKind::Git) {
            Ok(resolved) => resolved,
            Err(error) => {
                results.push(NestedRepoImportProjectResult {
                    path: candidate_path.clone(),
                    project_id: None,
                    status: NestedRepoImportStatus::Failed,
                    error: Some(error.to_string()),
                });
                continue;
            }
        };
        let key = normalize_for_comparison(&resolved);
        if let Some(project_id) = imported_project_ids.get(&key) {
            results.push(NestedRepoImportProjectResult {
                path: candidate_path.clone(),
                project_id: Some(project_id.clone()),
                status: NestedRepoImportStatus::AlreadyKnown,
                error: None,
            });
            continue;
        }
        if let Some(existing) = store
            .repos()
            .into_iter()
            .find(|repo| repo_path(repo).is_some_and(|path| normalize_for_comparison(path) == key))
        {
            let existing_id = repo_id(&existing).to_string();
            if let Some(group_id_value) = root_group_id.as_deref() {
                move_project_to_group(
                    store,
                    &existing_id,
                    Some(group_id_value),
                    Some(order as f64),
                )?;
            }
            imported_project_ids.insert(key, existing_id.clone());
            results.push(NestedRepoImportProjectResult {
                path: candidate_path.clone(),
                project_id: Some(existing_id),
                status: NestedRepoImportStatus::AlreadyKnown,
                error: None,
            });
            continue;
        }
        let outcome = add_repo(store, fs, &resolved, RepoKind::Git, None, now)?;
        let new_id = repo_id(&outcome.repo).to_string();
        if let Some(group_id_value) = root_group_id.as_deref() {
            move_project_to_group(store, &new_id, Some(group_id_value), Some(order as f64))?;
        }
        imported_project_ids.insert(key, new_id.clone());
        results.push(NestedRepoImportProjectResult {
            path: candidate_path.clone(),
            project_id: Some(new_id),
            status: NestedRepoImportStatus::Imported,
            error: None,
        });
    }

    let imported_count = results
        .iter()
        .filter(|result| result.status == NestedRepoImportStatus::Imported)
        .count() as u64;
    let already_known_count = results
        .iter()
        .filter(|result| result.status == NestedRepoImportStatus::AlreadyKnown)
        .count() as u64;
    let failed_count = results
        .iter()
        .filter(|result| result.status == NestedRepoImportStatus::Failed)
        .count() as u64;

    // Why: an import that matched nothing must not leave an empty group behind
    // (oracle `importNested`).
    let group = match root_group {
        Some(group) if imported_count + already_known_count > 0 => Some(group),
        Some(group) => {
            let group_id_value = group_id(&group).to_string();
            store.mutate_groups(|groups| groups.retain(|row| group_id(row) != group_id_value))?;
            None
        }
        None => None,
    };

    Ok(NestedRepoImportResult {
        group: group.map(Json::new),
        projects: results,
        imported_count,
        already_known_count,
        failed_count,
    })
}

/// Read the projects registry's project groups (spec §5.2).
#[tauri::command]
#[specta::specta]
pub async fn project_groups_list(state: State<'_, AppState>) -> Result<Json, BridgeError> {
    Ok(Json::new(Value::Array(list_groups(&lock(&state.projects)))))
}

/// Create one project group; blank names normalize to `Untitled group`.
#[tauri::command]
#[specta::specta]
pub async fn project_groups_create(
    state: State<'_, AppState>,
    args: ProjectGroupsCreateArgs,
) -> Result<Json, BridgeError> {
    let group = {
        let mut projects = lock(&state.projects);
        create_group(&mut projects, &args, now_ms())?
    };
    events::emit_repos_changed(&state.app);
    Ok(Json::new(group))
}

/// Update the contract-allowed fields of one group.
#[tauri::command]
#[specta::specta]
pub async fn project_groups_update(
    state: State<'_, AppState>,
    args: ProjectGroupsUpdateArgs,
) -> Result<Json, BridgeError> {
    let updated = {
        let mut projects = lock(&state.projects);
        update_group(&mut projects, &args.group_id, &args.updates.0, now_ms())?
    };
    match updated {
        Some(group) => {
            events::emit_repos_changed(&state.app);
            Ok(Json::new(group))
        }
        None => Ok(Json::new(Value::Null)),
    }
}

/// Delete one group and its subtree; its repos survive ungrouped.
#[tauri::command]
#[specta::specta]
pub async fn project_groups_delete(
    state: State<'_, AppState>,
    args: ProjectGroupsDeleteArgs,
) -> Result<bool, BridgeError> {
    let deleted = {
        let mut projects = lock(&state.projects);
        delete_group(&mut projects, &state.fs, &args.group_id)?
    };
    if deleted {
        events::emit_repos_changed(&state.app);
    }
    Ok(deleted)
}

/// Move one repo into (or out of) a group.
#[tauri::command]
#[specta::specta]
pub async fn project_groups_move_project(
    state: State<'_, AppState>,
    args: ProjectGroupsMoveProjectArgs,
) -> Result<Json, BridgeError> {
    let moved = {
        let mut projects = lock(&state.projects);
        move_project_to_group(
            &mut projects,
            &args.project_id,
            args.group_id.as_deref(),
            args.order,
        )?
    };
    match moved {
        Some(repo) => {
            events::emit_repos_changed(&state.app);
            Ok(Json::new(repo))
        }
        None => Ok(Json::new(Value::Null)),
    }
}

/// Bounded nested-repo scan with optional progress events and cancellation.
#[tauri::command]
#[specta::specta]
pub async fn project_groups_scan_nested(
    state: State<'_, AppState>,
    args: ProjectGroupsScanNestedArgs,
) -> Result<NestedRepoScanResult, BridgeError> {
    let options = validate_scan_args(&args)?;
    let path = args.path.clone();
    let connection_id = args.connection_id.clone();
    let scan_id = args.scan_id.clone();
    let cancel = scan_id.as_deref().map(register_scan);
    let cancel_for_scan = cancel.clone();
    let progress_scan_id = scan_id.clone();
    let app = state.app.clone();

    let result = run_blocking(move || {
        let mut on_progress = |scan: &NestedRepoScanResult, scanned: u64| {
            if let Some(scan_id) = &progress_scan_id {
                events::emit_json(
                    &app,
                    events::PROJECT_GROUPS_SCAN_NESTED_PROGRESS,
                    events::ScanNestedProgressPayload {
                        scan_id: scan_id.clone(),
                        scanned,
                        found: scan.repos.len() as u64,
                        scan: scan.clone(),
                    },
                );
            }
        };
        Ok(scan_nested_repos(
            &path,
            &options,
            cancel_for_scan.as_deref(),
            Some(&mut on_progress),
        ))
    })
    .await?;

    if let Some(scan_id) = scan_id.as_deref() {
        remember_completed_scan(scan_id, &result, connection_id.as_deref());
        finish_scan(scan_id, cancel.as_ref());
    }
    Ok(result)
}

/// Cancel an in-flight scan; `false` when it already finished.
#[tauri::command]
#[specta::specta]
pub async fn project_groups_cancel_nested_scan(
    args: ProjectGroupsCancelNestedScanArgs,
) -> Result<bool, BridgeError> {
    Ok(cancel_scan(&args.scan_id))
}

/// Import scanned nested repos as repos plus one root group in `group` mode.
#[tauri::command]
#[specta::specta]
pub async fn project_groups_import_nested(
    state: State<'_, AppState>,
    args: ProjectGroupsImportNestedArgs,
) -> Result<NestedRepoImportResult, BridgeError> {
    validate_import_args(&args)?;
    let scan = match completed_scan(
        args.scan_id.as_deref(),
        &args.parent_path,
        args.connection_id.as_deref(),
    ) {
        Some(scan) => scan,
        None => {
            let path = args.parent_path.clone();
            // Why: a fallback rescan validates the root exactly like
            // `scanNestedReposForIpc` does; a cached scan already validated it.
            if !Path::new(&path).is_absolute() {
                return Err(BridgeError::message("Repo path must be an absolute path"));
            }
            let options = normalize_nested_repo_scan_options(&serde_json::json!({
                "timeoutMs": IMPORT_NESTED_SCAN_TIMEOUT_MS,
            }));
            run_blocking(move || {
                Ok::<NestedRepoScanResult, BridgeError>(scan_nested_repos(
                    &path, &options, None, None,
                ))
            })
            .await?
        }
    };
    let result = {
        let mut projects = lock(&state.projects);
        import_nested(&mut projects, &state.fs, &args, &scan, now_ms())?
    };
    events::emit_repos_changed(&state.app);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("ade-bridge-groups-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }

        fn dir(&self, name: &str) -> PathBuf {
            let path = self.path.join(name);
            std::fs::create_dir_all(&path).expect("create dir");
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn store(dir: &TestDir) -> ProjectsStore {
        ProjectsStore::load(dir.file("projects.json"))
    }

    fn create_args(name: &str) -> ProjectGroupsCreateArgs {
        ProjectGroupsCreateArgs {
            name: name.to_string(),
            parent_path: None,
            connection_id: None,
            parent_group_id: None,
            created_from: None,
        }
    }

    fn scan_options(
        max_depth: u64,
        max_repos: u64,
        timeout_ms: Option<u64>,
    ) -> NestedRepoScanOptions {
        NestedRepoScanOptions {
            max_depth,
            max_repos,
            timeout_ms,
        }
    }

    #[test]
    fn create_uses_defaults_and_increments_tab_order() {
        let dir = TestDir::new("create");
        let mut store = store(&dir);
        let first = create_group(&mut store, &create_args("  First  "), 10).unwrap();
        assert_eq!(first["name"], "First");
        assert_eq!(first["parentPath"], Value::Null);
        assert_eq!(first["connectionId"], Value::Null);
        assert_eq!(first["parentGroupId"], Value::Null);
        assert_eq!(first["createdFrom"], "manual");
        assert_eq!(first["tabOrder"], 0);
        assert_eq!(first["isCollapsed"], false);
        assert_eq!(first["color"], Value::Null);
        assert_eq!(first["createdAt"], 10);
        assert_eq!(first["updatedAt"], 10);

        let second = create_group(&mut store, &create_args("   "), 20).unwrap();
        assert_eq!(second["name"], DEFAULT_PROJECT_GROUP_NAME);
        assert_eq!(second["tabOrder"], 1);
        assert_eq!(store.project_groups().len(), 2);
    }

    #[test]
    fn create_rejects_an_empty_name() {
        let dir = TestDir::new("create-empty");
        let mut store = store(&dir);
        let error = create_group(&mut store, &create_args(""), 1).unwrap_err();
        assert_eq!(error.to_string(), "invalid_project_group_create_args");
    }

    #[test]
    fn list_sorts_by_tab_order_then_name() {
        let dir = TestDir::new("list-sort");
        let mut store = store(&dir);
        store
            .mutate_groups(|groups| {
                groups.push(json!({ "id": "b", "name": "Beta", "tabOrder": 1 }));
                groups.push(json!({ "id": "a", "name": "Alpha", "tabOrder": 1 }));
                groups.push(json!({ "id": "z", "name": "Zeta", "tabOrder": 0 }));
            })
            .unwrap();
        let listed = list_groups(&store);
        assert_eq!(
            listed.iter().map(group_id).collect::<Vec<_>>(),
            vec!["z", "a", "b"]
        );
    }

    #[test]
    fn update_normalizes_name_and_validates_types() {
        let dir = TestDir::new("update");
        let mut store = store(&dir);
        let group = create_group(&mut store, &create_args("Original"), 1).unwrap();
        let id = group_id(&group).to_string();

        let updated = update_group(
            &mut store,
            &id,
            &json!({ "name": "  Renamed ", "isCollapsed": true, "tabOrder": 7, "color": "#abc" }),
            99,
        )
        .unwrap()
        .expect("group exists");
        assert_eq!(updated["name"], "Renamed");
        assert_eq!(updated["isCollapsed"], true);
        assert_eq!(updated["tabOrder"], 7);
        assert_eq!(updated["color"], "#abc");
        assert_eq!(updated["updatedAt"], 99);

        let blank = update_group(&mut store, &id, &json!({ "name": "   " }), 100)
            .unwrap()
            .expect("group exists");
        assert_eq!(blank["name"], "Renamed", "blank keeps the previous name");

        let cleared = update_group(&mut store, &id, &json!({ "color": 7 }), 101)
            .unwrap()
            .expect("group exists");
        assert_eq!(cleared["color"], Value::Null);

        assert_eq!(
            update_group(&mut store, &id, &json!({ "name": 7 }), 102)
                .unwrap_err()
                .to_string(),
            "invalid_project_group_update_args"
        );
        assert!(update_group(&mut store, "missing", &json!({}), 103)
            .unwrap()
            .is_none());
    }

    #[test]
    fn delete_removes_the_subtree_and_ungroups_repos() {
        let dir = TestDir::new("delete");
        let fs = FsService::new();
        let mut store = store(&dir);
        let root = create_group(&mut store, &create_args("Root"), 1).unwrap();
        let child = create_group(&mut store, &create_args("Child"), 2).unwrap();
        let other = create_group(&mut store, &create_args("Other"), 3).unwrap();
        let root_id = group_id(&root).to_string();
        let child_id = group_id(&child).to_string();
        store
            .mutate_groups(|groups| {
                groups
                    .iter_mut()
                    .find(|group| group_id(group) == child_id)
                    .unwrap()["parentGroupId"] = json!(root_id);
            })
            .unwrap();
        let folder = dir.dir("folder");
        store
            .mutate_repos(|repos| {
                repos.push(json!({ "id": "r1", "path": "/a", "projectGroupId": root_id }));
                repos.push(json!({ "id": "r2", "path": "/b", "projectGroupId": child_id }));
                repos.push(json!({ "id": "r3", "path": "/c", "projectGroupId": group_id(&other) }));
            })
            .unwrap();
        store
            .mutate_folder_workspaces(|workspaces| {
                workspaces.push(json!({
                    "id": "w1",
                    "projectGroupId": root_id,
                    "folderPath": folder.to_str().unwrap()
                }));
            })
            .unwrap();
        fs.authorize_root(folder.to_str().unwrap()).unwrap();

        assert!(delete_group(&mut store, &fs, &root_id).unwrap());
        assert_eq!(list_groups(&store).len(), 1, "other group survives");
        let repos = store.repos();
        assert_eq!(repos.len(), 3);
        assert_eq!(repos[0]["projectGroupId"], Value::Null);
        assert_eq!(repos[1]["projectGroupId"], Value::Null);
        assert_eq!(repos[2]["projectGroupId"], group_id(&other));
        assert!(store.folder_workspaces().is_empty());
        assert!(matches!(
            fs.resolve(folder.to_str().unwrap()),
            Err(ade_fs::FsError::PathAccessDenied)
        ));
        assert!(!delete_group(&mut store, &fs, &root_id).unwrap());
    }

    #[test]
    fn move_project_sets_group_and_order() {
        let dir = TestDir::new("move");
        let mut store = store(&dir);
        let group = create_group(&mut store, &create_args("Group"), 1).unwrap();
        let group_id_value = group_id(&group).to_string();
        store
            .mutate_repos(|repos| {
                repos.push(json!({ "id": "r1", "path": "/a" }));
                repos.push(json!({ "id": "r2", "path": "/b", "projectGroupId": group_id_value }));
            })
            .unwrap();

        let moved = move_project_to_group(&mut store, "r1", Some(&group_id_value), None)
            .unwrap()
            .expect("repo exists");
        assert_eq!(moved["projectGroupId"], group_id_value);
        assert_eq!(
            moved["projectGroupOrder"], 0,
            "r2 has no order yet, so the next bucket rank is 0"
        );

        let explicit = move_project_to_group(&mut store, "r2", Some("missing"), Some(4.0))
            .unwrap()
            .expect("repo exists");
        assert_eq!(explicit["projectGroupId"], Value::Null);
        assert_eq!(explicit["projectGroupOrder"], 4);

        assert!(
            move_project_to_group(&mut store, "nope", Some(&group_id_value), None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn has_git_marker_accepts_directories_files_and_bare_layouts() {
        let dir = TestDir::new("markers");
        let with_dir = dir.dir("with-dir");
        std::fs::create_dir_all(with_dir.join(".git")).unwrap();
        assert!(has_git_marker(&with_dir));

        let with_file = dir.dir("with-file");
        std::fs::write(with_file.join(".git"), "gitdir: /elsewhere\n").unwrap();
        assert!(has_git_marker(&with_file));

        let bare = dir.dir("bare");
        std::fs::create_dir_all(bare.join("objects")).unwrap();
        std::fs::create_dir_all(bare.join("refs")).unwrap();
        std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert!(has_git_marker(&bare));

        let partial = dir.dir("partial");
        std::fs::create_dir_all(partial.join("objects")).unwrap();
        std::fs::write(partial.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert!(!has_git_marker(&partial));
        assert!(!has_git_marker(&dir.dir("plain")));
    }

    #[test]
    fn scan_finds_markers_and_skips_ignored_directories() {
        let dir = TestDir::new("scan");
        let root = dir.dir("root");
        std::fs::create_dir_all(root.join("service").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("node_modules").join("dep").join(".git")).unwrap();
        // Hidden directories are pruned below the scan root (depth > 0).
        std::fs::create_dir_all(root.join("plain").join(".hidden").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("plain").join("nested").join(".git")).unwrap();

        let result = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 100, None),
            None,
            None,
        );
        assert_eq!(
            result.selected_path_kind,
            NestedRepoSelectedPathKind::NonGitFolder
        );
        assert!(!result.truncated);
        assert!(!result.timed_out);
        assert!(!result.stopped);
        assert_eq!(
            result
                .repos
                .iter()
                .map(|repo| repo.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                root.join("service").to_str().unwrap(),
                root.join("plain").join("nested").to_str().unwrap(),
            ]
        );
        assert_eq!(result.repos[0].display_name, "service");
        assert_eq!(result.repos[0].depth, 1);
        assert_eq!(result.repos[1].display_name, "nested");
        assert_eq!(result.repos[1].depth, 2);
    }

    #[test]
    fn scan_reports_a_git_selected_path() {
        let dir = TestDir::new("scan-selected");
        let root = dir.dir("root");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let result = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 100, None),
            None,
            None,
        );
        assert_eq!(
            result.selected_path_kind,
            NestedRepoSelectedPathKind::GitRepo
        );
        assert!(result.repos.is_empty());
    }

    #[test]
    fn scan_truncates_at_max_repos() {
        let dir = TestDir::new("scan-caps");
        let root = dir.dir("root");
        for name in ["a", "b", "c", "d"] {
            std::fs::create_dir_all(root.join(name).join(".git")).unwrap();
        }
        let result = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 2, None),
            None,
            None,
        );
        assert!(result.truncated);
        assert_eq!(result.repos.len(), 2);
        assert_eq!(result.repos[0].display_name, "a");
        assert_eq!(result.repos[1].display_name, "b");
    }

    #[test]
    fn scan_respects_max_depth() {
        let dir = TestDir::new("scan-depth");
        let root = dir.dir("root");
        std::fs::create_dir_all(root.join("one").join("two").join("three").join(".git")).unwrap();
        // maxDepth bounds traversal, not marker detection: at 1 the `two`
        // folder is never entered, so the depth-3 repo stays hidden.
        let shallow = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(1, 100, None),
            None,
            None,
        );
        assert!(shallow.repos.is_empty());
        let deep = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(2, 100, None),
            None,
            None,
        );
        assert_eq!(deep.repos.len(), 1);
        assert_eq!(deep.repos[0].display_name, "three");
        assert_eq!(deep.repos[0].depth, 3);
    }

    #[test]
    fn scan_cancel_returns_what_was_found() {
        let dir = TestDir::new("scan-cancel");
        let root = dir.dir("root");
        std::fs::create_dir_all(root.join("a").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("b").join(".git")).unwrap();
        let cancel = AtomicBool::new(false);
        let mut seen = 0;
        let result = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 100, None),
            Some(&cancel),
            Some(&mut |_: &NestedRepoScanResult, _| {
                seen += 1;
                if seen == 1 {
                    cancel.store(true, Ordering::SeqCst);
                }
            }),
        );
        assert!(result.stopped);
        assert_eq!(result.repos.len(), 1);
        assert_eq!(result.repos[0].display_name, "a");

        let pre_cancelled = AtomicBool::new(true);
        let result = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 100, None),
            Some(&pre_cancelled),
            None,
        );
        assert!(result.stopped);
        assert!(result.repos.is_empty());
    }

    #[test]
    fn scan_times_out_when_the_clock_exceeds_the_limit() {
        let dir = TestDir::new("scan-timeout");
        let root = dir.dir("root");
        std::fs::create_dir_all(root.join("a").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("b").join(".git")).unwrap();
        let result = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 100, Some(500)),
            None,
            Some(&mut |_: &NestedRepoScanResult, _| {
                std::thread::sleep(std::time::Duration::from_millis(600));
            }),
        );
        assert!(result.timed_out);
        assert_eq!(result.repos.len(), 1);
        assert_eq!(result.repos[0].display_name, "a");
        assert_eq!(result.timeout_ms, Some(500));

        let no_timeout = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 100, None),
            None,
            None,
        );
        assert!(!no_timeout.timed_out);
        assert_eq!(no_timeout.timeout_ms, None);
    }

    #[test]
    fn scan_progress_reports_scanned_and_found() {
        let dir = TestDir::new("scan-progress");
        let root = dir.dir("root");
        std::fs::create_dir_all(root.join("a").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("b").join(".git")).unwrap();
        let mut progress: Vec<(u64, u64)> = Vec::new();
        let result = scan_nested_repos(
            root.to_str().unwrap(),
            &scan_options(3, 100, None),
            None,
            Some(&mut |scan: &NestedRepoScanResult, scanned: u64| {
                progress.push((scanned, scan.repos.len() as u64));
            }),
        );
        assert_eq!(result.repos.len(), 2);
        assert_eq!(progress, vec![(1, 1), (1, 2)]);
    }

    #[test]
    fn import_selection_rejects_paths_outside_the_scan() {
        let scan = NestedRepoScanResult {
            selected_path: "/workspace".to_string(),
            selected_path_kind: NestedRepoSelectedPathKind::NonGitFolder,
            repos: vec![NestedRepoCandidate {
                path: "/workspace/api".to_string(),
                display_name: "api".to_string(),
                depth: 1,
            }],
            truncated: false,
            timed_out: false,
            stopped: false,
            duration_ms: 1,
            max_depth: 3,
            max_repos: 100,
            timeout_ms: None,
        };
        let (selected, rejected) = resolve_import_selection(
            &scan,
            &[
                "/workspace/api".to_string(),
                "/workspace/api/".to_string(),
                "/elsewhere".to_string(),
            ],
        );
        assert_eq!(selected, vec!["/workspace/api".to_string()]);
        assert_eq!(rejected, vec!["/elsewhere".to_string()]);
    }

    #[test]
    fn scan_args_validate_path_scan_id_and_ssh() {
        let base = ProjectGroupsScanNestedArgs {
            path: "/workspace".to_string(),
            connection_id: None,
            scan_id: None,
            options: None,
        };
        let options = validate_scan_args(&ProjectGroupsScanNestedArgs {
            scan_id: Some("scan-args".to_string()),
            options: Some(Json::new(json!({ "maxDepth": 2 }))),
            ..base.clone()
        })
        .unwrap();
        assert_eq!(options.max_depth, 2);
        assert_eq!(options.max_repos, 100);

        assert_eq!(
            validate_scan_args(&ProjectGroupsScanNestedArgs {
                path: String::new(),
                ..base.clone()
            })
            .unwrap_err()
            .to_string(),
            "invalid_project_group_scan_nested_args"
        );
        assert_eq!(
            validate_scan_args(&ProjectGroupsScanNestedArgs {
                scan_id: Some(String::new()),
                ..base.clone()
            })
            .unwrap_err()
            .to_string(),
            "invalid_project_group_scan_nested_args"
        );
        assert_eq!(
            validate_scan_args(&ProjectGroupsScanNestedArgs {
                path: "relative/path".to_string(),
                ..base.clone()
            })
            .unwrap_err()
            .to_string(),
            "Repo path must be an absolute path"
        );
        assert_eq!(
            validate_scan_args(&ProjectGroupsScanNestedArgs {
                connection_id: Some("box".to_string()),
                ..base
            })
            .unwrap_err()
            .to_string(),
            "ssh_connection_unavailable"
        );
    }

    #[test]
    fn import_args_validate_mode_scan_id_and_ssh() {
        let base = ProjectGroupsImportNestedArgs {
            parent_path: "/workspace".to_string(),
            group_name: None,
            project_paths: Vec::new(),
            connection_id: None,
            scan_id: None,
            mode: "group".to_string(),
        };
        assert!(validate_import_args(&base).is_ok());
        assert!(validate_import_args(&ProjectGroupsImportNestedArgs {
            mode: "separate".to_string(),
            ..base.clone()
        })
        .is_ok());
        for invalid in [
            ProjectGroupsImportNestedArgs {
                parent_path: String::new(),
                ..base.clone()
            },
            ProjectGroupsImportNestedArgs {
                mode: "nonsense".to_string(),
                ..base.clone()
            },
            ProjectGroupsImportNestedArgs {
                scan_id: Some(String::new()),
                ..base.clone()
            },
            ProjectGroupsImportNestedArgs {
                connection_id: Some("box".to_string()),
                ..base.clone()
            },
        ] {
            assert!(
                validate_import_args(&invalid).is_err(),
                "expected rejection for {invalid:?}"
            );
        }
        assert_eq!(
            validate_import_args(&ProjectGroupsImportNestedArgs {
                connection_id: Some("box".to_string()),
                ..base
            })
            .unwrap_err()
            .to_string(),
            "ssh_connection_unavailable"
        );
    }

    #[test]
    fn completed_scans_are_scoped_by_id_parent_and_connection() {
        let scan = NestedRepoScanResult {
            selected_path: "/workspace".to_string(),
            selected_path_kind: NestedRepoSelectedPathKind::NonGitFolder,
            repos: Vec::new(),
            truncated: false,
            timed_out: false,
            stopped: false,
            duration_ms: 1,
            max_depth: 3,
            max_repos: 100,
            timeout_ms: None,
        };
        remember_completed_scan("scan-cache", &scan, None);
        assert!(completed_scan(Some("scan-cache"), "/workspace", None).is_some());
        assert!(completed_scan(Some("scan-cache"), "/workspace/", None).is_some());
        assert!(completed_scan(Some("scan-cache"), "/elsewhere", None).is_none());
        assert!(completed_scan(Some("scan-cache"), "/workspace", Some("box")).is_none());
        assert!(completed_scan(None, "/workspace", None).is_none());
        assert!(completed_scan(Some("scan-missing"), "/workspace", None).is_none());
    }

    #[test]
    fn scan_and_cancel_registries_track_in_flight_scans() {
        let flag = register_scan("scan-1");
        assert!(!flag.load(Ordering::SeqCst));
        assert!(cancel_scan("scan-1"));
        assert!(flag.load(Ordering::SeqCst));
        assert!(!cancel_scan("scan-2"));
        finish_scan("scan-1", Some(&flag));
        assert!(!cancel_scan("scan-1"));

        let first = register_scan("scan-3");
        let second = register_scan("scan-3");
        assert!(
            first.load(Ordering::SeqCst),
            "re-registering cancels the old scan"
        );
        assert!(!second.load(Ordering::SeqCst));
        finish_scan("scan-3", Some(&first));
        assert!(
            cancel_scan("scan-3"),
            "a stale finish must not clear the newer registration"
        );
        finish_scan("scan-3", Some(&second));
        assert!(!cancel_scan("scan-3"));
    }
}
