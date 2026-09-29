//! Commit history, mirroring `orca:src/shared/git-history.ts` and
//! `git-history-log-parser.ts`: one bounded topo-order `git log` query feeds
//! the graph, HEAD/upstream/base refs stay comparison metadata, and the
//! `%(decorate:…)` format falls back to `%D` on Git versions that echo the
//! placeholder.

use ade_core::errors::CoreError;
use serde::Serialize;

use crate::command::{run, run_text};

/// Mirrors `GIT_HISTORY_DEFAULT_LIMIT` (`git-history-types.ts:23`).
pub const GIT_HISTORY_DEFAULT_LIMIT: u32 = 50;
/// Mirrors `GIT_HISTORY_MAX_LIMIT` (`git-history-types.ts:24`).
pub const GIT_HISTORY_MAX_LIMIT: u32 = 200;

/// The `--format` passed to `git log`, copied verbatim from
/// `GIT_HISTORY_COMMIT_FORMAT` (`git-history-log-parser.ts:9-10`).
pub const GIT_HISTORY_COMMIT_FORMAT: &str =
    "%H%n%aN%n%aE%n%at%n%ct%n%P%n%(decorate:prefix=,suffix=,separator=%x1f)%n%D%n%B";

/// Mirrors `GitHistoryGraphColorId` (`git-history-types.ts:1-9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum GitHistoryGraphColorId {
    GitGraphRef,
    GitGraphRemoteRef,
    GitGraphBaseRef,
    GitGraphLane1,
    GitGraphLane2,
    GitGraphLane3,
    GitGraphLane4,
    GitGraphLane5,
}

pub const GIT_HISTORY_REF_COLOR: GitHistoryGraphColorId = GitHistoryGraphColorId::GitGraphRef;
pub const GIT_HISTORY_REMOTE_REF_COLOR: GitHistoryGraphColorId =
    GitHistoryGraphColorId::GitGraphRemoteRef;
pub const GIT_HISTORY_BASE_REF_COLOR: GitHistoryGraphColorId =
    GitHistoryGraphColorId::GitGraphBaseRef;
pub const GIT_HISTORY_LANE_COLORS: [GitHistoryGraphColorId; 5] = [
    GitHistoryGraphColorId::GitGraphLane1,
    GitHistoryGraphColorId::GitGraphLane2,
    GitHistoryGraphColorId::GitGraphLane3,
    GitHistoryGraphColorId::GitGraphLane4,
    GitHistoryGraphColorId::GitGraphLane5,
];

/// Mirrors `GitHistoryRefCategory` (`git-history-types.ts:26`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum GitHistoryRefCategory {
    #[serde(rename = "branches")]
    Branches,
    #[serde(rename = "remote branches")]
    RemoteBranches,
    #[serde(rename = "tags")]
    Tags,
    #[serde(rename = "commits")]
    Commits,
}

/// Mirrors `GitHistoryItemRef` (`git-history-types.ts:28-35`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryItemRef {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<GitHistoryRefCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<GitHistoryGraphColorId>,
}

/// Mirrors `GitHistoryItemStatistics` (`git-history-types.ts:37-41`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryItemStatistics {
    pub files: u64,
    pub insertions: u64,
    pub deletions: u64,
}

/// Mirrors `GitHistoryItem` (`git-history-types.ts:43-55`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryItem {
    pub id: String,
    pub parent_ids: Vec<String>,
    pub subject: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statistics: Option<GitHistoryItemStatistics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<GitHistoryItemRef>>,
}

/// Mirrors `GitHistoryResult` (`git-history-types.ts:62-72`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryResult {
    pub items: Vec<GitHistoryItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_ref: Option<GitHistoryItemRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_ref: Option<GitHistoryItemRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<GitHistoryItemRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_base: Option<String>,
    pub has_incoming_changes: bool,
    pub has_outgoing_changes: bool,
    pub has_more: bool,
    pub limit: u32,
}

/// Mirrors `loadGitHistoryFromExecutor` (`git-history.ts:167-239`).
pub fn history(
    worktree_path: &str,
    limit: Option<u32>,
    base_ref: Option<&str>,
) -> Result<GitHistoryResult, CoreError> {
    let limit = clamp_history_limit(limit);
    let empty = |limit: u32| GitHistoryResult {
        items: Vec::new(),
        current_ref: None,
        remote_ref: None,
        base_ref: None,
        merge_base: None,
        has_incoming_changes: false,
        has_outgoing_changes: false,
        has_more: false,
        limit,
    };

    let head_oid = match resolve_commit(worktree_path, "HEAD") {
        Some(oid) => oid,
        None => return Ok(empty(limit)),
    };

    let (current_ref, branch_name) = resolve_current_ref(worktree_path, &head_oid);
    let remote_ref = branch_name
        .as_deref()
        .and_then(|name| resolve_upstream_ref(worktree_path, name));
    let raw_base_ref = resolve_named_ref(worktree_path, base_ref);
    let base_ref = match raw_base_ref {
        Some(candidate)
            if Some(candidate.id.as_str()) != remote_ref.as_ref().map(|entry| entry.id.as_str())
                && candidate.id != current_ref.id =>
        {
            Some(candidate)
        }
        _ => None,
    };

    let mut merge_base: Option<String> = None;
    if let (Some(remote_revision), Some(current_revision)) = (
        remote_ref
            .as_ref()
            .and_then(|entry| entry.revision.as_deref()),
        current_ref.revision.as_deref(),
    ) {
        if remote_revision != current_revision {
            if let Ok(value) = run_text(
                worktree_path,
                &["merge-base", current_revision, remote_revision],
            ) {
                if !value.is_empty() {
                    merge_base = Some(value);
                }
            }
        }
    }

    // Why: this panel is scoped to the active workspace; upstream and base refs
    // stay comparison metadata, so only HEAD feeds the log range.
    let stdout = run(
        worktree_path,
        &[
            "log",
            &format!("--format={GIT_HISTORY_COMMIT_FORMAT}"),
            "-z",
            "--topo-order",
            "--decorate=full",
            &format!("-n{}", limit + 1),
            &head_oid,
        ],
    )?;
    let parsed = parse_git_history_log(&String::from_utf8_lossy(&stdout));
    let has_more = parsed.len() > limit as usize;
    let items: Vec<GitHistoryItem> = parsed.into_iter().take(limit as usize).collect();

    let remote_revision = remote_ref
        .as_ref()
        .and_then(|entry| entry.revision.as_deref());
    let has_incoming_changes = remote_revision.is_some()
        && merge_base.is_some()
        && remote_revision != merge_base.as_deref();
    let has_outgoing_changes = current_ref.revision.is_some()
        && remote_revision.is_some()
        && merge_base.is_some()
        && current_ref.revision.as_deref() != merge_base.as_deref();

    Ok(GitHistoryResult {
        items,
        current_ref: Some(current_ref),
        remote_ref,
        base_ref,
        merge_base,
        has_incoming_changes,
        has_outgoing_changes,
        has_more,
        limit,
    })
}

const GIT_HISTORY_DECORATION_SEPARATOR: char = '\u{1f}';
const GIT_HISTORY_LEGACY_DECORATION_SEPARATOR: char = ',';

fn clamp_history_limit(limit: Option<u32>) -> u32 {
    match limit {
        None => GIT_HISTORY_DEFAULT_LIMIT,
        Some(value) => value.clamp(1, GIT_HISTORY_MAX_LIMIT),
    }
}

/// `resolveCommit` (`git-history.ts:46`): peels to a commit, refusing empty or
/// option-looking refs.
fn resolve_commit(worktree_path: &str, reference: &str) -> Option<String> {
    if reference.is_empty() || reference.starts_with('-') {
        return None;
    }
    let oid = run_text(
        worktree_path,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )
    .ok()?;
    if oid.is_empty() {
        None
    } else {
        Some(oid)
    }
}

/// `resolveSymbolicFullName` (`git-history.ts:66`): `--symbolic-full-name`
/// echoes the `--end-of-options` marker, so the first other line is the ref.
fn resolve_symbolic_full_name(worktree_path: &str, reference: &str) -> Option<String> {
    if reference.is_empty() || reference.starts_with('-') {
        return None;
    }
    let stdout = run_text(
        worktree_path,
        &[
            "rev-parse",
            "--symbolic-full-name",
            "--end-of-options",
            reference,
        ],
    )
    .ok()?;
    stdout
        .lines()
        .find(|line| !line.is_empty() && *line != "--end-of-options")
        .map(|line| line.to_string())
}

/// `resolveCurrentRef` (`git-history.ts:94`): the checked-out branch, or a
/// synthetic commit ref for a detached HEAD.
fn resolve_current_ref(worktree_path: &str, head_oid: &str) -> (GitHistoryItemRef, Option<String>) {
    if let Ok(branch) = run_text(worktree_path, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        if !branch.is_empty() {
            return (
                GitHistoryItemRef {
                    id: format!("refs/heads/{branch}"),
                    name: branch.clone(),
                    revision: Some(head_oid.to_string()),
                    category: Some(GitHistoryRefCategory::Branches),
                    description: None,
                    color: None,
                },
                Some(branch),
            );
        }
    }
    (
        GitHistoryItemRef {
            id: head_oid.to_string(),
            name: short_git_hash(head_oid),
            revision: Some(head_oid.to_string()),
            category: Some(GitHistoryRefCategory::Commits),
            description: None,
            color: None,
        },
        None,
    )
}

/// `resolveUpstreamRef` (`git-history.ts:123`): the branch's configured
/// upstream, resolved through `for-each-ref` so a missing upstream is a
/// non-event.
fn resolve_upstream_ref(worktree_path: &str, branch_name: &str) -> Option<GitHistoryItemRef> {
    let stdout = run_text(
        worktree_path,
        &[
            "for-each-ref",
            "--format=%(upstream)%00%(upstream:short)",
            &format!("refs/heads/{branch_name}"),
        ],
    )
    .ok()?;
    let mut parts = stdout.split('\0');
    let full_name = parts.next().unwrap_or("").trim();
    let short_name = parts.next().unwrap_or("").trim();
    if full_name.is_empty() || short_name.is_empty() {
        return None;
    }
    let oid = resolve_commit(worktree_path, full_name)?;
    Some(git_history_ref_from_full_name(
        Some(full_name),
        short_name,
        &oid,
    ))
}

/// `resolveNamedRef` (`git-history.ts:151`): resolves an explicit base ref's
/// commit and symbolic name; a ref equal to the current/remote ref is dropped
/// by the caller.
fn resolve_named_ref(worktree_path: &str, base_ref: Option<&str>) -> Option<GitHistoryItemRef> {
    let normalized = base_ref?.trim();
    if normalized.is_empty() || normalized.starts_with('-') {
        return None;
    }
    let revision = resolve_commit(worktree_path, normalized)?;
    let full_name = resolve_symbolic_full_name(worktree_path, normalized);
    Some(git_history_ref_from_full_name(
        full_name.as_deref(),
        normalized,
        &revision,
    ))
}

/// `gitHistoryRefFromFullName` (`git-history-log-parser.ts:160`).
fn git_history_ref_from_full_name(
    full_name: Option<&str>,
    fallback_name: &str,
    revision: &str,
) -> GitHistoryItemRef {
    let id = full_name.filter(|value| !value.is_empty()).unwrap_or(fallback_name);
    if let Some(branch) = id.strip_prefix("refs/heads/") {
        return GitHistoryItemRef {
            id: id.to_string(),
            name: branch.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::Branches),
            description: None,
            color: None,
        };
    }
    if let Some(remote_branch) = id.strip_prefix("refs/remotes/") {
        return GitHistoryItemRef {
            id: id.to_string(),
            name: remote_branch.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::RemoteBranches),
            description: None,
            color: None,
        };
    }
    if let Some(tag) = id.strip_prefix("refs/tags/") {
        return GitHistoryItemRef {
            id: id.to_string(),
            name: tag.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::Tags),
            description: None,
            color: None,
        };
    }
    GitHistoryItemRef {
        id: id.to_string(),
        name: if fallback_name.is_empty() {
            short_git_hash(revision)
        } else {
            fallback_name.to_string()
        },
        revision: Some(revision.to_string()),
        category: Some(GitHistoryRefCategory::Commits),
        description: None,
        color: None,
    }
}

/// `parseGitHistoryLog` (`git-history-log-parser.ts:107`): one NUL-delimited
/// record per commit, fields newline-delimited, `%B` last.
fn parse_git_history_log(stdout: &str) -> Vec<GitHistoryItem> {
    let unexpanded_placeholder = format!(
        "%(decorate:prefix=,suffix=,separator={})",
        GIT_HISTORY_DECORATION_SEPARATOR
    );
    let mut items = Vec::new();

    for raw_record in stdout.split('\0') {
        let record = raw_record.trim_start_matches('\n');
        if record.trim().is_empty() {
            continue;
        }

        let mut lines: Vec<&str> = Vec::with_capacity(8);
        let mut message_start = 0usize;
        for _ in 0..8 {
            match record[message_start..].find('\n') {
                Some(offset) => {
                    let end = message_start + offset;
                    lines.push(&record[message_start..end]);
                    message_start = end + 1;
                }
                None => {
                    lines.push(&record[message_start..]);
                    message_start = record.len();
                    break;
                }
            }
        }

        let hash = lines.first().map(|line| line.trim()).unwrap_or("").to_string();
        if !is_hex_oid(&hash) {
            continue;
        }

        let author_name = lines.get(1).copied().unwrap_or("");
        let author_email = lines.get(2).copied().unwrap_or("");
        let author_date_seconds = lines
            .get(3)
            .copied()
            .unwrap_or("")
            .trim()
            .parse::<i64>()
            .ok();
        let parents = lines.get(5).copied().unwrap_or("").trim();
        let decorate_field = lines.get(6).copied().unwrap_or("");
        let is_legacy_git = decorate_field == unexpanded_placeholder;
        let decorations = if is_legacy_git {
            lines.get(7).copied().unwrap_or("")
        } else {
            decorate_field
        };
        let message = record[message_start..]
            .strip_suffix('\n')
            .unwrap_or(&record[message_start..])
            .to_string();

        items.push(GitHistoryItem {
            parent_ids: if parents.is_empty() {
                Vec::new()
            } else {
                parents.split(' ').map(|value| value.to_string()).collect()
            },
            subject: commit_subject(&message),
            message,
            author: (!author_name.is_empty()).then(|| author_name.to_string()),
            author_email: (!author_email.is_empty()).then(|| author_email.to_string()),
            display_id: Some(short_git_hash(&hash)),
            timestamp: author_date_seconds.map(|seconds| seconds * 1000),
            statistics: None,
            references: Some(parse_decoration_refs(
                decorations,
                &hash,
                if is_legacy_git {
                    GIT_HISTORY_LEGACY_DECORATION_SEPARATOR
                } else {
                    GIT_HISTORY_DECORATION_SEPARATOR
                },
            )),
            id: hash,
        });
    }
    items
}

/// `commitSubject` (`git-history-log-parser.ts:19`).
fn commit_subject(message: &str) -> String {
    let first_line = message
        .lines()
        .next()
        .map(|line| line.trim())
        .unwrap_or("");
    if first_line.is_empty() {
        "(no commit message)".to_string()
    } else {
        first_line.to_string()
    }
}

/// `parseGitDecorationRefs` (`git-history-log-parser.ts:24`).
fn parse_decoration_refs(
    raw: &str,
    revision: &str,
    separator: char,
) -> Vec<GitHistoryItemRef> {
    if raw.trim().is_empty() {
        return Vec::new();
    }

    let mut refs = Vec::new();
    for part in raw.split(separator) {
        let value = part.trim();
        if value.is_empty() || value == "HEAD" {
            continue;
        }
        if let Some(rest) = value.strip_prefix("refs/remotes/") {
            if let Some((_, tail)) = rest.split_once('/') {
                if tail == "HEAD" {
                    continue;
                }
            }
        }

        if let Some(id) = value.strip_prefix("HEAD -> ") {
            if let Some(branch) = id.strip_prefix("refs/heads/") {
                refs.push(GitHistoryItemRef {
                    id: id.to_string(),
                    name: branch.to_string(),
                    revision: Some(revision.to_string()),
                    category: Some(GitHistoryRefCategory::Branches),
                    description: None,
                    color: None,
                });
            }
            continue;
        }
        if let Some(branch) = value.strip_prefix("refs/heads/") {
            refs.push(GitHistoryItemRef {
                id: value.to_string(),
                name: branch.to_string(),
                revision: Some(revision.to_string()),
                category: Some(GitHistoryRefCategory::Branches),
                description: None,
                color: None,
            });
            continue;
        }
        if let Some(remote_branch) = value.strip_prefix("refs/remotes/") {
            refs.push(GitHistoryItemRef {
                id: value.to_string(),
                name: remote_branch.to_string(),
                revision: Some(revision.to_string()),
                category: Some(GitHistoryRefCategory::RemoteBranches),
                description: None,
                color: None,
            });
            continue;
        }
        if let Some(tag) = value.strip_prefix("tag: refs/tags/") {
            refs.push(GitHistoryItemRef {
                id: format!("refs/tags/{tag}"),
                name: tag.to_string(),
                revision: Some(revision.to_string()),
                category: Some(GitHistoryRefCategory::Tags),
                description: None,
                color: None,
            });
        }
    }

    refs.sort_by(|left, right| {
        ref_category_order(left)
            .cmp(&ref_category_order(right))
            .then_with(|| left.name.cmp(&right.name))
    });
    refs
}

fn ref_category_order(reference: &GitHistoryItemRef) -> u8 {
    if reference.id.starts_with("refs/heads/") {
        1
    } else if reference.id.starts_with("refs/remotes/") {
        2
    } else if reference.id.starts_with("refs/tags/") {
        3
    } else {
        99
    }
}

fn short_git_hash(hash: &str) -> String {
    hash.chars().take(7).collect()
}

fn is_hex_oid(value: &str) -> bool {
    (40..=64).contains(&value.len()) && value.chars().all(|char| char.is_ascii_hexdigit())
}
