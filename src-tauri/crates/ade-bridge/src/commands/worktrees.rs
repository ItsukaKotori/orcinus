use ade_core::models::repo::RepoKind;
use ade_core::models::worktree::{Worktree, DEFAULT_WORKSPACE_STATUS};
use ade_core::path_compare::normalize_for_comparison;
use ade_fs::FsService;
use ade_git::branch::{
    build_branch_name, compute_worktree_path, resolve_create_base, resolve_git_username,
    sanitize_worktree_name, select_branch_prefix_input,
};
use ade_git::worktree_create::{
    configure_branch_base, ensure_push_auto_setup_remote, worktree_add, AddWorktreeRequest,
};
use ade_git::worktree_remove::{
    assert_worktree_removable, delete_branch, force_delete_branch, worktree_remove,
    BranchDeleteOutcome,
};
use ade_store::projects_store::ProjectsStore;
use ade_store::WorktreeMetaStore;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::State;

use crate::commands::project_groups::revoke_root_if_unused;
use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::events;
use crate::json::Json;
use crate::state::{lock, AppState};

/// Mirrors `WORKTREE_CREATE_MAX_SUFFIX_ATTEMPTS`
/// (`orca:src/main/worktree-create-candidates.ts:8`).
const WORKTREE_CREATE_MAX_SUFFIX_ATTEMPTS: u32 = 100;

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesListArgs {
    pub repo_id: String,
}

pub fn repo_id(repo: &Value) -> &str {
    repo.get("id").and_then(Value::as_str).unwrap_or_default()
}

pub fn repo_path(repo: &Value) -> &str {
    repo.get("path").and_then(Value::as_str).unwrap_or_default()
}

pub fn repo_display_name(repo: &Value) -> &str {
    repo.get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Unknown or absent kinds are git (oracle `getRepoKind`).
pub fn repo_kind(repo: &Value) -> RepoKind {
    repo.get("kind")
        .and_then(Value::as_str)
        .and_then(RepoKind::parse)
        .unwrap_or(RepoKind::Git)
}

/// Merge persisted worktree metadata into a projection row (spec §3.4).
///
/// Only the whitelisted user-facing keys are honored; everything else the
/// renderer persists stays invisible until a later phase defines it. A
/// persisted `displayName` is an explicit label, so the row reports the
/// `custom` display-name mode instead of the automatic one.
pub fn apply_worktree_meta(worktree: &mut Worktree, meta: Option<&Value>) {
    let Some(meta) = meta.and_then(Value::as_object) else {
        return;
    };
    if let Some(display_name) = meta.get("displayName").and_then(Value::as_str) {
        if !display_name.is_empty() {
            worktree.display_name = display_name.to_string();
            worktree.display_name_mode = "custom".to_string();
        }
    }
    if let Some(comment) = meta.get("comment").and_then(Value::as_str) {
        worktree.comment = comment.to_string();
    }
    if let Some(linked_issue) = meta.get("linkedIssue").and_then(Value::as_u64) {
        worktree.linked_issue = Some(linked_issue);
    }
    if let Some(linked_pr) = meta.get("linkedPR").and_then(Value::as_u64) {
        worktree.linked_pr = Some(linked_pr);
    }
    if let Some(linked_linear_issue) = meta.get("linkedLinearIssue").and_then(Value::as_str) {
        worktree.linked_linear_issue = Some(linked_linear_issue.to_string());
    }
    for (field, target) in [
        ("isArchived", &mut worktree.is_archived),
        ("isUnread", &mut worktree.is_unread),
        ("isPinned", &mut worktree.is_pinned),
    ] {
        if let Some(flag) = meta.get(field).and_then(Value::as_bool) {
            *target = flag;
        }
    }
    if let Some(sort_order) = meta.get("sortOrder").and_then(Value::as_u64) {
        worktree.sort_order = sort_order;
    }
    if let Some(last_activity_at) = meta.get("lastActivityAt").and_then(Value::as_u64) {
        worktree.last_activity_at = last_activity_at;
    }
    if let Some(status) = meta.get("workspaceStatus").and_then(Value::as_str) {
        if !status.is_empty() {
            worktree.workspace_status = status.to_string();
        }
    }
}

/// Map one `git worktree list` entry (spec §5.3) with its persisted metadata.
pub fn git_worktree(
    repo: &Value,
    entry: &ade_git::GitWorktreeEntry,
    meta: Option<&Value>,
) -> Worktree {
    let branch = entry.branch.clone().unwrap_or_default();
    let mut worktree = Worktree::for_git_entry(
        repo_id(repo),
        repo_display_name(repo),
        &entry.path,
        &entry.head,
        &branch,
        entry.is_bare,
        entry.is_main_worktree,
    );
    apply_worktree_meta(&mut worktree, meta);
    worktree
}

/// A folder workspace belongs to a repo only when both carry the same project
/// group. An ungrouped repo (absent/null `projectGroupId`) owns just its root
/// workspace, so a null group never pulls in other repos' workspaces.
fn folder_workspace_in_scope(repo: &Value, workspace: &Value) -> bool {
    match repo.get("projectGroupId").and_then(Value::as_str) {
        Some(group) => workspace.get("projectGroupId").and_then(Value::as_str) == Some(group),
        None => false,
    }
}

fn folder_worktree(repo: &Value, workspace: &Value) -> Worktree {
    let path = workspace
        .get("folderPath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = workspace
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    // Why: a blank name falls back to the repo label, matching
    // `mergeFolderWorkspace`'s `meta.displayName || repo.displayName`.
    let display_name = if name.is_empty() {
        repo_display_name(repo)
    } else {
        name
    };
    let mut worktree = Worktree::for_folder_workspace(repo_id(repo), display_name, path, false);
    if let Some(comment) = workspace.get("comment").and_then(Value::as_str) {
        worktree.comment = comment.to_string();
    }
    for (field, target) in [
        ("isArchived", &mut worktree.is_archived),
        ("isUnread", &mut worktree.is_unread),
        ("isPinned", &mut worktree.is_pinned),
    ] {
        if let Some(flag) = workspace.get(field).and_then(Value::as_bool) {
            *target = flag;
        }
    }
    if let Some(sort_order) = workspace.get("sortOrder").and_then(Value::as_u64) {
        worktree.sort_order = sort_order;
    }
    if let Some(last_activity_at) = workspace.get("lastActivityAt").and_then(Value::as_u64) {
        worktree.last_activity_at = last_activity_at;
    }
    if let Some(status) = workspace.get("workspaceStatus").and_then(Value::as_str) {
        if !status.is_empty() {
            worktree.workspace_status = status.to_string();
        }
    }
    worktree
}

/// Folder repos project their own root workspace first, then the repo's own
/// folder workspaces by `lastActivityAt` descending (spec §5.3).
fn folder_worktrees(repo: &Value, folder_workspaces: &[Value]) -> Vec<Worktree> {
    let main = Worktree::for_folder_workspace(
        repo_id(repo),
        repo_display_name(repo),
        repo_path(repo),
        true,
    );
    // Why normalized: the repo path may keep a `/folder/` spelling while a
    // folder workspace points at the same directory; raw id comparison would
    // then emit the root row twice.
    let main_path_key = normalize_for_comparison(&main.path);
    let mut extras: Vec<Worktree> = folder_workspaces
        .iter()
        .filter(|workspace| folder_workspace_in_scope(repo, workspace))
        .map(|workspace| folder_worktree(repo, workspace))
        .filter(|worktree| {
            !worktree.path.is_empty() && normalize_for_comparison(&worktree.path) != main_path_key
        })
        .collect();
    extras.sort_by_key(|worktree| std::cmp::Reverse(worktree.last_activity_at));
    let mut worktrees = Vec::with_capacity(extras.len() + 1);
    worktrees.push(main);
    worktrees.extend(extras);
    worktrees
}

/// Authorize every returned worktree path so linked worktrees (siblings of the
/// repo root) are reachable in the file tree. A failed grant is logged, not
/// fatal, mirroring the persisted-root re-authorization at startup.
fn authorize_worktree_paths(fs: &FsService, worktrees: &[Worktree]) {
    for worktree in worktrees {
        if let Err(error) = fs.authorize_root(&worktree.path) {
            eprintln!(
                "[ade-bridge] failed to authorize worktree root '{}': {error}",
                worktree.path
            );
        }
    }
}

/// `worktrees.list({repoId})`: git repos shell out to `git worktree list`
/// (prunable entries are already dropped by the porcelain parser); folder repos
/// project their workspaces. Persisted metadata (`meta_items`, keyed by
/// worktree id) merges into git rows. An unknown repo lists nothing.
pub fn list_worktrees(
    repo: &Value,
    folder_workspaces: &[Value],
    meta_items: &Map<String, Value>,
    fs: &FsService,
) -> Result<Vec<Worktree>, BridgeError> {
    let worktrees = match repo_kind(repo) {
        RepoKind::Folder => folder_worktrees(repo, folder_workspaces),
        RepoKind::Git => {
            let entries = ade_git::worktree_list(repo_path(repo))?;
            entries
                .iter()
                .map(|entry| {
                    let id = ade_core::ids::worktree_id(repo_id(repo), &entry.path);
                    git_worktree(repo, entry, meta_items.get(&id))
                })
                .collect()
        }
    };
    authorize_worktree_paths(fs, &worktrees);
    Ok(worktrees)
}

/// `worktrees.listAll()`: every repo's projection, in registry order.
pub fn list_all_worktrees(
    repos: &[Value],
    folder_workspaces: &[Value],
    meta_items: &Map<String, Value>,
    fs: &FsService,
) -> Result<Vec<Worktree>, BridgeError> {
    let mut worktrees = Vec::new();
    for repo in repos {
        worktrees.extend(list_worktrees(repo, folder_workspaces, meta_items, fs)?);
    }
    Ok(worktrees)
}

/// Project the worktrees of one repo.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_list(
    state: State<'_, AppState>,
    args: WorktreesListArgs,
) -> Result<Vec<Worktree>, BridgeError> {
    let (repo, folder_workspaces, meta_items) = {
        let projects = lock(&state.projects);
        let repo = projects
            .repos()
            .into_iter()
            .find(|repo| repo_id(repo) == args.repo_id);
        (
            repo,
            projects.folder_workspaces(),
            state.worktree_meta_store().items(),
        )
    };
    let Some(repo) = repo else {
        return Ok(Vec::new());
    };
    let fs = state.fs.clone();
    run_blocking(move || list_worktrees(&repo, &folder_workspaces, &meta_items, &fs)).await
}

/// Project every repo's worktrees, merged in registry order.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_list_all(
    state: State<'_, AppState>,
) -> Result<Vec<Worktree>, BridgeError> {
    let (repos, folder_workspaces, meta_items) = {
        let projects = lock(&state.projects);
        (
            projects.repos(),
            projects.folder_workspaces(),
            state.worktree_meta_store().items(),
        )
    };
    let fs = state.fs.clone();
    run_blocking(move || list_all_worktrees(&repos, &folder_workspaces, &meta_items, &fs)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesCreateArgs {
    pub repo_id: String,
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub branch_name_override: Option<String>,
    #[serde(default)]
    pub workspace_status: Option<String>,
    #[serde(default)]
    pub manual_order: Option<u64>,
    #[serde(default)]
    pub created_with_agent: Option<String>,
}

/// `{ worktree, warnings? }` (spec §4.4): `warnings` carries the non-fatal
/// follow-up failures that the oracle only logs.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesCreateResult {
    pub worktree: Worktree,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesRemoveArgs {
    pub worktree_id: String,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub force: Option<bool>,
    #[serde(default)]
    pub allow_unverified_pty_stop: Option<bool>,
    #[serde(default)]
    pub skip_archive: Option<bool>,
    #[serde(default)]
    pub snapshot_prune_batch_id: Option<String>,
}

/// `{ preservedBranch?: { branchName, head? } }` (oracle `RemoveWorktreeResult`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesRemoveResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserved_branch: Option<PreservedBranch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PreservedBranch {
    pub branch_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesForgetLocalArgs {
    pub worktree_id: String,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub snapshot_prune_batch_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesForceDeleteArgs {
    pub worktree_id: String,
    pub branch_name: String,
    pub expected_head: String,
    #[serde(default)]
    pub host_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesForceDeleteResult {
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesUpdateMetaArgs {
    pub worktree_id: String,
    #[serde(default)]
    pub execution_host_id: Option<String>,
    pub updates: Json,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreesPersistSortOrderArgs {
    pub ordered_ids: Vec<String>,
}

/// Split `"{repoId}::{path}"` at the first separator; the repo id cannot
/// contain `::`, so the remainder is the path.
fn split_worktree_id(worktree_id: &str) -> Result<(&str, &str), BridgeError> {
    worktree_id
        .split_once("::")
        .ok_or_else(|| BridgeError::message(format!("Invalid worktreeId: {worktree_id}")))
}

/// `git worktree add` conflicts the suffix loop retries: an occupied path or an
/// existing branch, both reported as "already exists" by git or by
/// [`worktree_add`]'s pre-check.
fn is_worktree_name_conflict(message: &str) -> bool {
    message.to_lowercase().contains("already exists")
}

/// `getWorktreeCreateCandidate` (`orca:src/main/worktree-create-candidates.ts:10`):
/// suffix 1 keeps the base name, later suffixes append `-{suffix}`.
fn worktree_create_candidate(value: &str, suffix: u32) -> String {
    if suffix == 1 {
        value.to_string()
    } else {
        format!("{value}-{suffix}")
    }
}

/// One created worktree plus the warn-only follow-up failures
/// (`branch.<name>.base` / `push.autoSetupRemote`), mirroring the oracle's
/// `console.warn` side effects.
pub struct CreateWorktreeOutcome {
    pub worktree: Worktree,
    pub warnings: Vec<String>,
}

/// Testable `worktrees.create` orchestration returning just the projection row.
pub fn create_worktree_impl(
    repo: &Value,
    settings: &Value,
    meta: &WorktreeMetaStore,
    fs: &FsService,
    args: &WorktreesCreateArgs,
) -> Result<Worktree, BridgeError> {
    create_worktree_outcome(repo, settings, meta, fs, args).map(|outcome| outcome.worktree)
}

/// `worktrees.create` core (oracle `createLocalWorktree`,
/// `orca:src/main/ipc/worktree-remote.ts:2301`): resolve the branch name and
/// base ref, retry name conflicts with `-2..-100` suffixes, then persist
/// metadata and authorize the new checkout. Follow-up config failures are
/// warnings, never create failures.
pub fn create_worktree_outcome(
    repo: &Value,
    settings: &Value,
    meta: &WorktreeMetaStore,
    fs: &FsService,
    args: &WorktreesCreateArgs,
) -> Result<CreateWorktreeOutcome, BridgeError> {
    if repo_kind(repo) != RepoKind::Git {
        return Err(BridgeError::message(
            "Worktrees can only be created for git repositories",
        ));
    }
    let repo_path_value = repo_path(repo);
    let name = sanitize_worktree_name(&args.name)?;

    let strategy = settings
        .get("branchPrefix")
        .and_then(Value::as_str)
        .unwrap_or("git-username");
    let custom = settings.get("branchPrefixCustom").and_then(Value::as_str);
    // Why: the username probe is a git subprocess; only the strategy that
    // consumes it pays for it.
    let username = if strategy == "git-username" {
        resolve_git_username(repo_path_value)
    } else {
        None
    };
    let prefix = select_branch_prefix_input(strategy, custom, username.as_deref());
    let branch_leaf = args
        .branch_name_override
        .clone()
        .unwrap_or_else(|| name.clone());

    let base_ref = resolve_create_base(
        repo_path_value,
        args.base_branch.as_deref(),
        repo.get("worktreeBaseRef").and_then(Value::as_str),
    )?;
    // Why `||`: an empty repo override falls back to the global workspace dir.
    let root = repo
        .get("worktreeBasePath")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| settings.get("workspaceDir").and_then(Value::as_str))
        .unwrap_or_default();
    let nest = settings
        .get("nestWorkspaces")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let repo_name = ade_core::models::repo::basename(repo_path_value);

    let mut created: Option<(String, String)> = None;
    for suffix in 1..=WORKTREE_CREATE_MAX_SUFFIX_ATTEMPTS {
        let candidate_name = worktree_create_candidate(&name, suffix);
        let candidate_branch_leaf = worktree_create_candidate(&branch_leaf, suffix);
        let branch = build_branch_name(prefix.as_deref(), &candidate_branch_leaf);
        let path = compute_worktree_path(root, &repo_name, nest, &candidate_name)?;
        let request = AddWorktreeRequest {
            repo_path: repo_path_value.to_string(),
            worktree_path: path.clone(),
            branch: branch.clone(),
            base_ref: base_ref.clone(),
        };
        match worktree_add(&request) {
            Ok(()) => {
                created = Some((path, branch));
                break;
            }
            Err(error) if is_worktree_name_conflict(&error.to_string()) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let Some((path, branch)) = created else {
        // Why: the fixed text deliberately avoids every retryable-conflict
        // pattern the renderer's `isRetryableWorktreeCreateConflict` matches, so
        // an exhausted suffix loop is not retried a second time in JS.
        return Err(BridgeError::message(
            "Worktree creation failed: name conflict could not be resolved",
        ));
    };

    let mut warnings = Vec::new();
    if let Err(error) = configure_branch_base(&path, &branch, &base_ref) {
        warnings.push(error.to_string());
    }
    if let Err(error) = ensure_push_auto_setup_remote(&path) {
        warnings.push(error.to_string());
    }
    for warning in &warnings {
        eprintln!("[ade-bridge] worktree create warning: {warning}");
    }

    let worktree_id = ade_core::ids::worktree_id(repo_id(repo), &path);
    let mut updates = Map::new();
    updates.insert(
        "displayName".to_string(),
        Value::String(args.display_name.clone().unwrap_or_else(|| name.clone())),
    );
    updates.insert(
        "workspaceStatus".to_string(),
        Value::String(
            args.workspace_status
                .clone()
                .unwrap_or_else(|| DEFAULT_WORKSPACE_STATUS.to_string()),
        ),
    );
    if let Some(agent) = &args.created_with_agent {
        updates.insert("createdWithAgent".to_string(), Value::String(agent.clone()));
    }
    if let Some(order) = args.manual_order {
        updates.insert("manualOrder".to_string(), Value::from(order));
    }
    meta.merge(&worktree_id, &Value::Object(updates))?;
    fs.authorize_root(&path)?;

    let worktree = list_worktrees(repo, &[], &meta.items(), fs)?
        .into_iter()
        .find(|worktree| worktree.id == worktree_id)
        .ok_or_else(|| {
            BridgeError::message(format!("Worktree not found after creation: {worktree_id}"))
        })?;
    Ok(CreateWorktreeOutcome { worktree, warnings })
}

/// `worktrees.remove` core: preflight, `git worktree remove`, then `-d` branch
/// cleanup (preserving an unmerged branch) and metadata cleanup. Root
/// revocation and `worktrees:changed` stay in the command layer.
pub fn remove_worktree_impl(
    repo: &Value,
    meta: &WorktreeMetaStore,
    fs: &FsService,
    worktree_id: &str,
    force: bool,
) -> Result<WorktreesRemoveResult, BridgeError> {
    let (_repo_id_value, path) = split_worktree_id(worktree_id)?;
    let entry = list_worktrees(repo, &[], &meta.items(), fs)?
        .into_iter()
        .find(|worktree| worktree.id == worktree_id)
        .ok_or_else(|| BridgeError::message(format!("Worktree not found: {path}")))?;

    let repo_path_value = repo_path(repo);
    assert_worktree_removable(repo_path_value, &entry.path, force)?;
    worktree_remove(repo_path_value, &entry.path, force)?;

    let preserve_branch = meta
        .get(worktree_id)
        .and_then(|entry| entry.get("preserveBranchOnDelete").and_then(Value::as_bool))
        .unwrap_or(false);
    let preserved_branch = if preserve_branch {
        None
    } else {
        match delete_branch(repo_path_value, &entry.branch, false)? {
            BranchDeleteOutcome::Preserved { branch_name, head } => {
                Some(PreservedBranch { branch_name, head })
            }
            BranchDeleteOutcome::Deleted | BranchDeleteOutcome::Skipped => None,
        }
    };
    meta.remove(worktree_id)?;
    Ok(WorktreesRemoveResult { preserved_branch })
}

/// `worktrees.forgetLocal`: metadata + authorization only; the checkout and its
/// git registration are left untouched.
pub fn forget_local_impl(
    store: &ProjectsStore,
    meta: &WorktreeMetaStore,
    fs: &FsService,
    worktree_id: &str,
) -> Result<WorktreesRemoveResult, BridgeError> {
    let (_repo_id_value, path) = split_worktree_id(worktree_id)?;
    meta.remove(worktree_id)?;
    revoke_root_if_unused(store, fs, path);
    Ok(WorktreesRemoveResult::default())
}

/// `worktrees.forceDeletePreservedBranch` core: compare-and-swap branch delete
/// with the head the removal preserved.
pub fn force_delete_preserved_branch_impl(
    repo: &Value,
    branch_name: &str,
    expected_head: &str,
) -> Result<(), BridgeError> {
    if repo_kind(repo) != RepoKind::Git {
        return Err(BridgeError::message(
            "Folder workspaces do not have local Git branches.",
        ));
    }
    force_delete_branch(repo_path(repo), branch_name, expected_head)?;
    Ok(())
}

/// `worktrees.updateMeta` core: whitelist merge into `worktrees.json`, then the
/// merged projection row. A worktree that no longer exists answers an error
/// rather than a fabricated row.
pub fn update_meta_impl(
    repo: &Value,
    folder_workspaces: &[Value],
    meta: &WorktreeMetaStore,
    fs: &FsService,
    worktree_id: &str,
    updates: &Value,
) -> Result<Worktree, BridgeError> {
    meta.merge(worktree_id, updates)?;
    list_worktrees(repo, folder_workspaces, &meta.items(), fs)?
        .into_iter()
        .find(|worktree| worktree.id == worktree_id)
        .ok_or_else(|| BridgeError::message(format!("Worktree not found: {worktree_id}")))
}

/// `worktrees.persistSortOrder`: index assignment in `orderedIds` order.
pub fn persist_sort_order_impl(
    meta: &WorktreeMetaStore,
    ordered_ids: &[String],
) -> Result<(), BridgeError> {
    // Why: the oracle ignores an empty order list, leaving stored indexes as-is.
    if ordered_ids.is_empty() {
        return Ok(());
    }
    meta.persist_sort_order(ordered_ids)?;
    Ok(())
}

/// Create a worktree and persist its metadata; broadcasts `worktrees:changed`.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_create(
    state: State<'_, AppState>,
    args: WorktreesCreateArgs,
) -> Result<WorktreesCreateResult, BridgeError> {
    let repo = {
        let projects = lock(&state.projects);
        projects
            .repos()
            .into_iter()
            .find(|repo| repo_id(repo) == args.repo_id)
    };
    let Some(repo) = repo else {
        return Err(BridgeError::message(format!(
            "Repo not found: {}",
            args.repo_id
        )));
    };
    let settings = state.settings_store().get();
    let meta = state.worktree_meta_store();
    let fs = state.fs.clone();
    let outcome =
        run_blocking(move || create_worktree_outcome(&repo, &settings, &meta, &fs, &args)).await?;
    events::emit_worktrees_changed(&state.app, &outcome.worktree.repo_id);
    Ok(WorktreesCreateResult {
        worktree: outcome.worktree,
        warnings: (!outcome.warnings.is_empty()).then_some(outcome.warnings),
    })
}

/// Remove a worktree, revoke its root when unused, and broadcast the change.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_remove(
    state: State<'_, AppState>,
    args: WorktreesRemoveArgs,
) -> Result<WorktreesRemoveResult, BridgeError> {
    let (repo_id_value, path) = split_worktree_id(&args.worktree_id)?;
    let repo = {
        let projects = lock(&state.projects);
        projects
            .repos()
            .into_iter()
            .find(|repo| repo_id(repo) == repo_id_value)
    };
    let Some(repo) = repo else {
        return Err(BridgeError::message(format!(
            "Repo not found: {repo_id_value}"
        )));
    };
    let meta = state.worktree_meta_store();
    let fs = state.fs.clone();
    let worktree_id = args.worktree_id.clone();
    let force = args.force.unwrap_or(false);
    let impl_fs = fs.clone();
    let result =
        run_blocking(move || remove_worktree_impl(&repo, &meta, &impl_fs, &worktree_id, force))
            .await?;
    {
        let projects = lock(&state.projects);
        revoke_root_if_unused(&projects, &fs, path);
    }
    events::emit_worktrees_changed(&state.app, repo_id_value);
    Ok(result)
}

/// Drop a workspace's metadata and authorization without touching disk or git.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_forget_local(
    state: State<'_, AppState>,
    args: WorktreesForgetLocalArgs,
) -> Result<WorktreesRemoveResult, BridgeError> {
    let (repo_id_value, _path) = split_worktree_id(&args.worktree_id)?;
    let result = {
        let projects = lock(&state.projects);
        forget_local_impl(
            &projects,
            &state.worktree_meta_store(),
            &state.fs,
            &args.worktree_id,
        )?
    };
    events::emit_worktrees_changed(&state.app, repo_id_value);
    Ok(result)
}

/// Force-delete a branch a removal preserved; broadcasts `worktrees:changed`.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_force_delete_preserved_branch(
    state: State<'_, AppState>,
    args: WorktreesForceDeleteArgs,
) -> Result<WorktreesForceDeleteResult, BridgeError> {
    let (repo_id_value, _path) = split_worktree_id(&args.worktree_id)?;
    let repo = {
        let projects = lock(&state.projects);
        projects
            .repos()
            .into_iter()
            .find(|repo| repo_id(repo) == repo_id_value)
    };
    let Some(repo) = repo else {
        return Err(BridgeError::message(format!(
            "Repo not found: {repo_id_value}"
        )));
    };
    let branch_name = args.branch_name.clone();
    let expected_head = args.expected_head.clone();
    run_blocking(move || force_delete_preserved_branch_impl(&repo, &branch_name, &expected_head))
        .await?;
    events::emit_worktrees_changed(&state.app, repo_id_value);
    Ok(WorktreesForceDeleteResult { deleted: true })
}

/// Merge whitelisted metadata and return the merged projection row. No event:
/// the renderer applies this update optimistically.
#[tauri::command]
#[specta::specta]
pub async fn worktrees_update_meta(
    state: State<'_, AppState>,
    args: WorktreesUpdateMetaArgs,
) -> Result<Worktree, BridgeError> {
    let (repo_id_value, _path) = split_worktree_id(&args.worktree_id)?;
    let (repo, folder_workspaces) = {
        let projects = lock(&state.projects);
        (
            projects
                .repos()
                .into_iter()
                .find(|repo| repo_id(repo) == repo_id_value),
            projects.folder_workspaces(),
        )
    };
    let Some(repo) = repo else {
        return Err(BridgeError::message(format!(
            "Repo not found: {repo_id_value}"
        )));
    };
    let meta = state.worktree_meta_store();
    let fs = state.fs.clone();
    let worktree_id = args.worktree_id.clone();
    let updates = args.updates.0;
    run_blocking(move || {
        update_meta_impl(
            &repo,
            &folder_workspaces,
            &meta,
            &fs,
            &worktree_id,
            &updates,
        )
    })
    .await
}

/// Persist the manual worktree order (`sortOrder` = index in `orderedIds`).
#[tauri::command]
#[specta::specta]
pub async fn worktrees_persist_sort_order(
    state: State<'_, AppState>,
    args: WorktreesPersistSortOrderArgs,
) -> Result<(), BridgeError> {
    persist_sort_order_impl(&state.worktree_meta_store(), &args.ordered_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ade_git::GitWorktreeEntry;
    use serde_json::json;

    fn git_repo() -> Value {
        json!({ "id": "r1", "path": "/repo", "displayName": "Repo", "kind": "git" })
    }

    fn folder_repo() -> Value {
        json!({ "id": "f1", "path": "/folder", "displayName": "Folder", "kind": "folder" })
    }

    fn grouped_folder_repo(group: &str) -> Value {
        json!({
            "id": "f1",
            "path": "/folder",
            "displayName": "Folder",
            "kind": "folder",
            "projectGroupId": group
        })
    }

    #[test]
    fn kind_defaults_to_git_for_absent_or_unknown_values() {
        assert_eq!(repo_kind(&json!({ "id": "r" })), RepoKind::Git);
        assert_eq!(repo_kind(&json!({ "kind": "weird" })), RepoKind::Git);
        assert_eq!(repo_kind(&json!({ "kind": "folder" })), RepoKind::Folder);
    }

    #[test]
    fn git_entry_maps_branch_short_and_defaults() {
        let entry = GitWorktreeEntry {
            path: "/repo".to_string(),
            head: "abc".to_string(),
            branch: Some("refs/heads/main".to_string()),
            is_bare: false,
            is_main_worktree: true,
        };
        let worktree = git_worktree(&git_repo(), &entry, None);
        assert_eq!(worktree.id, "r1::/repo");
        assert_eq!(worktree.display_name, "main");
        assert_eq!(worktree.head, "abc");
        assert_eq!(worktree.branch, "refs/heads/main");
        assert!(worktree.is_main_worktree);
        assert_eq!(
            worktree.workspace_status,
            ade_core::models::worktree::DEFAULT_WORKSPACE_STATUS
        );
    }

    #[test]
    fn detached_entry_falls_back_to_repo_display_name() {
        let entry = GitWorktreeEntry {
            path: "/repo/wt".to_string(),
            head: "def".to_string(),
            branch: None,
            is_bare: false,
            is_main_worktree: false,
        };
        let worktree = git_worktree(&git_repo(), &entry, None);
        assert_eq!(worktree.display_name, "Repo");
        assert_eq!(worktree.branch, "");
        assert!(!worktree.is_main_worktree);
    }

    #[test]
    fn folder_repo_projects_main_workspace_without_git_metadata() {
        let worktrees = folder_worktrees(&folder_repo(), &[]);
        assert_eq!(worktrees.len(), 1);
        let main = &worktrees[0];
        assert_eq!(main.id, "f1::/folder");
        assert_eq!(main.path, "/folder");
        assert_eq!(main.display_name, "Folder");
        assert_eq!(main.head, "");
        assert_eq!(main.branch, "");
        assert!(main.is_main_worktree);
        assert!(!main.is_bare);
    }

    #[test]
    fn folder_workspaces_follow_main_sorted_by_last_activity_desc() {
        let workspaces = vec![
            json!({
                "id": "w1",
                "projectGroupId": "g1",
                "folderPath": "/folder/one",
                "name": "One",
                "lastActivityAt": 5
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g1",
                "folderPath": "/folder/two",
                "name": "Two",
                "lastActivityAt": 10,
                "isPinned": true,
                "comment": "hi",
                "workspaceStatus": "done"
            }),
        ];
        let worktrees = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(
            worktrees.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            vec!["f1::/folder", "f1::/folder/two", "f1::/folder/one"]
        );
        assert!(worktrees[0].is_main_worktree);
        assert!(!worktrees[1].is_main_worktree);
        assert_eq!(worktrees[1].display_name, "Two");
        assert_eq!(worktrees[1].last_activity_at, 10);
        assert!(worktrees[1].is_pinned);
        assert_eq!(worktrees[1].comment, "hi");
        assert_eq!(worktrees[1].workspace_status, "done");
        assert_eq!(worktrees[1].head, "");
        assert_eq!(worktrees[1].branch, "");
    }

    #[test]
    fn folder_workspaces_are_scoped_to_the_repo_group() {
        let workspaces = vec![
            json!({
                "id": "w1",
                "projectGroupId": "g1",
                "folderPath": "/folder/one",
                "name": "One",
                "lastActivityAt": 1
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g2",
                "folderPath": "/folder/two",
                "name": "Two",
                "lastActivityAt": 2
            }),
            json!({
                "id": "w3",
                "projectGroupId": null,
                "folderPath": "/folder/three",
                "name": "Three",
                "lastActivityAt": 3
            }),
        ];

        let first = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(
            first.iter().map(|w| w.path.as_str()).collect::<Vec<_>>(),
            vec!["/folder", "/folder/one"]
        );

        let mut second_repo = grouped_folder_repo("g2");
        second_repo["id"] = json!("f2");
        second_repo["path"] = json!("/other");
        let second = folder_worktrees(&second_repo, &workspaces);
        assert_eq!(
            second.iter().map(|w| w.path.as_str()).collect::<Vec<_>>(),
            vec!["/other", "/folder/two"]
        );
    }

    #[test]
    fn ungrouped_folder_repo_sees_only_its_root() {
        let workspaces = vec![
            json!({
                "id": "w1",
                "projectGroupId": null,
                "folderPath": "/folder/one",
                "name": "One",
                "lastActivityAt": 1
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g1",
                "folderPath": "/folder/two",
                "name": "Two",
                "lastActivityAt": 2
            }),
        ];
        let worktrees = folder_worktrees(&folder_repo(), &workspaces);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].id, "f1::/folder");
        assert!(worktrees[0].is_main_worktree);
    }

    #[test]
    fn folder_workspace_at_the_repo_root_is_not_duplicated() {
        let workspaces = vec![
            // Same directory as the root, spelled with a trailing separator.
            json!({
                "id": "w1",
                "projectGroupId": "g1",
                "folderPath": "/folder/",
                "name": "Root",
                "lastActivityAt": 1
            }),
            json!({
                "id": "w2",
                "projectGroupId": "g1",
                "folderPath": "/folder/child",
                "name": "Child",
                "lastActivityAt": 2
            }),
        ];
        let worktrees = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(
            worktrees.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            vec!["f1::/folder", "f1::/folder/child"]
        );
        assert_eq!(worktrees[0].display_name, "Folder");
    }

    #[test]
    fn blank_folder_workspace_name_falls_back_to_repo_display_name() {
        let workspaces = vec![json!({
            "id": "w1",
            "projectGroupId": "g1",
            "folderPath": "/folder/one",
            "name": "   ",
            "lastActivityAt": 1
        })];
        let worktrees = folder_worktrees(&grouped_folder_repo("g1"), &workspaces);
        assert_eq!(worktrees[1].display_name, "Folder");
    }

    #[test]
    fn list_all_merges_repos_in_registry_order() {
        let repos = vec![
            folder_repo(),
            json!({ "id": "f2", "path": "/other", "displayName": "Other", "kind": "folder" }),
        ];
        let worktrees = list_all_worktrees(&repos, &[], &Map::new(), &FsService::new()).unwrap();
        assert_eq!(
            worktrees.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            vec!["f1::/folder", "f2::/other"]
        );
    }
}
