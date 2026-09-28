//! Local staging, discard, commit and upstream status operations.
//!
//! Mirrors `orca:src/main/git/source-control/staging.ts`,
//! `discard-changes.ts`, `commit-changes.ts`, `git-pathspec.ts` and
//! `orca:src/shared/git-discard-path-safety.ts`: every path reaches Git as a
//! `:(literal)` pathspec, bulk writes split on [`BULK_PATHSPEC_CHUNK`], a
//! commit failure resolves as `{success:false,error}` instead of rejecting,
//! and upstream ahead/behind is measured locally without touching the network.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use ade_core::errors::CoreError;
use serde::Serialize;

use crate::runner::{run_git_in, GitOutput};
use crate::status::GitUpstreamStatus;

/// Ceiling on argv entries per bulk invocation, mirroring the oracle's
/// `BULK_CHUNK_SIZE` (`orca:src/main/git/source-control/git-pathspec.ts`).
pub const BULK_PATHSPEC_CHUNK: usize = 100;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of `git commit`, mirroring the oracle's `{success, error?}` shape.
/// Hook and identity failures resolve here instead of rejecting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CommitOutcome {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `:(literal)<path>` — every byte of `path` matches literally, so paths with
/// glob or pathspec-magic characters are staged as themselves.
pub fn literal_pathspec(path: &str) -> String {
    format!(":(literal){path}")
}

pub fn stage(worktree_path: &str, file_path: &str) -> Result<(), CoreError> {
    let pathspec = literal_pathspec(file_path);
    run_checked(worktree_path, &["add", "--", &pathspec])
}

pub fn unstage(worktree_path: &str, file_path: &str) -> Result<(), CoreError> {
    let pathspec = literal_pathspec(file_path);
    run_checked(worktree_path, &["restore", "--staged", "--", &pathspec])
}

pub fn bulk_stage(worktree_path: &str, file_paths: &[String]) -> Result<(), CoreError> {
    bulk_apply(worktree_path, &["add", "--"], file_paths)
}

pub fn bulk_unstage(worktree_path: &str, file_paths: &[String]) -> Result<(), CoreError> {
    bulk_apply(worktree_path, &["restore", "--staged", "--"], file_paths)
}

pub fn discard(worktree_path: &str, file_path: &str) -> Result<(), CoreError> {
    validate_discard_path(worktree_path, file_path)?;
    let pathspec = literal_pathspec(file_path);
    if is_tracked(worktree_path, &pathspec) {
        run_checked(
            worktree_path,
            &["restore", "--worktree", "--source=HEAD", "--", &pathspec],
        )
    } else {
        run_checked(worktree_path, &["clean", "-ffdx", "--", &pathspec])
    }
}

pub fn bulk_discard(worktree_path: &str, file_paths: &[String]) -> Result<(), CoreError> {
    if file_paths.is_empty() {
        return Ok(());
    }
    // Why: validate every target before anything mutates, mirroring the
    // oracle's up-front containment sweep in `bulkDiscardChanges`.
    for file_path in file_paths {
        validate_discard_path(worktree_path, file_path)?;
    }

    let mut tracked_paths: Vec<String> = Vec::new();
    for chunk in pathspec_chunks(&["ls-files", "-z", "--"], file_paths) {
        let args = as_args(&chunk);
        let output = run_git_in(worktree_path, &args, COMMAND_TIMEOUT, None)?;
        if !output.status.success() {
            return Err(git_command_failed(&args, &output));
        }
        for path in String::from_utf8_lossy(&output.stdout).split('\0') {
            if !path.is_empty() {
                tracked_paths.push(path.to_string());
            }
        }
    }

    let mut tracked: Vec<String> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();
    for file_path in file_paths {
        if is_tracked_path_spec(file_path, &tracked_paths) {
            tracked.push(file_path.clone());
        } else {
            untracked.push(file_path.clone());
        }
    }

    // Why: Git pathspec cleanup avoids raw recursive deletion through
    // symlinked parents; a pathspec-free `clean -ffdx` would sweep the whole
    // worktree.
    if !tracked.is_empty() {
        bulk_apply(
            worktree_path,
            &["restore", "--worktree", "--source=HEAD", "--"],
            &tracked,
        )?;
    }
    if !untracked.is_empty() {
        bulk_apply(worktree_path, &["clean", "-ffdx", "--"], &untracked)?;
    }
    Ok(())
}

/// `git commit -m <message>` only. Every git failure — a rejected commit
/// (hook, identity, nothing staged) or a runner-level failure (spawn, timeout,
/// cancellation) — resolves as `{success:false,error}` instead of rejecting,
/// matching the oracle's catch-all. Git's own text is preferred in the order
/// stderr → stdout → `"Commit failed"`; an empty message is the one input
/// validation that still rejects.
pub fn commit(worktree_path: &str, message: &str) -> Result<CommitOutcome, CoreError> {
    if message.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "Commit message is required".to_string(),
        ));
    }
    let output = match run_git_in(
        worktree_path,
        &["commit", "-m", message],
        COMMAND_TIMEOUT,
        None,
    ) {
        Ok(output) => output,
        Err(error) => return Ok(outcome_from_error(error.to_string())),
    };
    if output.status.success() {
        return Ok(CommitOutcome {
            success: true,
            error: None,
        });
    }
    let stderr = text(&output.stderr);
    let stdout = text(&output.stdout);
    let error = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "Commit failed".to_string()
    };
    Ok(outcome_from_error(error))
}

fn outcome_from_error(error: String) -> CommitOutcome {
    CommitOutcome {
        success: false,
        error: Some(error),
    }
}

/// Ahead/behind for the configured `@{upstream}`. A branch without one
/// reports `{has_upstream:false,ahead:0,behind:0}` instead of failing.
/// `has_configured_push_target` and `behind_commits_are_patch_equivalent` stay
/// unset (recorded deviation: no push-target or patch-equivalence probing).
pub fn upstream_status(worktree_path: &str) -> Result<GitUpstreamStatus, CoreError> {
    let Some(upstream_name) = configured_upstream_name(worktree_path) else {
        return Ok(no_upstream_status());
    };

    let range = format!("{upstream_name}...HEAD");
    let args = ["rev-list", "--left-right", "--count", &range];
    let output = run_git_in(worktree_path, &args, COMMAND_TIMEOUT, None)?;
    if !output.status.success() {
        return Err(git_command_failed(&args, &output));
    }
    // `--left-right` on `<upstream>...HEAD` prints `behind\tahead`.
    let (behind, ahead) = parse_behind_ahead(&output.stdout).unwrap_or((0, 0));

    Ok(GitUpstreamStatus {
        has_upstream: true,
        upstream_name: Some(upstream_name),
        ahead,
        behind,
        has_configured_push_target: None,
        behind_commits_are_patch_equivalent: None,
    })
}

fn no_upstream_status() -> GitUpstreamStatus {
    GitUpstreamStatus {
        has_upstream: false,
        upstream_name: None,
        ahead: 0,
        behind: 0,
        has_configured_push_target: None,
        behind_commits_are_patch_equivalent: None,
    }
}

/// Any failure reads as "no upstream": the command is expected to fail on a
/// branch without one, and the caller's contract maps that to a
/// `has_upstream:false` result.
fn configured_upstream_name(worktree_path: &str) -> Option<String> {
    let args = [
        "rev-parse",
        "--abbrev-ref",
        "--symbolic-full-name",
        "@{upstream}",
    ];
    match run_git_in(worktree_path, &args, COMMAND_TIMEOUT, None) {
        Ok(output) if output.status.success() => {
            let name = text(&output.stdout);
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        }
        _ => None,
    }
}

fn parse_behind_ahead(stdout: &[u8]) -> Option<(u64, u64)> {
    let line = String::from_utf8_lossy(stdout);
    let mut fields = line.split_whitespace();
    let behind = fields.next()?.parse().ok()?;
    let ahead = fields.next()?.parse().ok()?;
    Some((behind, ahead))
}

/// `ls-files --error-unmatch` proves tracked-ness; a non-zero exit for an
/// unmatched path reads as untracked, mirroring the oracle's `catch {}`.
fn is_tracked(worktree_path: &str, pathspec: &str) -> bool {
    matches!(
        run_git_in(
            worktree_path,
            &["ls-files", "--error-unmatch", "--", pathspec],
            COMMAND_TIMEOUT,
            None
        ),
        Ok(output) if output.status.success()
    )
}

/// One invocation's argv per chunk: the leading args plus up to
/// [`BULK_PATHSPEC_CHUNK`] literal pathspecs.
fn pathspec_chunks(leading: &[&str], file_paths: &[String]) -> Vec<Vec<String>> {
    file_paths
        .chunks(BULK_PATHSPEC_CHUNK)
        .map(|chunk| {
            let mut args: Vec<String> = leading.iter().map(|arg| (*arg).to_string()).collect();
            args.extend(chunk.iter().map(|path| literal_pathspec(path)));
            args
        })
        .collect()
}

fn bulk_apply(
    worktree_path: &str,
    leading: &[&str],
    file_paths: &[String],
) -> Result<(), CoreError> {
    for chunk in pathspec_chunks(leading, file_paths) {
        run_checked(worktree_path, &as_args(&chunk))?;
    }
    Ok(())
}

fn run_checked(worktree_path: &str, args: &[&str]) -> Result<(), CoreError> {
    let output = run_git_in(worktree_path, args, COMMAND_TIMEOUT, None)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_command_failed(args, &output))
    }
}

fn git_command_failed(args: &[&str], output: &GitOutput) -> CoreError {
    CoreError::GitCommandFailed {
        command: args.join(" "),
        stderr: text(&output.stderr),
        exit_code: output.status.code(),
    }
}

fn as_args(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

/// Mirrors the oracle's `isTrackedPathSpec`: an exact tracked path, or a path
/// that is a parent directory of tracked paths.
fn is_tracked_path_spec(file_path: &str, tracked_paths: &[String]) -> bool {
    let normalized = normalize_git_path_for_compare(file_path);
    tracked_paths.iter().any(|tracked_path| {
        let normalized_tracked = normalize_git_path_for_compare(tracked_path);
        normalized_tracked == normalized
            || normalized_tracked.starts_with(&format!("{normalized}/"))
    })
}

fn normalize_git_path_for_compare(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_string()
}

/// Proves a target path is inside the worktree before any discard runs.
///
/// First lexically (mirroring the oracle's `path.resolve`/`isWithinWorktree`
/// comparison), then against real paths: a symlink leaf is checked via its
/// parent so discarding removes the link itself, while a missing leaf falls
/// back to its nearest existing ancestor so a symlinked parent cannot redirect
/// the target outside the worktree
/// (`orca:src/shared/git-discard-path-safety.ts`).
fn validate_discard_path(worktree_path: &str, file_path: &str) -> Result<(), CoreError> {
    let worktree = Path::new(worktree_path);
    let normalized_worktree = normalize_lexically(worktree);
    let normalized_target = normalize_lexically(&worktree.join(file_path));

    // Why: the worktree root itself is never a valid discard target, even for
    // an empty or self-referential path.
    if normalized_target == normalized_worktree
        || !normalized_target.starts_with(&normalized_worktree)
    {
        return Err(CoreError::PathNotAllowed(file_path.to_string()));
    }

    let real_worktree = std::fs::canonicalize(&normalized_worktree)
        .map_err(|_| CoreError::PathNotAllowed(file_path.to_string()))?;
    let real_target = resolve_real_path(&normalized_target)
        .map_err(|_| CoreError::PathNotAllowed(file_path.to_string()))?;
    if real_target == real_worktree || !real_target.starts_with(&real_worktree) {
        return Err(CoreError::PathNotAllowed(file_path.to_string()));
    }
    Ok(())
}

/// Lexical normalization (`.`, `..`, trailing slashes) equivalent to Node's
/// `path.resolve`, so dot segments never reach the filesystem resolver.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Component::RootDir.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !path.is_absolute() {
                    normalized.push(Component::ParentDir.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

/// Real path of a possibly-missing leaf. The nearest existing ancestor is
/// canonicalized (following symlinks) and the missing tail is appended; an
/// exact symlink leaf is deliberately not followed.
fn resolve_real_path(path: &Path) -> io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let parent = path.parent().ok_or_else(no_existing_ancestor)?;
            std::fs::canonicalize(parent)
                .map(|resolved| resolved.join(path.file_name().unwrap_or_default()))
        }
        Ok(_) => std::fs::canonicalize(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut ancestor = path.parent();
            while let Some(current) = ancestor {
                if let Ok(resolved) = std::fs::canonicalize(current) {
                    let mut result = resolved;
                    if let Ok(remainder) = path.strip_prefix(current) {
                        result.push(remainder);
                    }
                    return Ok(result);
                }
                ancestor = current.parent();
            }
            Err(no_existing_ancestor())
        }
        Err(error) => Err(error),
    }
}

fn no_existing_ancestor() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "path has no existing ancestor")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_pathspec_wraps_the_path_in_literal_magic() {
        assert_eq!(literal_pathspec("src/app.ts"), ":(literal)src/app.ts");
        assert_eq!(literal_pathspec("star*.txt"), ":(literal)star*.txt");
        assert_eq!(literal_pathspec(""), ":(literal)");
    }

    #[test]
    fn pathspec_chunks_split_every_hundred_paths_and_keep_the_prefix() {
        let paths: Vec<String> = (0..250).map(|index| format!("f{index:03}.txt")).collect();

        let chunks = pathspec_chunks(&["add", "--"], &paths);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 2 + 100);
        assert_eq!(chunks[1].len(), 2 + 100);
        assert_eq!(chunks[2].len(), 2 + 50);
        assert_eq!(chunks[0][0], "add");
        assert_eq!(chunks[0][1], "--");
        assert_eq!(chunks[0][2], ":(literal)f000.txt");
        assert_eq!(chunks[2][2], ":(literal)f200.txt");
        assert_eq!(chunks[2].last().unwrap(), ":(literal)f249.txt");
        assert!(pathspec_chunks(&["restore", "--staged", "--"], &[]).is_empty());
    }

    #[test]
    fn tracked_path_spec_matches_exact_paths_and_tracked_path_parents() {
        let tracked = vec!["src/app.ts".to_string(), "docs/readme.md".to_string()];

        assert!(is_tracked_path_spec("src/app.ts", &tracked));
        assert!(is_tracked_path_spec("src", &tracked));
        assert!(is_tracked_path_spec("src/", &tracked));
        assert!(!is_tracked_path_spec("src/app", &tracked));
        assert!(!is_tracked_path_spec("src2", &tracked));
        assert!(!is_tracked_path_spec("new.txt", &tracked));
    }

    #[test]
    fn normalize_lexically_resolves_dot_segments_and_trailing_slashes() {
        assert_eq!(
            normalize_lexically(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_lexically(Path::new("/a/./b/")),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            normalize_lexically(Path::new("/a/b/..")),
            PathBuf::from("/a")
        );
        assert_eq!(normalize_lexically(Path::new("/../a")), PathBuf::from("/a"));
    }
}
