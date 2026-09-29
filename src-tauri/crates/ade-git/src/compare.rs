//! Branch and commit compare, mirroring `orca:src/main/git/source-control/`:
//! `branch-compare.ts`, `commit-compare.ts`, `branch-change-entries.ts`,
//! `branch-diff.ts`, `commit-diff.ts` and `compare-ref-oids.ts`. Compare
//! failures fold into the summary `status` state machine instead of rejecting;
//! only the diff readers reuse [`crate::diff::diff_refs`] and can fail.

use ade_core::errors::CoreError;
use serde::Serialize;

use crate::command::{run, run_text};
use crate::diff::{diff_refs, DiffSide, GitDiffResult};

/// One changed path in a branch/commit compare, mirroring the renderer's
/// `GitBranchChangeStatus` (`src/shared/git-status-types.ts`) without
/// `untracked`: compared trees can never grow an untracked file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum GitBranchChangeStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
}

/// Mirrors `GitBranchChangeEntry` in `src/shared/git-diff-compare-types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchChangeEntry {
    pub path: String,
    pub status: GitBranchChangeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,
}

/// Mirrors `GitBranchCompareSummary` in `src/shared/git-diff-compare-types.ts`.
/// `status` takes `'ready' | 'invalid-base' | 'unborn-head' | 'no-merge-base' |
/// 'error'`; `base_oid`/`head_oid`/`merge_base` serialize as `null` when the
/// oracle keeps them nullable.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCompareSummary {
    pub base_ref: String,
    pub base_oid: Option<String>,
    pub compare_ref: String,
    pub head_oid: Option<String>,
    pub merge_base: Option<String>,
    pub changed_files: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits_ahead: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits_behind: Option<u64>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Mirrors `GitBranchCompareResult` in `src/shared/git-diff-compare-types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitBranchCompareResult {
    pub summary: GitBranchCompareSummary,
    pub entries: Vec<GitBranchChangeEntry>,
}

/// Mirrors `GitCommitCompareSummary` in `src/shared/git-diff-compare-types.ts`.
/// `status` takes `'ready' | 'invalid-commit' | 'error'`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitCommitCompareSummary {
    pub commit_oid: String,
    pub parent_oid: Option<String>,
    pub compare_ref: String,
    pub base_ref: String,
    pub changed_files: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Mirrors `GitCommitCompareResult` in `src/shared/git-diff-compare-types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitCommitCompareResult {
    pub summary: GitCommitCompareSummary,
    pub entries: Vec<GitBranchChangeEntry>,
}

/// Mirrors `getBranchCompare` (`branch-compare.ts:17`): resolves the base ref
/// and HEAD, follows the summary state machine, then lists the changed files
/// from the merge base to HEAD.
pub fn branch_compare(
    worktree_path: &str,
    base_ref: &str,
) -> Result<GitBranchCompareResult, CoreError> {
    let mut summary = GitBranchCompareSummary {
        base_ref: base_ref.to_string(),
        base_oid: None,
        compare_ref: "HEAD".to_string(),
        head_oid: None,
        merge_base: None,
        changed_files: 0,
        commits_ahead: None,
        commits_behind: None,
        status: "loading".to_string(),
        error_message: None,
    };

    summary.compare_ref = resolve_compare_ref(worktree_path);
    let resolved_base_ref = resolve_worktree_add_base_ref(worktree_path, base_ref);
    let head_oid = resolve_ref_oid(worktree_path, "HEAD");
    let base_oid = resolve_ref_oid(worktree_path, &resolved_base_ref);

    let head_oid = match head_oid {
        Ok(oid) => {
            summary.head_oid = Some(oid.clone());
            oid
        }
        Err(_) => {
            if let Ok(oid) = base_oid {
                // Why: an unborn branch (new remote worktree) has no changes
                // yet; a compare error would look broken.
                summary.base_oid = Some(oid);
                summary.commits_ahead = Some(0);
                summary.commits_behind = Some(0);
                summary.status = "ready".to_string();
                return Ok(GitBranchCompareResult {
                    summary,
                    entries: Vec::new(),
                });
            }
            summary.status = "unborn-head".to_string();
            summary.error_message = Some(
                "This branch does not have a committed HEAD yet, so compare-to-base is unavailable."
                    .to_string(),
            );
            return Ok(GitBranchCompareResult {
                summary,
                entries: Vec::new(),
            });
        }
    };

    let base_oid = match base_oid {
        Ok(oid) => {
            summary.base_oid = Some(oid.clone());
            oid
        }
        Err(_) => {
            summary.status = "invalid-base".to_string();
            summary.error_message = Some(format!(
                "Base ref {base_ref} could not be resolved in this repository."
            ));
            return Ok(GitBranchCompareResult {
                summary,
                entries: Vec::new(),
            });
        }
    };

    let merge_base = match resolve_merge_base(worktree_path, &base_oid, &head_oid) {
        Ok(merge_base) => {
            summary.merge_base = Some(merge_base.clone());
            merge_base
        }
        Err(_) => {
            summary.status = "no-merge-base".to_string();
            summary.error_message = Some(format!(
                "This branch and {base_ref} do not share a merge base, so compare-to-base is unavailable."
            ));
            return Ok(GitBranchCompareResult {
                summary,
                entries: Vec::new(),
            });
        }
    };

    match (
        load_branch_changes(worktree_path, &merge_base, &head_oid),
        count_compare_divergence(worktree_path, &base_oid, &head_oid),
    ) {
        (Ok(entries), Ok((ahead, behind))) => {
            summary.changed_files = entries.len() as u64;
            summary.commits_ahead = Some(ahead);
            summary.commits_behind = Some(behind);
            summary.status = "ready".to_string();
            Ok(GitBranchCompareResult { summary, entries })
        }
        (Err(error), _) | (_, Err(error)) => {
            summary.status = "error".to_string();
            summary.error_message = Some(error.to_string());
            Ok(GitBranchCompareResult {
                summary,
                entries: Vec::new(),
            })
        }
    }
}

/// Mirrors `getCommitCompare` (`commit-compare.ts:9`): resolves the commit,
/// finds its first parent, then lists the tree delta against that parent (or
/// the empty tree for a root commit).
pub fn commit_compare(
    worktree_path: &str,
    commit_id: &str,
) -> Result<GitCommitCompareResult, CoreError> {
    let commit_oid = match resolve_ref_oid(worktree_path, &format!("{commit_id}^{{commit}}")) {
        Ok(oid) => oid,
        Err(_) => {
            return Ok(GitCommitCompareResult {
                summary: GitCommitCompareSummary {
                    commit_oid: String::new(),
                    parent_oid: None,
                    compare_ref: commit_id.to_string(),
                    base_ref: "parent".to_string(),
                    changed_files: 0,
                    status: "invalid-commit".to_string(),
                    error_message: Some(format!(
                        "Commit {commit_id} could not be resolved in this repository."
                    )),
                },
                entries: Vec::new(),
            });
        }
    };

    let mut summary = GitCommitCompareSummary {
        commit_oid: commit_oid.clone(),
        parent_oid: None,
        compare_ref: short_oid(&commit_oid),
        base_ref: "empty tree".to_string(),
        changed_files: 0,
        status: "ready".to_string(),
        error_message: None,
    };

    let parent_oid = match run_text(
        worktree_path,
        &["rev-list", "--parents", "-n", "1", &commit_oid],
    ) {
        Ok(stdout) => parse_first_parent(&stdout),
        Err(error) => {
            summary.status = "error".to_string();
            summary.error_message = Some(error.to_string());
            return Ok(GitCommitCompareResult {
                summary,
                entries: Vec::new(),
            });
        }
    };

    summary.base_ref = parent_oid
        .as_deref()
        .map(short_oid)
        .unwrap_or_else(|| "empty tree".to_string());

    match load_commit_changes(worktree_path, parent_oid.as_deref(), &commit_oid) {
        Ok(entries) => {
            summary.parent_oid = parent_oid;
            summary.changed_files = entries.len() as u64;
            Ok(GitCommitCompareResult { summary, entries })
        }
        Err(error) => {
            summary.parent_oid = parent_oid;
            summary.status = "error".to_string();
            summary.error_message = Some(error.to_string());
            Ok(GitCommitCompareResult {
                summary,
                entries: Vec::new(),
            })
        }
    }
}

/// Mirrors `getBranchDiff` (`branch-diff.ts:9`): both sides read whole blobs,
/// the left at `base_oid:<old_path ?? file_path>`, the right at
/// `head_oid:<file_path>`.
pub fn branch_diff(
    worktree_path: &str,
    base_oid: &str,
    head_oid: &str,
    file_path: &str,
    old_path: Option<&str>,
) -> Result<GitDiffResult, CoreError> {
    diff_refs(
        worktree_path,
        &DiffSide::Rev {
            rev: base_oid.to_string(),
            path: file_path.to_string(),
        },
        &DiffSide::Rev {
            rev: head_oid.to_string(),
            path: file_path.to_string(),
        },
        file_path,
        old_path,
    )
}

/// Mirrors `getCommitDiff` (`commit-diff.ts:9`): the left side is the parent
/// tree (`Empty` for a root commit), the right side the commit.
pub fn commit_diff(
    worktree_path: &str,
    commit_oid: &str,
    parent_oid: Option<&str>,
    file_path: &str,
    old_path: Option<&str>,
) -> Result<GitDiffResult, CoreError> {
    let left = match parent_oid {
        Some(parent) => DiffSide::Rev {
            rev: parent.to_string(),
            path: file_path.to_string(),
        },
        None => DiffSide::Empty,
    };
    diff_refs(
        worktree_path,
        &left,
        &DiffSide::Rev {
            rev: commit_oid.to_string(),
            path: file_path.to_string(),
        },
        file_path,
        old_path,
    )
}

/// `resolveCompareRef` (`compare-ref-oids.ts:5`): the short current branch
/// name, or `HEAD` for a detached head or a git failure.
fn resolve_compare_ref(worktree_path: &str) -> String {
    match run_text(worktree_path, &["branch", "--show-current"]) {
        Ok(branch) if !branch.is_empty() => branch,
        _ => "HEAD".to_string(),
    }
}

/// `resolveRefOid` (`compare-ref-oids.ts:20`): the ref's raw oid, never
/// peeled — remote-tracking refs may store annotated tags whose raw oid must
/// be preserved.
fn resolve_ref_oid(worktree_path: &str, reference: &str) -> Result<String, CoreError> {
    run_text(
        worktree_path,
        &["rev-parse", "--verify", "--end-of-options", reference],
    )
}

fn resolve_merge_base(
    worktree_path: &str,
    base_oid: &str,
    head_oid: &str,
) -> Result<String, CoreError> {
    run_text(worktree_path, &["merge-base", base_oid, head_oid])
}

fn resolve_worktree_base_commit_oid(worktree_path: &str, qualified_ref: &str) -> bool {
    run(
        worktree_path,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{qualified_ref}^{{commit}}"),
        ],
    )
    .is_ok()
}

/// `resolveWorktreeAddBaseRef` (`shared/worktree/base-ref.ts:3`): short refs
/// prefer the remote-tracking namespace when they contain a slash, then local
/// branches; an unresolvable ref is returned unchanged.
fn resolve_worktree_add_base_ref(worktree_path: &str, base_ref: &str) -> String {
    if base_ref.starts_with("refs/") {
        return base_ref.to_string();
    }
    let candidates = if base_ref.contains('/') {
        vec![
            format!("refs/remotes/{base_ref}"),
            format!("refs/heads/{base_ref}"),
        ]
    } else {
        vec![format!("refs/heads/{base_ref}")]
    };
    for candidate in &candidates {
        if resolve_worktree_base_commit_oid(worktree_path, candidate) {
            return candidate.clone();
        }
    }
    base_ref.to_string()
}

/// `countCompareDivergence` (`compare-ref-oids.ts:45`): `--left-right --count
/// <base>...<head>` reports behind first, then ahead.
fn count_compare_divergence(
    worktree_path: &str,
    base_oid: &str,
    head_oid: &str,
) -> Result<(u64, u64), CoreError> {
    let stdout = run_text(
        worktree_path,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base_oid}...{head_oid}"),
        ],
    )?;
    let mut parts = stdout.split_whitespace();
    let behind = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let ahead = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    Ok((ahead, behind))
}

/// `loadBranchChanges` (`branch-change-entries.ts:27`), read through the `-z`
/// form so paths never need C-quote decoding. Rename/copy entries carry the
/// preimage as a second NUL fragment.
fn load_branch_changes(
    worktree_path: &str,
    merge_base: &str,
    head_oid: &str,
) -> Result<Vec<GitBranchChangeEntry>, CoreError> {
    let name_status = run(
        worktree_path,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--name-status",
            "-z",
            "-M",
            "-C",
            merge_base,
            head_oid,
        ],
    )?;
    let numstat = run(
        worktree_path,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "-z",
            "--numstat",
            "-M",
            "-C",
            merge_base,
            head_oid,
        ],
    )?;
    Ok(parse_name_status_z(
        &name_status,
        &crate::status_read::parse_numstat(&numstat),
    ))
}

/// `loadCommitChanges` (`branch-change-entries.ts:65`): root commits use
/// `diff-tree --root`, which relies on git's own empty tree instead of a
/// hash-format-specific oid.
fn load_commit_changes(
    worktree_path: &str,
    parent_oid: Option<&str>,
    commit_oid: &str,
) -> Result<Vec<GitBranchChangeEntry>, CoreError> {
    let (name_status_args, numstat_args) = match parent_oid {
        Some(parent) => (
            vec![
                "-c",
                "core.quotePath=false",
                "diff",
                "--name-status",
                "-z",
                "-M",
                "-C",
                parent,
                commit_oid,
            ],
            vec![
                "-c",
                "core.quotePath=false",
                "diff",
                "-z",
                "--numstat",
                "-M",
                "-C",
                parent,
                commit_oid,
            ],
        ),
        None => (
            vec![
                "-c",
                "core.quotePath=false",
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--name-status",
                "-r",
                "-M",
                "-C",
                "-z",
                commit_oid,
            ],
            vec![
                "-c",
                "core.quotePath=false",
                "diff-tree",
                "-z",
                "--root",
                "--no-commit-id",
                "--numstat",
                "-r",
                "-M",
                "-C",
                commit_oid,
            ],
        ),
    };
    let name_status = run(worktree_path, &name_status_args)?;
    let numstat = run(worktree_path, &numstat_args)?;
    Ok(parse_name_status_z(
        &name_status,
        &crate::status_read::parse_numstat(&numstat),
    ))
}

fn parse_branch_status_char(char: char) -> GitBranchChangeStatus {
    match char {
        'M' => GitBranchChangeStatus::Modified,
        'A' => GitBranchChangeStatus::Added,
        'D' => GitBranchChangeStatus::Deleted,
        'R' => GitBranchChangeStatus::Renamed,
        'C' => GitBranchChangeStatus::Copied,
        _ => GitBranchChangeStatus::Modified,
    }
}

fn parse_name_status_z(
    bytes: &[u8],
    stats: &std::collections::HashMap<String, crate::status_read::GitLineStats>,
) -> Vec<GitBranchChangeEntry> {
    let records: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.is_empty() {
            index += 1;
            continue;
        }
        let raw_status = String::from_utf8_lossy(record);
        let status = parse_branch_status_char(raw_status.chars().next().unwrap_or('M'));
        if raw_status.starts_with('R') || raw_status.starts_with('C') {
            let old_path = records
                .get(index + 1)
                .map(|value| String::from_utf8_lossy(value).to_string())
                .unwrap_or_default();
            let path = records
                .get(index + 2)
                .map(|value| String::from_utf8_lossy(value).to_string())
                .unwrap_or_default();
            if !path.is_empty() {
                entries.push(build_entry(path, Some(old_path), status, stats));
            }
            index += 3;
        } else {
            let path = records
                .get(index + 1)
                .map(|value| String::from_utf8_lossy(value).to_string())
                .unwrap_or_default();
            if !path.is_empty() {
                entries.push(build_entry(path, None, status, stats));
            }
            index += 2;
        }
    }
    entries
}

fn build_entry(
    path: String,
    old_path: Option<String>,
    status: GitBranchChangeStatus,
    stats: &std::collections::HashMap<String, crate::status_read::GitLineStats>,
) -> GitBranchChangeEntry {
    let stats = stats.get(&path);
    GitBranchChangeEntry {
        path,
        status,
        old_path,
        added: stats.and_then(|value| value.added),
        removed: stats.and_then(|value| value.removed),
    }
}

/// `parseGitRevListFirstParentOid` (`shared/git-rev-list-output.ts`): the
/// first token after the commit oid on the first non-empty line.
fn parse_first_parent(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| line.split_whitespace().nth(1))
        .map(|value| value.to_string())
}

fn short_oid(oid: &str) -> String {
    oid.chars().take(7).collect()
}
