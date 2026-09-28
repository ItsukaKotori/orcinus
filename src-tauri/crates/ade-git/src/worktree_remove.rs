//! Worktree removal preflight, `git worktree remove`/prune and branch cleanup.
//!
//! Mirrors `orca:src/main/git/worktree-removal.ts`,
//! `worktree-removal-preflight.ts`, the locked rejection in
//! `orca:src/shared/worktree/removal.ts:79-85` and
//! `orca:src/main/git/worktree-branch-removal.ts`.

use std::path::Path;
use std::time::Duration;

use ade_core::errors::CoreError;

use crate::runner::run_git_in;
use crate::{are_worktree_paths_equal, git_command_failed, worktree_list};

/// Mirrors `WORKTREE_REMOVAL_PREFLIGHT_TIMEOUT_MS`
/// (`orca:src/main/git/worktree-operation-options.ts:50`).
pub const REMOVAL_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
/// `git worktree remove` may recursively delete a large checkout; the oracle
/// leaves it unbounded, the port caps it.
const REMOVAL_TIMEOUT: Duration = Duration::from_secs(180);
/// Prune/ref operations are registration-scale, mirroring
/// `WORKTREE_REMOVAL_REGISTRATION_TIMEOUT_MS`.
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);

/// What happened to the worktree's branch during removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchDeleteOutcome {
    Deleted,
    Preserved {
        branch_name: String,
        head: Option<String>,
    },
    Skipped,
}

/// Assert a registered, unlocked worktree is clean enough for removal.
///
/// Mirrors `assertWorktreeUnlockedForRemoval`
/// (`orca:src/shared/worktree/removal.ts:79-85`) plus
/// `assertWorktreeCleanForRemoval`
/// (`orca:src/main/git/worktree-removal-preflight.ts:8-44`): re-list to prove
/// the registration still exists, refuse Git locks (a lock survives `--force`
/// and represents an external safety contract), and, unless forced, require an
/// empty `git status --porcelain -z --untracked-files=all`. A status command
/// that fails outright is not proof of cleanliness.
pub fn assert_worktree_removable(
    repo_path: &str,
    worktree_path: &str,
    force: bool,
) -> Result<(), CoreError> {
    let entries = worktree_list(repo_path)?;
    if !entries
        .iter()
        .any(|entry| are_worktree_paths_equal(&entry.path, worktree_path))
    {
        return Err(CoreError::InvalidInput(format!(
            "Worktree registration changed during deletion: {worktree_path}"
        )));
    }

    if let Some(lock_reason) = worktree_lock_reason(repo_path, worktree_path)? {
        return Err(locked_worktree_removal_error(lock_reason.as_deref()));
    }

    if force {
        return Ok(());
    }

    let args = ["status", "--porcelain", "-z", "--untracked-files=all"];
    let output = run_git_in(worktree_path, &args, REMOVAL_PREFLIGHT_TIMEOUT, None)?;
    if !output.status.success() {
        return Err(git_command_failed(&args, &output));
    }
    if !output.stdout.is_empty() {
        return Err(CoreError::InvalidInput(
            "Worktree has uncommitted or untracked changes.".to_string(),
        ));
    }
    Ok(())
}

/// Remove a worktree, falling back to `git worktree prune` and one retry.
///
/// Mirrors the oracle's inline `git worktree remove [--force] <path>` plus its
/// prune fallback (`worktree-removal.ts:152-183`): when the directory is
/// already gone (deleted by hand), the registration is pruned away and the
/// desired end state counts as success.
pub fn worktree_remove(repo_path: &str, worktree_path: &str, force: bool) -> Result<(), CoreError> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(worktree_path);

    if let Ok(output) = run_git_in(repo_path, &args, REMOVAL_TIMEOUT, None) {
        if output.status.success() {
            return Ok(());
        }
    }

    let _ = run_git_in(
        repo_path,
        &["worktree", "prune"],
        REGISTRATION_TIMEOUT,
        None,
    );
    let output = run_git_in(repo_path, &args, REMOVAL_TIMEOUT, None)?;
    if output.status.success() {
        return Ok(());
    }

    // The directory and its registration both being gone is the end state the
    // caller asked for, even though the retry had nothing left to remove.
    if !Path::new(worktree_path).exists() && !is_registered(repo_path, worktree_path)? {
        return Ok(());
    }
    Err(git_command_failed(&args, &output))
}

/// Delete the worktree's local branch after removal.
///
/// Mirrors `deleteLocalBranchAfterWorktreeRemoval`
/// (`orca:src/main/git/worktree-branch-removal.ts:63-102`) minus the
/// checked-out prune-and-retry and squash-merged force cleanup: `-d` preserves
/// a branch Git calls "not fully merged", every other failure surfaces. A ref
/// that is not `refs/heads/<name>` is skipped, mirroring the oracle's
/// `normalizeLocalBranchRef` producing an empty branch name for detached
/// worktrees.
pub fn delete_branch(
    repo_path: &str,
    branch_ref: &str,
    force: bool,
) -> Result<BranchDeleteOutcome, CoreError> {
    let Some(branch_name) = branch_ref.strip_prefix("refs/heads/") else {
        return Ok(BranchDeleteOutcome::Skipped);
    };
    if branch_name.is_empty() {
        return Ok(BranchDeleteOutcome::Skipped);
    }

    let flag = if force { "-D" } else { "-d" };
    let args = ["branch", flag, "--", branch_name];
    let output = run_git_in(repo_path, &args, REGISTRATION_TIMEOUT, None)?;
    if output.status.success() {
        return Ok(BranchDeleteOutcome::Deleted);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !force && stderr.contains("not fully merged") {
        let head = run_git_in(
            repo_path,
            &["rev-parse", branch_ref],
            REGISTRATION_TIMEOUT,
            None,
        )
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|head| !head.is_empty());
        return Ok(BranchDeleteOutcome::Preserved {
            branch_name: branch_name.to_string(),
            head,
        });
    }
    Err(git_command_failed(&args, &output))
}

/// Force-delete a preserved branch with a compare-and-swap guard.
///
/// Mirrors `forceDeleteLocalBranch`'s `update-ref -d` contract
/// (`orca:src/main/git/worktree-branch-removal.ts:151-179`): the ref is
/// deleted only while it still points at the commit the removal preserved, so
/// a stale recovery action can never discard newer work. The
/// `branch.<name>` config section cleanup that follows is best-effort.
pub fn force_delete_branch(
    repo_path: &str,
    branch_name: &str,
    expected_head: &str,
) -> Result<(), CoreError> {
    let ref_name = format!("refs/heads/{branch_name}");
    let args = ["update-ref", "-d", ref_name.as_str(), expected_head];
    let output = run_git_in(repo_path, &args, REGISTRATION_TIMEOUT, None)?;
    if !output.status.success() {
        return Err(CoreError::InvalidInput(format!(
            "Local branch \"{branch_name}\" changed after the workspace was deleted. Review it before deleting it."
        )));
    }

    let section = format!("branch.{branch_name}");
    let _ = run_git_in(
        repo_path,
        &["config", "--remove-section", section.as_str()],
        REGISTRATION_TIMEOUT,
        None,
    );
    Ok(())
}

fn is_registered(repo_path: &str, worktree_path: &str) -> Result<bool, CoreError> {
    Ok(worktree_list(repo_path)?
        .iter()
        .any(|entry| are_worktree_paths_equal(&entry.path, worktree_path)))
}

/// `Some(reason)` when the raw listing marks the target locked. Locked
/// worktrees survive `git worktree prune`, so removal must refuse them before
/// touching the filesystem.
fn worktree_lock_reason(
    repo_path: &str,
    worktree_path: &str,
) -> Result<Option<Option<String>>, CoreError> {
    let args = ["worktree", "list", "--porcelain", "-z"];
    let output = run_git_in(repo_path, &args, REGISTRATION_TIMEOUT, None)?;
    if !output.status.success() {
        return Err(git_command_failed(&args, &output));
    }

    let mut current_path: Option<String> = None;
    for record in output.stdout.split(|byte| *byte == 0) {
        let text = String::from_utf8_lossy(record);
        if let Some(path) = text.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(reason) = text.strip_prefix("locked") {
            if current_path
                .as_deref()
                .is_some_and(|path| are_worktree_paths_equal(path, worktree_path))
            {
                let reason = reason.trim();
                return Ok(Some(if reason.is_empty() {
                    None
                } else {
                    Some(reason.to_string())
                }));
            }
        }
    }
    Ok(None)
}

/// Verbatim from `createLockedWorktreeRemovalError`
/// (`orca:src/shared/worktree/removal.ts:70-77`).
fn locked_worktree_removal_error(lock_reason: Option<&str>) -> CoreError {
    let reason = lock_reason
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    let message = match reason {
        Some(reason) => format!(
            "Worktree is locked by Git. Lock reason: {reason}. Run git worktree unlock <worktree-path> from its repository, then retry deletion."
        ),
        None => "Worktree is locked by Git. Run git worktree unlock <worktree-path> from its repository, then retry deletion.".to_string(),
    };
    CoreError::InvalidInput(message)
}
