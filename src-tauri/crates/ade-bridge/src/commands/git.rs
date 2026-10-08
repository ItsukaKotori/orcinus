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
use ade_git::history::GitHistoryResult;
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
