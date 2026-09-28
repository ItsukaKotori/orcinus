//! Worktree creation: the `git worktree add` invocation and the follow-up
//! config writes.
//!
//! Mirrors `orca:src/main/git/worktree-add.ts` (`performAddWorktree`,
//! `persistWorktreeCreationBase`, `configurePushAutoSetupRemote`).

use std::time::Duration;

use ade_core::errors::CoreError;

use crate::runner::run_git_in;
use crate::{are_worktree_paths_equal, git_command_failed, worktree_list};

/// Mirrors `WORKTREE_ADD_TIMEOUT_MS`
/// (`orca:src/main/git/worktree-operation-options.ts:47`).
const WORKTREE_ADD_TIMEOUT: Duration = Duration::from_secs(180);
/// Local config reads/writes are fast; the probe deadline matches `branch.rs`.
const CONFIG_TIMEOUT: Duration = Duration::from_secs(5);

/// One new-branch worktree creation, mirroring the oracle's
/// `addWorktree(repoPath, worktreePath, branch, baseBranch)` arguments.
pub struct AddWorktreeRequest {
    pub repo_path: String,
    pub worktree_path: String,
    pub branch: String,
    pub base_ref: String,
}

/// Run `git worktree add --no-track -b <branch> <path> <base_ref>`.
///
/// A path that is already registered is rejected before git runs, so the
/// caller can recognize the conflict and retry with a suffixed name. The
/// `branch.<branch>.base` / `push.autoSetupRemote` follow-ups stay separate
/// (the bridge collects their failures as warnings, mirroring the oracle's
/// warn-only side effects).
pub fn worktree_add(request: &AddWorktreeRequest) -> Result<(), CoreError> {
    let entries = worktree_list(&request.repo_path)?;
    if entries
        .iter()
        .any(|entry| are_worktree_paths_equal(&entry.path, &request.worktree_path))
    {
        return Err(CoreError::InvalidInput(format!(
            "Worktree path already exists locally: {}",
            request.worktree_path
        )));
    }

    let args = [
        "worktree",
        "add",
        "--no-track",
        "-b",
        request.branch.as_str(),
        request.worktree_path.as_str(),
        request.base_ref.as_str(),
    ];
    let output = run_git_in(&request.repo_path, &args, WORKTREE_ADD_TIMEOUT, None)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_command_failed(&args, &output))
    }
}

/// Persist the creation base for the new branch.
///
/// Mirrors `persistWorktreeCreationBase`
/// (`orca:src/main/git/worktree-add.ts:70-95`) with the brief's stricter
/// failure recovery: when `--replace-all` fails, the whole stale
/// `branch.<branch>` section is dropped before the error surfaces, so no
/// consumer trusts stale lineage.
pub fn configure_branch_base(
    worktree_path: &str,
    branch: &str,
    base_ref: &str,
) -> Result<(), CoreError> {
    let key = format!("branch.{branch}.base");
    let args = ["config", "--local", "--replace-all", key.as_str(), base_ref];
    let output = run_git_in(worktree_path, &args, CONFIG_TIMEOUT, None)?;
    if output.status.success() {
        return Ok(());
    }

    let failure = git_command_failed(&args, &output);
    let section = format!("branch.{branch}");
    let _ = run_git_in(
        worktree_path,
        &["config", "--local", "--remove-section", section.as_str()],
        CONFIG_TIMEOUT,
        None,
    );
    Err(failure)
}

/// Enable `push.autoSetupRemote` for the repository behind `worktree_path`,
/// unless the user already chose a value at any scope.
///
/// Mirrors `configurePushAutoSetupRemote`
/// (`orca:src/main/git/worktree-add.ts:97-124`): a plain `git config --get`
/// (include/global included) wins, the repo-local value is checked too, and a
/// read failure other than "unset" (exit 1) must not overwrite config.
pub fn ensure_push_auto_setup_remote(worktree_path: &str) -> Result<(), CoreError> {
    if config_is_set(worktree_path, &["config", "--get", "push.autoSetupRemote"])? {
        return Ok(());
    }
    if config_is_set(
        worktree_path,
        &["config", "--local", "--get", "push.autoSetupRemote"],
    )? {
        return Ok(());
    }

    let args = ["config", "--local", "push.autoSetupRemote", "true"];
    let output = run_git_in(worktree_path, &args, CONFIG_TIMEOUT, None)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_command_failed(&args, &output))
    }
}

/// `git config --get` tri-state: `Ok(true)` set, `Ok(false)` unset (exit 1),
/// `Err` any other read failure — which must not read as "unset".
fn config_is_set(path: &str, args: &[&str]) -> Result<bool, CoreError> {
    let output = run_git_in(path, args, CONFIG_TIMEOUT, None)?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(git_command_failed(args, &output)),
    }
}
