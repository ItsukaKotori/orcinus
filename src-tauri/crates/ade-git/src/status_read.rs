//! `git status` execution, unmerged-row resolution, and working-tree line
//! statistics.
//!
//! Mirrors `orca:src/main/git/source-control/status-read.ts`,
//! `status-line-stats.ts`, `git-conflict-operation.ts` and
//! `orca:src/shared/git-status-conflict-entries.ts`. A status that fails for
//! any reason other than cancellation still resolves as an empty result, so
//! polling a non-repository path is not an error.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ade_core::errors::CoreError;

use crate::runner::{run_git_in, CancelToken};
use crate::status::{
    GitBranchLineTotal, GitConflictKind, GitConflictOperation, GitConflictResolutionStatus,
    GitFileStatus, GitStagingArea, GitStatusEntry, GitStatusResult, GitUpstreamStatus,
    ParsedStatus, StatusParser, StatusRecord,
};

pub const DEFAULT_GIT_STATUS_LIMIT: usize = 1000;

const STATUS_TIMEOUT: Duration = Duration::from_secs(120);
const NUMSTAT_TIMEOUT: Duration = Duration::from_secs(30);
const BRANCH_LINE_TOTAL_TIMEOUT: Duration = Duration::from_secs(15);
/// Keeps status polling cheap: large untracked files are commonly generated
/// assets, and reading them every poll stalls the source-control sidebar.
const MAX_UNTRACKED_LINE_COUNT_BYTES: u64 = 2 * 1024 * 1024;
/// A NUL byte in the first chunk is Git's own heuristic for "this is binary".
const BINARY_SNIFF_BYTES: usize = 8192;

#[derive(Debug, Clone, Default)]
pub struct StatusOptions {
    /// Max changed rows before git is stopped and the result is marked
    /// `did_hit_limit`; `None` (and negatives) resolve to the default cap, `0`
    /// disables the cap.
    pub limit: Option<i64>,
    pub include_ignored: bool,
    pub include_line_stats: bool,
    /// Merge-base OID the caller wants the branch line total measured against;
    /// absent means no ranged diff runs at all.
    pub branch_line_total_merge_base: Option<String>,
}

/// A bad limit (negative) would break early-stop, so require a valid
/// non-negative integer; 0 disables the cap.
pub fn resolve_status_limit(limit: Option<i64>) -> usize {
    match limit {
        Some(value) if value >= 0 => value as usize,
        _ => DEFAULT_GIT_STATUS_LIMIT,
    }
}

pub fn status(
    worktree_path: &str,
    options: &StatusOptions,
    cancel: Option<&CancelToken>,
) -> Result<GitStatusResult, CoreError> {
    let limit = resolve_status_limit(options.limit);
    let conflict_operation = conflict_operation(worktree_path)?;

    // Why: core.quotePath=false keeps non-ASCII paths as raw UTF-8, not octal
    // escapes, so entry.path is readable and lookups match.
    let mut args: Vec<&str> = vec![
        "-c",
        "core.quotePath=false",
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=all",
        "-z",
    ];
    if options.include_ignored {
        args.push("--ignored=matching");
    }

    // Why: an aborted scan must reject, not resolve as an empty result; every
    // other failure (not a repo, git missing, timeout) is "no status".
    let (status_succeeded, stdout) = match run_git_in(worktree_path, &args, STATUS_TIMEOUT, cancel)
    {
        Ok(output) if output.status.success() => (true, output.stdout),
        Ok(_) => (false, Vec::new()),
        Err(error @ CoreError::GitCommandCancelled { .. }) => return Err(error),
        Err(_) => (false, Vec::new()),
    };

    // Why: the runner buffers stdout, so the parser sees the whole listing in
    // one chunk; it still stops at the cap and keeps the count it saw, and the
    // caller slices the ordered records down to the limit below.
    let mut parser = StatusParser::new();
    let did_hit_limit = if status_succeeded {
        parser.update(&stdout, limit)
    } else {
        false
    };
    if !did_hit_limit {
        parser.finish();
    }
    let parsed = parser.into_parsed();

    let mut result = GitStatusResult {
        entries: collect_entries(worktree_path, &parsed, limit, did_hit_limit),
        conflict_operation,
        head: parsed.head.clone(),
        branch: parsed.branch.clone(),
        upstream_status: None,
        ignored_paths: None,
        did_hit_limit: None,
        status_length: None,
        branch_line_total: None,
    };

    if status_succeeded {
        result.upstream_status = Some(match parsed.upstream_name.as_deref() {
            Some(upstream_name) => GitUpstreamStatus {
                has_upstream: true,
                upstream_name: Some(upstream_name.to_string()),
                ahead: parsed.ahead_behind.map(|(ahead, _)| ahead).unwrap_or(0),
                behind: parsed.ahead_behind.map(|(_, behind)| behind).unwrap_or(0),
                has_configured_push_target: None,
                behind_commits_are_patch_equivalent: None,
            },
            None => GitUpstreamStatus {
                has_upstream: false,
                upstream_name: None,
                ahead: 0,
                behind: 0,
                has_configured_push_target: None,
                behind_commits_are_patch_equivalent: None,
            },
        });
    }

    if options.include_ignored {
        result.ignored_paths = Some(parsed.ignored_paths.clone());
    }

    if did_hit_limit {
        result.did_hit_limit = Some(true);
        result.status_length = Some(parsed.changed_count);
    }

    // Why: line counts run only for areas with entries (clean tree = no calls);
    // a capped listing skips them so numstat never runs over a huge set.
    if status_succeeded && !did_hit_limit {
        if options.include_line_stats {
            attach_line_stats(worktree_path, &mut result.entries)?;
        }
        if let Some(merge_base) = options
            .branch_line_total_merge_base
            .as_deref()
            .and_then(valid_merge_base)
        {
            result.branch_line_total = branch_line_total(worktree_path, merge_base, cancel)?;
        }
    }

    // Why: a cancellation that lands after Git exited still must reject, not
    // publish a result computed after the caller gave up.
    if cancel.is_some_and(CancelToken::is_cancelled) {
        return Err(CoreError::GitCommandCancelled {
            command: args.join(" "),
        });
    }

    Ok(result)
}

/// Resolves changed rows in Git's output order, stopping at `limit` resolved
/// rows so the cap cannot hide an early conflict behind later ordinary ones.
fn collect_entries(
    worktree_path: &str,
    parsed: &ParsedStatus,
    limit: usize,
    did_hit_limit: bool,
) -> Vec<GitStatusEntry> {
    let mut entries = Vec::new();
    for record in &parsed.records {
        if did_hit_limit && entries.len() >= limit {
            break;
        }
        match record {
            StatusRecord::Entry(entry) => entries.push(entry.clone()),
            StatusRecord::Unmerged(line) => {
                if let Some(entry) = parse_unmerged_entry(worktree_path, line) {
                    entries.push(entry);
                }
            }
        }
    }
    entries
}

/// Detects an in-progress merge/rebase/cherry-pick from the worktree's Git
/// metadata directory. Rebase is read from the persist-all-steps
/// `rebase-merge`/`rebase-apply` directories rather than the lingering,
/// partial `REBASE_HEAD` marker, and merge wins over the others.
pub fn conflict_operation(worktree_path: &str) -> Result<GitConflictOperation, CoreError> {
    let git_dir = resolve_git_dir(worktree_path);
    if git_dir.join("MERGE_HEAD").exists() {
        return Ok(GitConflictOperation::Merge);
    }
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        return Ok(GitConflictOperation::Rebase);
    }
    if git_dir.join("CHERRY_PICK_HEAD").exists() {
        return Ok(GitConflictOperation::CherryPick);
    }
    Ok(GitConflictOperation::Unknown)
}

/// A linked worktree's `.git` is a file pointing at its per-worktree metadata
/// directory; a main checkout's `.git` is that directory itself.
fn resolve_git_dir(worktree_path: &str) -> PathBuf {
    let dot_git = Path::new(worktree_path).join(".git");
    if dot_git.is_dir() {
        return dot_git;
    }
    if let Ok(content) = std::fs::read_to_string(&dot_git) {
        if let Some(payload) = parse_gitdir_marker(&content) {
            let path = Path::new(&payload);
            return if path.is_absolute() {
                path.to_path_buf()
            } else {
                Path::new(worktree_path).join(path)
            };
        }
    }
    dot_git
}

/// The `gitdir:` payload of a `.git` gitfile, mirroring Git's own
/// `read_gitfile_gently`: the marker must start the first line and the payload
/// is trimmed. The prefix is compared as *bytes*: a `.git` file can hold
/// arbitrary text, and byte-slicing the `&str` would panic when a multibyte
/// character straddles the marker boundary (e.g. a line starting `résumé`).
fn parse_gitdir_marker(content: &str) -> Option<String> {
    const MARKER: &[u8] = b"gitdir:";
    let first_line = content.lines().next()?;
    let is_marker = first_line
        .as_bytes()
        .get(..MARKER.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(MARKER));
    if !is_marker {
        return None;
    }
    // The byte comparison proves the first `MARKER.len()` bytes are the ASCII
    // marker, so byte index `MARKER.len()` is a char boundary.
    let value = first_line[MARKER.len()..].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Resolves one raw `u ` porcelain v2 record. Returns `None` when the row is
/// not a resolvable conflict, so a malformed record never invents a row.
pub fn parse_unmerged_entry(worktree_path: &str, line: &str) -> Option<GitStatusEntry> {
    let parts: Vec<&str> = line.split(' ').collect();
    let xy = *parts.get(1)?;
    let mode_stage1 = parts.get(3)?;
    let mode_stage2 = parts.get(4)?;
    let mode_stage3 = parts.get(5)?;
    let mode_worktree = parts.get(6)?;
    let file_path = parts.get(10..)?.join(" ");
    if file_path.is_empty() {
        return None;
    }

    // Why: submodule conflicts (mode 160000) are out of scope for v1 — they
    // need different resolution UX.
    if [mode_stage1, mode_stage2, mode_stage3]
        .iter()
        .any(|mode| **mode == "160000")
    {
        return None;
    }

    let conflict_kind = parse_conflict_kind(xy)?;
    let status =
        conflict_compatibility_status(worktree_path, &file_path, &conflict_kind, mode_worktree);

    // Why: porcelain v2 `u` records lack rename-origin metadata, so old_path
    // is intentionally omitted; `status` here is a rendering-compat choice for
    // icon/color plumbing, not a semantic claim.
    Some(GitStatusEntry {
        path: file_path,
        status,
        area: GitStagingArea::Unstaged,
        old_path: None,
        conflict_kind: Some(conflict_kind),
        conflict_status: Some(GitConflictResolutionStatus::Unresolved),
        conflict_status_source: None,
        submodule: None,
        submodule_root: None,
        added: None,
        removed: None,
    })
}

fn parse_conflict_kind(xy: &str) -> Option<GitConflictKind> {
    match xy {
        "UU" => Some(GitConflictKind::BothModified),
        "AA" => Some(GitConflictKind::BothAdded),
        "DD" => Some(GitConflictKind::BothDeleted),
        "AU" => Some(GitConflictKind::AddedByUs),
        "UA" => Some(GitConflictKind::AddedByThem),
        "DU" => Some(GitConflictKind::DeletedByUs),
        "UD" => Some(GitConflictKind::DeletedByThem),
        _ => None,
    }
}

fn conflict_compatibility_status(
    worktree_path: &str,
    file_path: &str,
    conflict_kind: &GitConflictKind,
    mode_worktree: &str,
) -> GitFileStatus {
    match conflict_kind {
        GitConflictKind::BothModified | GitConflictKind::BothAdded => GitFileStatus::Modified,
        GitConflictKind::BothDeleted => GitFileStatus::Deleted,
        _ => {
            // Why: `mW` is the worktree mode Git already stat'ed for this row —
            // `000000` means absent, so no extra probe is needed.
            if is_octal_file_mode(mode_worktree) {
                return if mode_worktree == "000000" {
                    GitFileStatus::Deleted
                } else {
                    GitFileStatus::Modified
                };
            }
            // Why: only reachable on output no real Git emits (truncated or
            // malformed `u` record); any definite absence reads as deleted.
            match std::fs::metadata(Path::new(worktree_path).join(file_path)) {
                Ok(_) => GitFileStatus::Modified,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    GitFileStatus::Deleted
                }
                Err(_) => GitFileStatus::Modified,
            }
        }
    }
}

fn is_octal_file_mode(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| (b'0'..=b'7').contains(&byte))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct GitLineStats {
    pub(crate) added: Option<u64>,
    pub(crate) removed: Option<u64>,
}

/// Attaches per-entry working-tree line counts. A failed numstat pass leaves
/// rows uncounted instead of failing the whole status; only cancellation would
/// reject, and this signature carries no token, so it is effectively
/// infallible.
pub fn attach_line_stats(
    worktree_path: &str,
    entries: &mut [GitStatusEntry],
) -> Result<(), CoreError> {
    if entries.is_empty() {
        return Ok(());
    }

    let has_staged = entries
        .iter()
        .any(|entry| entry.area == GitStagingArea::Staged);
    let has_unstaged = entries
        .iter()
        .any(|entry| entry.area == GitStagingArea::Unstaged);

    let staged_stats = if has_staged {
        run_numstat(worktree_path, true)
    } else {
        None
    };
    let unstaged_stats = if has_unstaged {
        run_numstat(worktree_path, false)
    } else {
        None
    };
    let untracked_stats: HashMap<String, GitLineStats> = entries
        .iter()
        .filter(|entry| entry.area == GitStagingArea::Untracked)
        .filter_map(|entry| {
            let absolute = Path::new(worktree_path).join(&entry.path);
            count_untracked_additions(&absolute).map(|stats| (entry.path.clone(), stats))
        })
        .collect();

    for entry in entries.iter_mut() {
        let stats = match entry.area {
            GitStagingArea::Staged => staged_stats
                .as_ref()
                .and_then(|stats| stats.get(&entry.path)),
            GitStagingArea::Unstaged => unstaged_stats
                .as_ref()
                .and_then(|stats| stats.get(&entry.path)),
            GitStagingArea::Untracked => untracked_stats.get(&entry.path),
        };
        let Some(stats) = stats else {
            continue;
        };
        if let Some(added) = stats.added {
            entry.added = Some(added);
        }
        if let Some(removed) = stats.removed {
            entry.removed = Some(removed);
        }
    }
    Ok(())
}

/// `None` flags a failed pass so its rows stay uncounted.
fn run_numstat(worktree_path: &str, cached: bool) -> Option<HashMap<String, GitLineStats>> {
    let mut args: Vec<&str> = vec!["-c", "core.quotePath=false", "diff", "-z"];
    if cached {
        args.push("--cached");
    }
    args.extend_from_slice(&["--numstat", "-M"]);
    match run_git_in(worktree_path, &args, NUMSTAT_TIMEOUT, None) {
        Ok(output) if output.status.success() => Some(parse_numstat(&output.stdout)),
        Ok(_) => None,
        Err(_) => None,
    }
}

/// Parses `git diff -z --numstat -M` output. In `-z` form a rename header
/// carries an empty path and is followed by the preimage and postimage as
/// separate NUL fragments; the postimage keys the status row. Shared with
/// branch/commit compare, whose line stats come from the same oracle parser.
pub(crate) fn parse_numstat(stdout: &[u8]) -> HashMap<String, GitLineStats> {
    let records: Vec<&[u8]> = stdout.split(|byte| *byte == 0).collect();
    let mut stats = HashMap::new();
    let mut index = 0;
    while index < records.len() {
        if records[index].is_empty() {
            index += 1;
            continue;
        }
        let parts: Vec<&[u8]> = records[index].split(|byte| *byte == b'\t').collect();
        let added = parse_numstat_count(parts.first().copied());
        let removed = parse_numstat_count(parts.get(1).copied());
        let raw_path = join_bytes(&parts[2.min(parts.len())..], b'\t');
        let path = if raw_path.is_empty() {
            // header, preimage, postimage
            index += 3;
            match records.get(index - 1) {
                Some(postimage) => text(postimage),
                None => continue,
            }
        } else {
            index += 1;
            text(&raw_path)
        };
        if path.is_empty() {
            continue;
        }
        stats.insert(path, GitLineStats { added, removed });
    }
    stats
}

/// `-` means binary; malformed counts are unknown, matching the oracle's
/// `Number.parseInt` guard.
fn parse_numstat_count(field: Option<&[u8]>) -> Option<u64> {
    let field = field?;
    if field == b"-" {
        return None;
    }
    std::str::from_utf8(field).ok()?.parse().ok()
}

/// Untracked files have no Git baseline, so their contents are counted
/// directly; oversized and binary files are left uncounted. A symlink counts
/// as one addition.
fn count_untracked_additions(absolute_path: &Path) -> Option<GitLineStats> {
    let metadata = std::fs::symlink_metadata(absolute_path).ok()?;
    if metadata.file_type().is_symlink() {
        return Some(GitLineStats {
            added: Some(1),
            removed: None,
        });
    }
    if !metadata.is_file() || metadata.len() > MAX_UNTRACKED_LINE_COUNT_BYTES {
        return None;
    }
    let bytes = std::fs::read(absolute_path).ok()?;
    let sniff_len = bytes.len().min(BINARY_SNIFF_BYTES);
    if bytes[..sniff_len].contains(&0) {
        return None;
    }
    if bytes.is_empty() {
        return Some(GitLineStats {
            added: Some(0),
            removed: None,
        });
    }
    let newline_count = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
    // A trailing newline marks the final line complete; without one the last
    // partial line still counts, matching Git's numstat.
    let added = if bytes.last() == Some(&b'\n') {
        newline_count
    } else {
        newline_count + 1
    };
    Some(GitLineStats {
        added: Some(added),
        removed: None,
    })
}

/// `mergeBase → working tree`, deduplicated, so committing does not move it.
/// The oracle also folds untracked additions in and splits test/generated
/// buckets; both are out of scope here (see the Task 3 report).
fn branch_line_total(
    worktree_path: &str,
    merge_base: &str,
    cancel: Option<&CancelToken>,
) -> Result<Option<GitBranchLineTotal>, CoreError> {
    let args = [
        "-c",
        "core.quotePath=false",
        "diff",
        "-z",
        "--numstat",
        "-M",
        merge_base,
        "--",
    ];
    match run_git_in(worktree_path, &args, BRANCH_LINE_TOTAL_TIMEOUT, cancel) {
        Ok(output) if output.status.success() => {
            let stats = parse_numstat(&output.stdout);
            let mut added = 0;
            let mut removed = 0;
            for line_stats in stats.values() {
                // Binary files parse to no counts and contribute nothing,
                // matching the per-file rows.
                added += line_stats.added.unwrap_or(0);
                removed += line_stats.removed.unwrap_or(0);
            }
            Ok(Some(GitBranchLineTotal {
                added,
                removed,
                merge_base: merge_base.to_string(),
                test: None,
                generated: None,
            }))
        }
        Ok(_) => Ok(None),
        Err(error @ CoreError::GitCommandCancelled { .. }) => Err(error),
        Err(_) => Ok(None),
    }
}

/// The merge base reaches the host as untrusted RPC input and is spliced into
/// a git argv. Only an object-name shape is legitimate (`/^[0-9a-f]{7,64}$/`),
/// so anything else — notably a leading `-` — is rejected before it can act as
/// a flag.
fn valid_merge_base(value: &str) -> Option<&str> {
    let valid = (7..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if valid {
        Some(value)
    } else {
        None
    }
}

fn join_bytes(parts: &[&[u8]], separator: u8) -> Vec<u8> {
    let mut joined = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            joined.push(separator);
        }
        joined.extend_from_slice(part);
    }
    joined
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> GitStatusEntry {
        GitStatusEntry {
            path: path.to_string(),
            status: GitFileStatus::Modified,
            area: GitStagingArea::Unstaged,
            old_path: None,
            conflict_kind: None,
            conflict_status: None,
            conflict_status_source: None,
            submodule: None,
            submodule_root: None,
            added: None,
            removed: None,
        }
    }

    #[test]
    fn resolve_status_limit_defaults_negatives_and_zero() {
        assert_eq!(resolve_status_limit(None), DEFAULT_GIT_STATUS_LIMIT);
        assert_eq!(resolve_status_limit(Some(-1)), DEFAULT_GIT_STATUS_LIMIT);
        assert_eq!(
            resolve_status_limit(Some(i64::MIN)),
            DEFAULT_GIT_STATUS_LIMIT
        );
        assert_eq!(resolve_status_limit(Some(0)), 0);
        assert_eq!(resolve_status_limit(Some(7)), 7);
    }

    #[test]
    fn parse_unmerged_entry_maps_every_conflict_kind() {
        let line = |xy: &str| format!("u {xy} N... 100644 100644 100644 100644 a b c conflict.txt");
        let kinds = [
            ("UU", GitConflictKind::BothModified),
            ("AA", GitConflictKind::BothAdded),
            ("DD", GitConflictKind::BothDeleted),
            ("AU", GitConflictKind::AddedByUs),
            ("UA", GitConflictKind::AddedByThem),
            ("DU", GitConflictKind::DeletedByUs),
            ("UD", GitConflictKind::DeletedByThem),
        ];
        for (xy, kind) in kinds {
            let parsed = parse_unmerged_entry("/nonexistent", &line(xy))
                .unwrap_or_else(|| panic!("{xy} should parse"));
            assert_eq!(parsed.conflict_kind, Some(kind));
            assert_eq!(
                parsed.conflict_status,
                Some(GitConflictResolutionStatus::Unresolved)
            );
            assert_eq!(parsed.area, GitStagingArea::Unstaged);
            assert_eq!(parsed.path, "conflict.txt");
        }
    }

    #[test]
    fn parse_unmerged_entry_drops_submodules_unknown_kinds_and_empty_paths() {
        assert!(parse_unmerged_entry(
            "/nonexistent",
            "u UU N... 160000 160000 160000 160000 a b c submodule"
        )
        .is_none());
        assert!(parse_unmerged_entry(
            "/nonexistent",
            "u XY N... 100644 100644 100644 100644 a b c weird.txt"
        )
        .is_none());
        assert!(parse_unmerged_entry(
            "/nonexistent",
            "u UU N... 100644 100644 100644 100644 a b c "
        )
        .is_none());
    }

    #[test]
    fn parse_unmerged_entry_uses_worktree_mode_for_compatibility_status() {
        let deleted = "u UD N... 100644 100644 100644 000000 a b c gone.txt";
        assert_eq!(
            parse_unmerged_entry("/nonexistent", deleted)
                .unwrap()
                .status,
            GitFileStatus::Deleted
        );
        let modified = "u UD N... 100644 100644 100644 100644 a b c here.txt";
        assert_eq!(
            parse_unmerged_entry("/nonexistent", modified)
                .unwrap()
                .status,
            GitFileStatus::Modified
        );
    }

    #[test]
    fn collect_entries_truncates_in_output_order_including_unmerged() {
        let records = vec![
            StatusRecord::Unmerged(
                "u UU N... 100644 100644 100644 100644 a b c conflict.txt".to_string(),
            ),
            StatusRecord::Entry(entry("a.txt")),
            StatusRecord::Entry(entry("b.txt")),
        ];

        let entries = collect_entries("/nonexistent", &parsed_with(records), 1, true);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "conflict.txt");
    }

    fn parsed_with(records: Vec<StatusRecord>) -> ParsedStatus {
        ParsedStatus {
            records,
            ..ParsedStatus::default()
        }
    }

    #[test]
    fn parse_numstat_reads_binary_and_rename_records() {
        let stdout = b"-\t-\tbin.dat\0\
            0\t0\t\0old name.txt\0new name.txt\0\
            3\t2\tsrc/app.rs\0";
        let stats = parse_numstat(stdout);

        assert_eq!(
            stats.get("bin.dat"),
            Some(&GitLineStats {
                added: None,
                removed: None
            })
        );
        assert_eq!(
            stats.get("new name.txt"),
            Some(&GitLineStats {
                added: Some(0),
                removed: Some(0)
            })
        );
        assert_eq!(
            stats.get("src/app.rs"),
            Some(&GitLineStats {
                added: Some(3),
                removed: Some(2)
            })
        );
    }

    #[test]
    fn valid_merge_base_accepts_only_object_names() {
        assert_eq!(valid_merge_base("0123456"), Some("0123456"));
        assert_eq!(
            valid_merge_base("0123456789abcdef0123456789abcdef01234567"),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(valid_merge_base("HEAD"), None);
        assert_eq!(valid_merge_base("--upload-pack=boom"), None);
        assert_eq!(valid_merge_base("ABCDEF0"), None);
        assert_eq!(valid_merge_base("012345"), None);
    }

    #[test]
    fn gitdir_marker_reads_first_line_payload() {
        assert_eq!(
            parse_gitdir_marker("gitdir: /repo/.git/worktrees/wt\n"),
            Some("/repo/.git/worktrees/wt".to_string())
        );
        assert_eq!(
            parse_gitdir_marker("GITDIR: ../repo/.git/worktrees/wt"),
            Some("../repo/.git/worktrees/wt".to_string())
        );
        assert_eq!(parse_gitdir_marker("gitdir:   \n"), None);
        assert_eq!(parse_gitdir_marker("nope: x\n"), None);
    }

    #[test]
    fn gitdir_marker_tolerates_multibyte_first_line() {
        // 第 7 字节落在多字节字符内部时不得 panic（按字节比较而不是切 &str）。
        assert_eq!(parse_gitdir_marker("résumé\n"), None);
        assert_eq!(parse_gitdir_marker("résumé: x\n"), None);
        assert_eq!(parse_gitdir_marker("日本語のテキスト"), None);
        assert_eq!(
            parse_gitdir_marker("GITDIR: /repo/é"),
            Some("/repo/é".to_string())
        );
    }
}
