//! The git command surface: thin, argument-mapped wrappers over `ade_git`.
//!
//! Every command takes its renderer payload as `{ args }` with camelCase
//! fields matching `src/shared/preload-api/api/git-inspection-api.ts` and
//! `git-operation-api.ts`; unknown TS fields are ignored by serde. Blocking
//! git subprocesses run through [`run_blocking`] so they never sit on the
//! async runtime threads.

use std::path::{Component, Path};
use std::time::Duration;

use ade_fs::FsService;
use ade_git::compare::{GitBranchCompareResult, GitCommitCompareResult};
use ade_git::diff::GitDiffResult;
use ade_git::history::{GitHistoryQuery, GitHistoryRefs, GitHistoryResult, GitRefsResult};
use ade_git::runner::CancelToken;
use ade_git::status::{GitConflictOperation, GitStatusResult, GitUpstreamStatus};
use ade_git::status_read::StatusOptions;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::run_blocking;
use crate::errors::BridgeError;
use crate::state::AppState;

/// `git show`'s budget mirrors `ade_git::diff`'s own 120s command timeout.
pub const GIT_DIFF_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub admission_tier: Option<String>,
    #[serde(default)]
    pub include_ignored: Option<bool>,
    #[serde(default)]
    pub include_line_stats: Option<bool>,
    #[serde(default)]
    pub bypass_effective_upstream_negative_cache: Option<bool>,
    #[serde(default)]
    pub reuse_line_stats: Option<bool>,
    #[serde(default)]
    pub branch_line_total_merge_base: Option<String>,
    #[serde(default)]
    pub request_token: Option<String>,
}

impl GitStatusArgs {
    /// The renderer only sends `includeLineStats` when it is explicitly
    /// `false` (`runtime-git-status-client.ts:26-27`), so an absent field means
    /// line stats are wanted. `limit` has no renderer counterpart; `None`
    /// resolves to the 1000-row cap.
    pub fn to_status_options(&self) -> StatusOptions {
        StatusOptions {
            limit: None,
            include_ignored: self.include_ignored.unwrap_or(false),
            include_line_stats: self.include_line_stats.unwrap_or(true),
            branch_line_total_merge_base: self.branch_line_total_merge_base.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCancelStatusArgs {
    pub request_token: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffArgs {
    pub worktree_path: String,
    pub file_path: String,
    pub staged: bool,
    #[serde(default)]
    pub compare_against_head: Option<bool>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFileArgs {
    pub worktree_path: String,
    pub file_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFilesArgs {
    pub worktree_path: String,
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitArgs {
    pub worktree_path: String,
    pub message: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

/// `git.commit` resolves a rejected commit as `{success:false,error?}` rather
/// than rejecting; only a missing message still rejects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitOutcome {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<ade_git::staging::CommitOutcome> for GitCommitOutcome {
    fn from(outcome: ade_git::staging::CommitOutcome) -> Self {
        Self {
            success: outcome.success,
            error: outcome.error,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitRemoteUrl {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCompareArgs {
    pub worktree_path: String,
    pub base_ref: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub admission_tier: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitCompareArgs {
    pub worktree_path: String,
    pub commit_id: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

/// `branchDiff.compare`: the caller passes both the base tip and the resolved
/// merge base, and the diff runs from the merge base (not `baseOid`), so a
/// diverged fork compares only the branch's own changes.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCompareRefsArgs {
    pub base_ref: String,
    pub base_oid: String,
    pub head_oid: String,
    pub merge_base: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDiffArgs {
    pub worktree_path: String,
    pub compare: GitCompareRefsArgs,
    pub file_path: String,
    #[serde(default)]
    pub old_path: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitDiffArgs {
    pub worktree_path: String,
    pub commit_oid: String,
    #[serde(default)]
    pub parent_oid: Option<String>,
    pub file_path: String,
    #[serde(default)]
    pub old_path: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub base_ref: Option<String>,
}

// ─── pure impls (state-free, so tests can exercise them without a Tauri app) ──

pub fn status_impl(
    worktree_path: &str,
    options: &StatusOptions,
    cancel: Option<&CancelToken>,
) -> Result<GitStatusResult, BridgeError> {
    Ok(ade_git::status_read::status(
        worktree_path,
        options,
        cancel,
    )?)
}

pub fn diff_impl(
    worktree_path: &str,
    file_path: &str,
    staged: bool,
    compare_against_head: bool,
) -> Result<GitDiffResult, BridgeError> {
    Ok(ade_git::diff::diff(
        worktree_path,
        file_path,
        staged,
        compare_against_head,
        GIT_DIFF_TIMEOUT,
    )?)
}

pub fn stage_impl(worktree_path: &str, file_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::staging::stage(worktree_path, file_path)?)
}

pub fn bulk_stage_impl(worktree_path: &str, file_paths: &[String]) -> Result<(), BridgeError> {
    Ok(ade_git::staging::bulk_stage(worktree_path, file_paths)?)
}

pub fn unstage_impl(worktree_path: &str, file_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::staging::unstage(worktree_path, file_path)?)
}

pub fn bulk_unstage_impl(worktree_path: &str, file_paths: &[String]) -> Result<(), BridgeError> {
    Ok(ade_git::staging::bulk_unstage(worktree_path, file_paths)?)
}

pub fn discard_impl(worktree_path: &str, file_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::staging::discard(worktree_path, file_path)?)
}

pub fn bulk_discard_impl(worktree_path: &str, file_paths: &[String]) -> Result<(), BridgeError> {
    Ok(ade_git::staging::bulk_discard(worktree_path, file_paths)?)
}

pub fn commit_impl(worktree_path: &str, message: &str) -> Result<GitCommitOutcome, BridgeError> {
    Ok(ade_git::staging::commit(worktree_path, message)?.into())
}

pub fn upstream_status_impl(worktree_path: &str) -> Result<GitUpstreamStatus, BridgeError> {
    Ok(ade_git::staging::upstream_status(worktree_path)?)
}

pub fn conflict_operation_impl(worktree_path: &str) -> Result<GitConflictOperation, BridgeError> {
    Ok(ade_git::status_read::conflict_operation(worktree_path)?)
}

pub fn branch_compare_impl(
    worktree_path: &str,
    base_ref: &str,
) -> Result<GitBranchCompareResult, BridgeError> {
    Ok(ade_git::compare::branch_compare(worktree_path, base_ref)?)
}

pub fn commit_compare_impl(
    worktree_path: &str,
    commit_id: &str,
) -> Result<GitCommitCompareResult, BridgeError> {
    Ok(ade_git::compare::commit_compare(worktree_path, commit_id)?)
}

/// The left rev is the caller-provided merge base, never `baseOid`: the
/// renderer passes the fork point so a diverged base compares only the
/// branch's own changes (Task 8 report ruling).
pub fn branch_diff_impl(
    worktree_path: &str,
    merge_base: &str,
    head_oid: &str,
    file_path: &str,
    old_path: Option<&str>,
) -> Result<GitDiffResult, BridgeError> {
    Ok(ade_git::compare::branch_diff(
        worktree_path,
        merge_base,
        head_oid,
        file_path,
        old_path,
    )?)
}

pub fn commit_diff_impl(
    worktree_path: &str,
    commit_oid: &str,
    parent_oid: Option<&str>,
    file_path: &str,
    old_path: Option<&str>,
) -> Result<GitDiffResult, BridgeError> {
    Ok(ade_git::compare::commit_diff(
        worktree_path,
        commit_oid,
        parent_oid,
        file_path,
        old_path,
    )?)
}

pub fn history_impl(
    worktree_path: &str,
    limit: Option<u32>,
    base_ref: Option<&str>,
) -> Result<GitHistoryResult, BridgeError> {
    Ok(ade_git::history::history(worktree_path, limit, base_ref)?)
}

/// `git remote -v` 的 fetch URL，按 remote 名首次出现去重。
pub fn parse_remote_urls(stdout: &str) -> Vec<GitRemoteUrl> {
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for line in stdout.lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let Some((url, kind)) = rest.rsplit_once(' ') else {
            continue;
        };
        if kind != "(fetch)" || !seen.insert(name.to_string()) {
            continue;
        }
        rows.push(GitRemoteUrl {
            name: name.to_string(),
            url: url.to_string(),
        });
    }
    rows
}

pub fn remote_urls_impl(worktree_path: &str) -> Result<Vec<GitRemoteUrl>, BridgeError> {
    let output = ade_git::runner::run_git_in(
        worktree_path,
        &["remote", "-v"],
        std::time::Duration::from_secs(10),
        None,
    )?;
    Ok(parse_remote_urls(&String::from_utf8_lossy(&output.stdout)))
}

// ─── authorization guards ────────────────────────────────────────────────────

/// Reject a worktree path outside every authorized fs root, so the renderer
/// cannot run git against an arbitrary repository (the `fs.*` authorization
/// model).
pub fn require_authorized_worktree(fs: &FsService, worktree_path: &str) -> Result<(), BridgeError> {
    fs.resolve(worktree_path)?;
    Ok(())
}

/// Renderer-supplied repo-relative paths must not escape the authorized
/// worktree: absolute paths and `..` segments are rejected before git runs.
pub fn validate_relative_file_path(file_path: &str) -> Result<(), BridgeError> {
    let path = Path::new(file_path);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(BridgeError::message(format!(
            "Invalid file path: {file_path}"
        )));
    }
    Ok(())
}

fn validate_relative_file_paths(file_paths: &[String]) -> Result<(), BridgeError> {
    for file_path in file_paths {
        validate_relative_file_path(file_path)?;
    }
    Ok(())
}

// ─── commands ────────────────────────────────────────────────────────────────

/// `git.status`: registers the renderer's `requestToken` (or a generated one)
/// so `git_cancel_status` can kill the running subprocess; the registration is
/// released whether the run succeeds or fails.
#[tauri::command]
#[specta::specta]
pub async fn git_status(
    state: State<'_, AppState>,
    args: GitStatusArgs,
) -> Result<GitStatusResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let request_token = args
        .request_token
        .clone()
        .unwrap_or_else(ade_core::ids::new_uuid);
    let worktree_path = args.worktree_path.clone();
    let options = args.to_status_options();
    let cancel = state.git_cancels.register(&request_token);
    let result = run_blocking(move || status_impl(&worktree_path, &options, Some(&cancel))).await;
    state.git_cancels.finish(&request_token);
    result
}

/// `git.cancelStatus`: sets the registered token; `false` (silently) when the
/// status already finished, matching the renderer's fire-and-forget call.
#[tauri::command]
#[specta::specta]
pub async fn git_cancel_status(
    state: State<'_, AppState>,
    args: GitCancelStatusArgs,
) -> Result<(), BridgeError> {
    state.git_cancels.cancel(&args.request_token);
    Ok(())
}

/// `git.diff`: HEAD/index vs worktree blob contents for one path.
#[tauri::command]
#[specta::specta]
pub async fn git_diff(
    state: State<'_, AppState>,
    args: GitDiffArgs,
) -> Result<GitDiffResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || {
        diff_impl(
            &args.worktree_path,
            &args.file_path,
            args.staged,
            args.compare_against_head.unwrap_or(false),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn git_stage(state: State<'_, AppState>, args: GitFileArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || stage_impl(&args.worktree_path, &args.file_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_bulk_stage(
    state: State<'_, AppState>,
    args: GitFilesArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_paths(&args.file_paths)?;
    run_blocking(move || bulk_stage_impl(&args.worktree_path, &args.file_paths)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_unstage(state: State<'_, AppState>, args: GitFileArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || unstage_impl(&args.worktree_path, &args.file_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_bulk_unstage(
    state: State<'_, AppState>,
    args: GitFilesArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_paths(&args.file_paths)?;
    run_blocking(move || bulk_unstage_impl(&args.worktree_path, &args.file_paths)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_discard(state: State<'_, AppState>, args: GitFileArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    run_blocking(move || discard_impl(&args.worktree_path, &args.file_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_bulk_discard(
    state: State<'_, AppState>,
    args: GitFilesArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_paths(&args.file_paths)?;
    run_blocking(move || bulk_discard_impl(&args.worktree_path, &args.file_paths)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit(
    state: State<'_, AppState>,
    args: GitCommitArgs,
) -> Result<GitCommitOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || commit_impl(&args.worktree_path, &args.message)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_upstream_status(
    state: State<'_, AppState>,
    args: GitWorktreeArgs,
) -> Result<GitUpstreamStatus, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || upstream_status_impl(&args.worktree_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_conflict_operation(
    state: State<'_, AppState>,
    args: GitWorktreeArgs,
) -> Result<GitConflictOperation, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || conflict_operation_impl(&args.worktree_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_compare(
    state: State<'_, AppState>,
    args: GitBranchCompareArgs,
) -> Result<GitBranchCompareResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || branch_compare_impl(&args.worktree_path, &args.base_ref)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit_compare(
    state: State<'_, AppState>,
    args: GitCommitCompareArgs,
) -> Result<GitCommitCompareResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || commit_compare_impl(&args.worktree_path, &args.commit_id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_diff(
    state: State<'_, AppState>,
    args: GitBranchDiffArgs,
) -> Result<GitDiffResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    if let Some(old_path) = &args.old_path {
        validate_relative_file_path(old_path)?;
    }
    run_blocking(move || {
        branch_diff_impl(
            &args.worktree_path,
            &args.compare.merge_base,
            &args.compare.head_oid,
            &args.file_path,
            args.old_path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn git_commit_diff(
    state: State<'_, AppState>,
    args: GitCommitDiffArgs,
) -> Result<GitDiffResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    validate_relative_file_path(&args.file_path)?;
    if let Some(old_path) = &args.old_path {
        validate_relative_file_path(old_path)?;
    }
    run_blocking(move || {
        commit_diff_impl(
            &args.worktree_path,
            &args.commit_oid,
            args.parent_oid.as_deref(),
            &args.file_path,
            args.old_path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn git_history(
    state: State<'_, AppState>,
    args: GitHistoryArgs,
) -> Result<GitHistoryResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || history_impl(&args.worktree_path, args.limit, args.base_ref.as_deref()))
        .await
}

/// Renderer `git.log` query (native Git Log view). Field names match the
/// frontend `logParams` verbatim.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitLogQueryIn {
    #[serde(default)]
    pub refs: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    #[serde(default)]
    pub grep: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub skip: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitLogQueryArgs {
    pub worktree_path: String,
    pub query: GitLogQueryIn,
}

/// ISO `YYYY-MM-DD` (optionally with a time suffix), month 01-12 / day 01-31.
fn is_iso_date(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && (b.len() == 10 || b[10] == b'T' || b[10] == b' ')
        && {
            let month = (b[5] - b'0') * 10 + (b[6] - b'0');
            let day = (b[8] - b'0') * 10 + (b[9] - b'0');
            (1..=12).contains(&month) && (1..=31).contains(&day)
        }
}

fn to_history_query(q: &GitLogQueryIn) -> Result<GitHistoryQuery, BridgeError> {
    for (key, value) in [("since", q.since.as_deref()), ("until", q.until.as_deref())] {
        if let Some(value) = value {
            if !is_iso_date(value) {
                return Err(BridgeError::message(format!(
                    "{key} must be an ISO date (YYYY-MM-DD)"
                )));
            }
        }
    }
    let refs = match q.refs.as_deref() {
        None | Some("head") => GitHistoryRefs::Head,
        Some("all") => GitHistoryRefs::All,
        Some(rev) => GitHistoryRefs::Rev(rev.to_string()),
    };
    Ok(GitHistoryQuery {
        refs,
        author: q.author.clone(),
        since: q.since.clone(),
        until: q.until.clone(),
        grep: q.grep.clone(),
        path: q.path.clone(),
        base_ref: None,
        skip: q.skip.unwrap_or(0),
        limit: q
            .limit
            .unwrap_or(ade_git::history::GIT_HISTORY_DEFAULT_LIMIT)
            .clamp(1, ade_git::history::GIT_HISTORY_MAX_LIMIT),
    })
}

pub fn git_log_query_impl(
    worktree_path: &str,
    query: &GitLogQueryIn,
) -> Result<GitHistoryResult, BridgeError> {
    let history_query = to_history_query(query)?;
    Ok(ade_git::history::history_with(worktree_path, &history_query)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_log_query(
    state: State<'_, AppState>,
    args: GitLogQueryArgs,
) -> Result<GitHistoryResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_log_query_impl(&args.worktree_path, &args.query)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitRefsArgs {
    pub worktree_path: String,
}

pub fn git_refs_impl(worktree_path: &str) -> Result<GitRefsResult, BridgeError> {
    Ok(ade_git::history::list_refs(worktree_path)?)
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCheckoutArgs {
    pub worktree_path: String,
    pub target: ade_git::GitCheckoutTarget,
}

/// 原地检出分支。业务级拒绝（被别的 worktree 占用、目标非法、已在目标分支）
/// 以 `Ok(GitCheckoutOutcome.status)` 透出，**不**经 `BridgeError` ——
/// 后者的 `Serialize` 只留 `{ message }`，类型信息全丢（spec §8.1）。
/// `Err` 只留给环境级失败：git 不可用、路径不允许、以及无法分类的 git 非零退出
/// （最典型是「checkout 会覆盖本地改动」）。
pub fn git_checkout_impl(
    worktree_path: &str,
    target: &ade_git::GitCheckoutTarget,
) -> Result<ade_git::GitCheckoutOutcome, BridgeError> {
    Ok(ade_git::branch_ops::checkout(worktree_path, target)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_refs(
    state: State<'_, AppState>,
    args: GitRefsArgs,
) -> Result<GitRefsResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_refs_impl(&args.worktree_path)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFetchArgs {
    pub worktree_path: String,
    /// 由 `real/git.ts` 从 `pushTarget.remoteName` 解析而来（与 `git_push` 同一分层）；
    /// 缺省时交给 git（按当前分支 upstream / 默认 remote）。
    #[serde(default)]
    pub remote: Option<String>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitPullArgs {
    pub worktree_path: String,
}

/// `git fetch --prune`。网络失败（权限、不可达）以 `Err` 上抛，stderr 原样带上。
/// 凭证提示已被 `GIT_TERMINAL_PROMPT=0` 禁掉，所以不会挂到超时。
pub fn git_fetch_impl(
    worktree_path: &str,
    remote: Option<&str>,
) -> Result<(), BridgeError> {
    Ok(ade_git::remote_ops::fetch(worktree_path, remote, None)?)
}

/// `git pull`（按用户 git 配置，不强制 ff-only / rebase）。
pub fn git_pull_impl(worktree_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::remote_ops::pull(worktree_path, None)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_fetch(
    state: State<'_, AppState>,
    args: GitFetchArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let path = args.worktree_path;
    let remote = args.remote;
    run_blocking(move || git_fetch_impl(&path, remote.as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_pull(state: State<'_, AppState>, args: GitPullArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_pull_impl(&args.worktree_path)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFastForwardArgs {
    pub worktree_path: String,
}

/// `git pull --ff-only`：只快进，分叉时原样失败（不自动 merge）。网络失败以 `Err` 上抛。
pub fn git_fast_forward_impl(worktree_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::remote_ops::fast_forward(worktree_path, None)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_fast_forward(
    state: State<'_, AppState>,
    args: GitFastForwardArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_fast_forward_impl(&args.worktree_path)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitRebaseFromBaseArgs {
    pub worktree_path: String,
    pub base_ref: String,
}

/// `git rebase <base_ref>`（本地）。冲突时非零退出、仓库留在 rebase 态，由 git_abort_rebase 回滚。
pub fn git_rebase_from_base_impl(worktree_path: &str, base_ref: &str) -> Result<(), BridgeError> {
    Ok(ade_git::rebase_ops::rebase_from_base(worktree_path, base_ref, None)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_rebase_from_base(
    state: State<'_, AppState>,
    args: GitRebaseFromBaseArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let path = args.worktree_path;
    let base = args.base_ref;
    run_blocking(move || git_rebase_from_base_impl(&path, &base)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitAbortRebaseArgs {
    pub worktree_path: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitAbortMergeArgs {
    pub worktree_path: String,
}

/// `git rebase --abort`：回到 rebase 前状态。
pub fn git_abort_rebase_impl(worktree_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::rebase_ops::abort_rebase(worktree_path, None)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_abort_rebase(
    state: State<'_, AppState>,
    args: GitAbortRebaseArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_abort_rebase_impl(&args.worktree_path)).await
}

/// `git merge --abort`：回到 merge 前状态。
pub fn git_abort_merge_impl(worktree_path: &str) -> Result<(), BridgeError> {
    Ok(ade_git::rebase_ops::abort_merge(worktree_path, None)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_abort_merge(
    state: State<'_, AppState>,
    args: GitAbortMergeArgs,
) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_abort_merge_impl(&args.worktree_path)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_checkout(
    state: State<'_, AppState>,
    args: GitCheckoutArgs,
) -> Result<ade_git::GitCheckoutOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let path = args.worktree_path;
    let target = args.target;
    run_blocking(move || git_checkout_impl(&path, &target)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCreateArgs {
    pub worktree_path: String,
    pub args: ade_git::GitBranchCreateInput,
}

/// 在 start-point 建分支 ref（不检出）。业务拒绝（名已占用）以 `Ok(Outcome.status)`
/// 透出，**不**经 `BridgeError`（§8.1）；`Err` 只留给环境级失败（非法名 / start-point 解析不到）。
pub fn git_branch_create_impl(
    worktree_path: &str,
    args: &ade_git::GitBranchCreateInput,
) -> Result<ade_git::GitBranchCreateOutcome, BridgeError> {
    Ok(ade_git::branch_ops::create(worktree_path, args)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_create(
    state: State<'_, AppState>,
    args: GitBranchCreateArgs,
) -> Result<ade_git::GitBranchCreateOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let path = args.worktree_path;
    let inner = args.args;
    run_blocking(move || git_branch_create_impl(&path, &inner)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchSetUpstreamArgs {
    pub worktree_path: String,
    pub args: ade_git::GitBranchSetUpstreamInput,
}

/// 给已存在的本地分支设 upstream。业务拒绝（上游不存在）以 `Ok(Outcome.status)` 透出，
/// **不**经 `BridgeError`（§8.1）；`Err` 只留给环境级失败（本地分支不存在 / git 报错）。
pub fn git_branch_set_upstream_impl(
    worktree_path: &str,
    args: &ade_git::GitBranchSetUpstreamInput,
) -> Result<ade_git::GitBranchSetUpstreamOutcome, BridgeError> {
    Ok(ade_git::branch_ops::set_upstream(worktree_path, args)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_set_upstream(
    state: State<'_, AppState>,
    args: GitBranchSetUpstreamArgs,
) -> Result<ade_git::GitBranchSetUpstreamOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let path = args.worktree_path;
    let inner = args.args;
    run_blocking(move || git_branch_set_upstream_impl(&path, &inner)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchRenameArgs {
    pub worktree_path: String,
    pub args: ade_git::GitBranchRenameInput,
}

/// 重命名本地分支（仅未检出）。业务拒绝（NameTaken / CheckedOut）以 `Ok(Outcome.status)` 透出。
pub fn git_branch_rename_impl(
    worktree_path: &str,
    args: &ade_git::GitBranchRenameInput,
) -> Result<ade_git::GitBranchRenameOutcome, BridgeError> {
    Ok(ade_git::branch_ops::rename(worktree_path, args)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_rename(
    state: State<'_, AppState>,
    args: GitBranchRenameArgs,
) -> Result<ade_git::GitBranchRenameOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let path = args.worktree_path;
    let inner = args.args;
    run_blocking(move || git_branch_rename_impl(&path, &inner)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDeleteArgs {
    pub worktree_path: String,
    pub args: ade_git::GitBranchDeleteInput,
}

/// 删除本地分支（仅未检出）。未合并→Ok(Refused(NotMergeable))、占用→Ok(Refused(CheckedOut))；不经 BridgeError。
pub fn git_branch_delete_impl(
    worktree_path: &str,
    args: &ade_git::GitBranchDeleteInput,
) -> Result<ade_git::GitBranchDeleteOutcome, BridgeError> {
    Ok(ade_git::branch_ops::delete(worktree_path, args)?)
}

#[tauri::command]
#[specta::specta]
pub async fn git_branch_delete(
    state: State<'_, AppState>,
    args: GitBranchDeleteArgs,
) -> Result<ade_git::GitBranchDeleteOutcome, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    let path = args.worktree_path;
    let inner = args.args;
    run_blocking(move || git_branch_delete_impl(&path, &inner)).await
}

#[tauri::command]
#[specta::specta]
pub async fn git_remote_urls(
    state: State<'_, AppState>,
    args: GitWorktreeArgs,
) -> Result<Vec<GitRemoteUrl>, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || remote_urls_impl(&args.worktree_path)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitReadArgs {
    pub worktree_path: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitReadResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

const GIT_READ_ALLOWED_SUBCOMMANDS: &[&str] = &[
    "config",
    "rev-parse",
    "symbolic-ref",
    "show-ref",
    "check-ref-format",
    "remote",
];
const GIT_CONFIG_READ_FLAGS: &[&str] = &["--get", "--get-all", "--get-regexp", "--list"];
const GIT_CONFIG_WRITE_FLAGS: &[&str] = &[
    "--unset",
    "--unset-all",
    "--add",
    "--replace-all",
    "--edit",
    "--rename-section",
    "--remove-section",
    "--set",
];
/// `config` 越权读入口：`--file`/`--blob` 的长选项名（用于识别缩写与 `=value`）。
const GIT_CONFIG_CONTAINMENT_FLAGS: &[&str] = &["file", "blob"];
/// `symbolic-ref` 仅允许这些精确读标志（长选项缩写一律拒绝）。
const GIT_SYMBOLIC_REF_READ_FLAGS: &[&str] = &["-q", "--quiet", "--short", "--no-recurse"];

/// `config` 的越权读入口判定：`--file`/`--blob`、其任意无歧义长选项缩写与
/// `=value` 形式，以及 `-f`（含 `-f<path>` 与组合短选项中的 `f`）。
fn is_config_containment_bypass(arg: &str) -> bool {
    if let Some(rest) = arg.strip_prefix("--") {
        let name = rest.split('=').next().unwrap_or(rest);
        return name.len() >= 2
            && GIT_CONFIG_CONTAINMENT_FLAGS
                .iter()
                .any(|flag| flag.starts_with(name));
    }
    arg.starts_with('-') && arg[1..].contains('f')
}

/// 只读 git 命令白名单：首参必须受支持；`config` 仅允许读形式（含 `--get*`/`--list`，
/// 且拒绝任何写标志、裸写位置参数与越权读入口）；`symbolic-ref` 仅允许精确读标志，
/// 其余任何以 `-` 开头的参数或第二个位置参数（写形式）一律拒绝。
pub fn is_allowed_git_read_args(args: &[String]) -> bool {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return false;
    };
    if !GIT_READ_ALLOWED_SUBCOMMANDS.contains(&subcommand) {
        return false;
    }
    if subcommand == "symbolic-ref" {
        if args[1..]
            .iter()
            .any(|arg| arg.starts_with('-') && !GIT_SYMBOLIC_REF_READ_FLAGS.contains(&arg.as_str()))
        {
            return false;
        }
        let positional = args[1..].iter().filter(|arg| !arg.starts_with('-')).count();
        return positional <= 1;
    }
    // `remote` 仅允许纯读形式：`remote`、`remote -v`、`remote --verbose`。
    if subcommand == "remote" {
        return args.len() == 1 || (args.len() == 2 && (args[1] == "-v" || args[1] == "--verbose"));
    }
    if subcommand != "config" {
        return true;
    }
    if args.iter().any(|arg| {
        GIT_CONFIG_WRITE_FLAGS
            .iter()
            .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
    }) {
        return false;
    }
    if args.iter().any(|arg| is_config_containment_bypass(arg)) {
        return false;
    }
    // `git config` 在首个位置参数处 STOP_AT_NON_OPTION：其后的选项会被当作
    // `<name> <value>` 的一部分（例如 `config core.hooksPath /tmp/evil --get`
    // 会写入），因此位置参数之后不允许再出现任何以 `-` 开头的参数。
    let mut seen_positional = false;
    for arg in &args[1..] {
        if arg.starts_with('-') {
            if seen_positional {
                return false;
            }
        } else {
            seen_positional = true;
        }
    }
    let has_read_flag = args
        .iter()
        .any(|arg| GIT_CONFIG_READ_FLAGS.contains(&arg.as_str()));
    // 裸写形式：`config <key> <value>`（≥2 个非选项位置参数）且无读标志。
    let positional = args[1..].iter().filter(|arg| !arg.starts_with('-')).count();
    has_read_flag && positional <= 2
}

pub fn git_read_impl(worktree_path: &str, args: &[String]) -> Result<GitReadResult, BridgeError> {
    if !is_allowed_git_read_args(args) {
        return Err(BridgeError::message(format!(
            "git read rejected: {}",
            args.first().cloned().unwrap_or_default()
        )));
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = ade_git::runner::run_git_in(
        worktree_path,
        &borrowed,
        std::time::Duration::from_secs(120),
        None,
    )?;
    Ok(GitReadResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn git_read(
    state: State<'_, AppState>,
    args: GitReadArgs,
) -> Result<GitReadResult, BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || git_read_impl(&args.worktree_path, &args.args)).await
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitPushArgs {
    pub worktree_path: String,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub refspec: Option<String>,
    #[serde(default)]
    pub force_with_lease: bool,
}

/// 安全 remote 名：1–100、按 `/` 分段每段非空且非 `.`/`..`、段首字母数字。
pub fn is_safe_remote_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 100 {
        return false;
    }
    name.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphanumeric())
            && segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    })
}

pub fn push_args(
    remote: Option<&str>,
    refspec: Option<&str>,
    force_with_lease: bool,
) -> Vec<String> {
    let mut args = vec!["push".to_string()];
    if force_with_lease {
        args.push("--force-with-lease".to_string());
    }
    args.push("--set-upstream".to_string());
    match (remote, refspec) {
        (Some(remote), Some(refspec)) => {
            args.push(remote.to_string());
            args.push(refspec.to_string());
        }
        _ => {
            args.push("origin".to_string());
            args.push("HEAD".to_string());
        }
    }
    args
}

pub fn git_push_impl(
    worktree_path: &str,
    remote: Option<&str>,
    refspec: Option<&str>,
    force_with_lease: bool,
) -> Result<(), BridgeError> {
    if let Some(remote) = remote {
        if !is_safe_remote_name(remote) {
            return Err(BridgeError::message(
                "git push rejected: unsafe remote name",
            ));
        }
    }
    if let Some(refspec) = refspec {
        if refspec.is_empty() || refspec.starts_with('-') {
            return Err(BridgeError::message("git push rejected: unsafe refspec"));
        }
    }
    let args = push_args(remote, refspec, force_with_lease);
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    // 与 fetch/pull 同一入口：`GIT_TERMINAL_PROMPT=0` 防止私有 HTTPS 仓在
    // 没有可用凭证 helper 时去开 `/dev/tty` 要密码 —— GUI 应用里那表现为
    // 挂到超时而不是快速失败。超时也从 120s 提到网络超时：
    // 大仓推送走 120s 读超时是误杀。
    let output = ade_git::runner::run_git_in_with_env(
        worktree_path,
        &borrowed,
        ade_git::remote_ops::GIT_NETWORK_TIMEOUT,
        None,
        ade_git::remote_ops::GIT_NETWORK_ENVS,
    )?;
    if output.status.success() {
        return Ok(());
    }
    Err(BridgeError::message(
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn git_push(state: State<'_, AppState>, args: GitPushArgs) -> Result<(), BridgeError> {
    require_authorized_worktree(&state.fs, &args.worktree_path)?;
    run_blocking(move || {
        git_push_impl(
            &args.worktree_path,
            args.remote.as_deref(),
            args.refspec.as_deref(),
            args.force_with_lease,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_options_default_to_line_stats_on_and_limit_uncapped() {
        let args: GitStatusArgs =
            serde_json::from_value(json!({ "worktreePath": "/tmp/x" })).unwrap();
        let options = args.to_status_options();
        assert_eq!(options.limit, None);
        assert!(!options.include_ignored);
        assert!(options.include_line_stats);
        assert_eq!(options.branch_line_total_merge_base, None);
    }

    #[test]
    fn status_options_honor_explicit_false_and_passthroughs() {
        let args: GitStatusArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "includeIgnored": true,
            "includeLineStats": false,
            "branchLineTotalMergeBase": "0123456789abcdef0123456789abcdef01234567"
        }))
        .unwrap();
        let options = args.to_status_options();
        assert!(options.include_ignored);
        assert!(!options.include_line_stats);
        assert_eq!(
            options.branch_line_total_merge_base.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
    }

    #[test]
    fn status_args_ignore_unknown_remote_only_fields() {
        let args: GitStatusArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "bypassEffectiveUpstreamNegativeCache": true,
            "reuseLineStats": true,
            "admissionTier": "status",
            "connectionId": "conn-1"
        }))
        .unwrap();
        assert_eq!(args.connection_id.as_deref(), Some("conn-1"));
        assert_eq!(args.admission_tier.as_deref(), Some("status"));
    }

    #[test]
    fn cancel_args_deserialize_the_request_token() {
        let args: GitCancelStatusArgs =
            serde_json::from_value(json!({ "requestToken": "t1" })).unwrap();
        assert_eq!(args.request_token, "t1");
    }

    #[test]
    fn branch_diff_args_keep_merge_base_separate_from_base_oid() {
        let args: GitBranchDiffArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "compare": {
                "baseRef": "main",
                "baseOid": "1111111111111111111111111111111111111111",
                "headOid": "2222222222222222222222222222222222222222",
                "mergeBase": "3333333333333333333333333333333333333333"
            },
            "filePath": "src/app.ts"
        }))
        .unwrap();
        assert_eq!(
            args.compare.merge_base,
            "3333333333333333333333333333333333333333"
        );
        assert_eq!(
            args.compare.base_oid,
            "1111111111111111111111111111111111111111"
        );
        assert_eq!(args.old_path, None);
    }

    #[test]
    fn commit_diff_args_accept_absent_and_null_parents() {
        let absent: GitCommitDiffArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "commitOid": "abc",
            "filePath": "a.txt"
        }))
        .unwrap();
        assert_eq!(absent.parent_oid, None);

        let null: GitCommitDiffArgs = serde_json::from_value(json!({
            "worktreePath": "/tmp/x",
            "commitOid": "abc",
            "parentOid": null,
            "filePath": "a.txt"
        }))
        .unwrap();
        assert_eq!(null.parent_oid, None);
    }

    #[test]
    fn relative_file_path_guard_rejects_absolute_and_parent_traversal() {
        assert!(validate_relative_file_path("src/app.ts").is_ok());
        assert!(validate_relative_file_path("../outside.txt").is_err());
        assert!(validate_relative_file_path("a/b/../c").is_err());
        assert!(validate_relative_file_path("/etc/passwd").is_err());
    }

    #[test]
    fn commit_outcome_converts_from_the_domain_outcome() {
        let outcome: GitCommitOutcome = ade_git::staging::CommitOutcome {
            success: false,
            error: Some("boom".to_string()),
        }
        .into();
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            json!({ "success": false, "error": "boom" })
        );
    }

    #[test]
    fn parse_remote_urls_keeps_fetch_lines_and_dedupes_by_name() {
        let stdout = concat!(
            "origin\tgit@github.com:owner/repo.git (fetch)\n",
            "origin\tgit@github.com:owner/repo.git (push)\n",
            "origin\tgit@github.com:other/repo.git (fetch)\n",
            "upstream\thttps://github.com/up/repo.git (fetch)\n",
            "malformed line without a tab\n",
        );

        let urls = parse_remote_urls(stdout);

        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].name, "origin");
        assert_eq!(urls[0].url, "git@github.com:owner/repo.git");
        assert_eq!(urls[1].name, "upstream");
        assert_eq!(urls[1].url, "https://github.com/up/repo.git");
    }
}
