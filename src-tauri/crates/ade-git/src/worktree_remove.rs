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
/// Mirrors `deleteBranchAfterWorktreeRemoval`
/// (`orca:src/main/git/worktree-branch-removal.ts:14-102`): the branch cleanup
/// never rejects after the worktree is gone — a failure keeps the branch
/// (`Preserved`) instead of discarding work or failing the removal. A
/// checked-out refusal may come from a stale registration, so it prunes and
/// retries once; a still-checked-out branch is left alone (`Skipped`). The
/// oracle's squash-merge tree-equivalence cleanup is out of this port's scope,
/// and preserving is the safe direction there too. A ref that is not
/// `refs/heads/<name>` is skipped, mirroring the oracle's
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
    if let Ok(output) = run_git_in(repo_path, &args, REGISTRATION_TIMEOUT, None) {
        if output.status.success() {
            return Ok(BranchDeleteOutcome::Deleted);
        }

        if is_checked_out_refusal(&output.stderr) {
            // Why: only pay for `worktree prune` when a stale admin record may
            // be blocking `branch -d`. If prune itself fails, the oracle reads
            // the branch as checked out and leaves it.
            let pruned = matches!(
                run_git_in(
                    repo_path,
                    &["worktree", "prune"],
                    REGISTRATION_TIMEOUT,
                    None
                ),
                Ok(output) if output.status.success()
            );
            if !pruned {
                return Ok(BranchDeleteOutcome::Skipped);
            }

            match run_git_in(repo_path, &args, REGISTRATION_TIMEOUT, None) {
                Ok(output) if output.status.success() => return Ok(BranchDeleteOutcome::Deleted),
                Ok(output) if is_checked_out_refusal(&output.stderr) => {
                    return Ok(BranchDeleteOutcome::Skipped)
                }
                // Any other retry outcome keeps the branch below.
                _ => {}
            }
        }
    }

    Ok(BranchDeleteOutcome::Preserved {
        branch_name: branch_name.to_string(),
        head: branch_head(repo_path, branch_ref),
    })
}

/// Force-delete a preserved branch with compare-and-swap and checkout guards.
///
/// Mirrors `forceDeleteLocalBranch`
/// (`orca:src/main/git/worktree-branch-removal.ts:131-179`): a branch that any
/// registered worktree still checks out is refused, the ref is deleted only
/// while it still points at the commit the removal preserved (so a stale
/// recovery action can never discard newer work), and a checkout that appears
/// concurrently is recovered by restoring the ref. The `branch.<name>` config
/// section cleanup that follows is best-effort.
pub fn force_delete_branch(
    repo_path: &str,
    branch_name: &str,
    expected_head: &str,
) -> Result<(), CoreError> {
    if branch_name.is_empty() || branch_name.contains('\0') {
        return Err(CoreError::InvalidInput("Invalid branch name".to_string()));
    }
    if expected_head.is_empty() {
        return Err(CoreError::InvalidInput(format!(
            "Cannot force-delete local branch \"{branch_name}\" without the commit Git preserved."
        )));
    }
    if is_branch_checked_out(repo_path, branch_name)? {
        return Err(checked_out_error(branch_name));
    }

    let ref_name = format!("refs/heads/{branch_name}");
    let args = ["update-ref", "-d", ref_name.as_str(), expected_head];
    let output = run_git_in(repo_path, &args, REGISTRATION_TIMEOUT, None)?;
    if !output.status.success() {
        return Err(CoreError::InvalidInput(format!(
            "Local branch \"{branch_name}\" changed after the workspace was deleted. Review it before deleting it."
        )));
    }

    if is_branch_checked_out(repo_path, branch_name)? {
        // Why: a concurrent checkout must get its ref back exactly where it was.
        let restore = ["update-ref", ref_name.as_str(), expected_head, ""];
        let _ = run_git_in(repo_path, &restore, REGISTRATION_TIMEOUT, None);
        return Err(checked_out_error(branch_name));
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

/// Mirrors `isLocalBranchCheckedOut`
/// (`orca:src/main/git/worktree-branch-removal.ts:181-189`): any registered
/// worktree whose branch ref is `refs/heads/<branch_name>`.
fn is_branch_checked_out(repo_path: &str, branch_name: &str) -> Result<bool, CoreError> {
    Ok(worktree_list(repo_path)?.iter().any(|entry| {
        entry
            .branch
            .as_deref()
            .and_then(|branch| branch.strip_prefix("refs/heads/"))
            == Some(branch_name)
    }))
}

/// Mirrors `isBranchCheckedOutInWorktreeError`
/// (`orca:src/shared/git-branch-delete-refusal.ts:12-16`): Git through 2.40
/// says "Cannot delete branch 'x' checked out at '<path>'", 2.43+ says "cannot
/// delete branch 'x' used by worktree at '<path>'", both on stderr.
fn is_checked_out_refusal(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr).to_lowercase();
    (text.contains("cannot delete branch")
        && (text.contains("used by worktree") || text.contains("checked out")))
        || (text.contains("branch") && text.contains("is checked out"))
}

fn checked_out_error(branch_name: &str) -> CoreError {
    CoreError::InvalidInput(format!(
        "Local branch \"{branch_name}\" is checked out in another worktree."
    ))
}

fn branch_head(repo_path: &str, branch_ref: &str) -> Option<String> {
    run_git_in(
        repo_path,
        &["rev-parse", branch_ref],
        REGISTRATION_TIMEOUT,
        None,
    )
    .ok()
    .filter(|output| output.status.success())
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    .filter(|head| !head.is_empty())
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
