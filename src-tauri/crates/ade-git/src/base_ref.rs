//! Base-ref UI queries: the short default base ref, the configured remote
//! count, and the bounded branch search behind the base-ref picker.
//!
//! Mirrors `orca:src/main/git/repo-default-base-ref.ts`
//! (`resolveDefaultBaseRefViaExec`, short names), `orca:src/main/git/repo.ts`
//! (`parseRemoteCount`/`getRemoteCount`/`listRemoteNames`) and
//! `orca:src/main/git/repo-base-ref-search.ts` (argv building, parse/filter,
//! dedup, limit clamps and query normalization).

use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;

use crate::runner::run_git_in;

/// Mirrors `DEFAULT_BASE_REF_PROBE_TIMEOUT_MS`
/// (`orca:src/main/git/repo-default-base-ref.ts:12`).
const BASE_REF_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The picker asks for 20; the API default stays at the legacy 25
/// (`REPO_SEARCH_REFS_DEFAULT_LIMIT`).
pub const SEARCH_REFS_DEFAULT_LIMIT: u32 = 25;
/// `REPO_SEARCH_REFS_MAX_LIMIT`.
pub const SEARCH_REFS_MAX_LIMIT: u32 = 1_000;
/// `REPO_SEARCH_REFS_MAX_SCAN_LIMIT`.
pub const SEARCH_REFS_MAX_SCAN_LIMIT: u32 = SEARCH_REFS_MAX_LIMIT + 1;
const REF_SEARCH_CANDIDATE_MULTIPLIER: u32 = 4;
const REF_SEARCH_LEGACY_HEADROOM: u32 = 100;

/// One entry of `repos:searchBaseRefDetails` (oracle `BaseRefSearchResult`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseRefSearchResult {
    pub ref_name: String,
    pub local_branch_name: String,
}

/// Probe order and returned short names (`DEFAULT_BASE_REF_PROBES`): a
/// remote-tracking `origin/main`/`origin/master` outranks the local
/// `main`/`master`.
const DEFAULT_BASE_REF_PROBES: [(&str, &str); 4] = [
    ("refs/remotes/origin/main", "origin/main"),
    ("refs/remotes/origin/master", "origin/master"),
    ("refs/heads/main", "main"),
    ("refs/heads/master", "master"),
];

/// Resolve the repository's default base ref as the picker spells it.
///
/// Mirrors `resolveDefaultBaseRefViaExec`
/// (`orca:src/main/git/repo-default-base-ref.ts:109-115`): a verified symbolic
/// `refs/remotes/origin/HEAD` wins, then the probe order, and an unresolvable
/// repo answers `None` instead of inventing a branch. Unlike
/// [`crate::branch::resolve_default_base_ref`] this returns short names
/// (`main`, `origin/main`) and never errors — the UI contract is
/// `{defaultBaseRef: string | null}`.
pub fn resolve_default_base_ref_short(repo_path: &str) -> Option<String> {
    if let Some(origin_head) = verified_origin_head_base_ref(repo_path) {
        return Some(origin_head);
    }
    for (reference, short_name) in DEFAULT_BASE_REF_PROBES {
        if ref_resolves(repo_path, reference) {
            return Some(short_name.to_string());
        }
    }
    None
}

/// Count configured remotes; zero means either none or unavailable
/// (oracle `getRemoteCount`).
pub fn remote_count(repo_path: &str) -> u32 {
    match run_git(repo_path, &["remote"]) {
        Ok(stdout) => parse_remote_count(&stdout),
        Err(error) => {
            eprintln!("[ade-git] git remote failed for '{repo_path}': {error}");
            0
        }
    }
}

/// Parse `git remote` stdout into a remote count (oracle `parseRemoteCount`).
pub fn parse_remote_count(stdout: &str) -> u32 {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u32
}

/// Search base refs, returning short names only (`repos:searchBaseRefs`).
///
/// Mirrors `searchBaseRefs` (`orca:src/main/git/repo-base-ref-search.ts:164-174`):
/// an invalid (zero) limit answers `[]`, a large one is clamped, and a failed
/// `for-each-ref` is logged and treated as no results.
pub fn search_base_refs(repo_path: &str, query: &str, limit: u32) -> Vec<String> {
    search_base_ref_details(repo_path, query, limit)
        .into_iter()
        .map(|entry| entry.ref_name)
        .collect()
}

/// Search base refs with their local branch names
/// (`repos:searchBaseRefDetails`).
///
/// Mirrors `searchBaseRefDetails`
/// (`orca:src/main/git/repo-base-ref-search.ts:176-217`): multi-token queries
/// run the segmented and branch-root pattern groups and merge them
/// round-robin; the scan is bounded by the oracle's candidate multiplier and
/// the parse pipeline dedupes and slices to the bounded limit.
pub fn search_base_ref_details(
    repo_path: &str,
    query: &str,
    limit: u32,
) -> Vec<BaseRefSearchResult> {
    if limit == 0 {
        // `isRepoSearchRefsRequestLimit`: a positive safe integer is required.
        return Vec::new();
    }
    let bounded_limit = limit.min(SEARCH_REFS_MAX_LIMIT);
    let normalized_query = normalize_ref_search_query(query);
    let remotes = list_remote_names(repo_path);
    let tokens: Vec<&str> = normalized_query
        .split('/')
        .filter(|token| !token.is_empty())
        .collect();

    if tokens.len() > 1 {
        let runs = [
            run_search_base_refs(
                repo_path,
                &normalized_query,
                bounded_limit,
                &remotes,
                PatternGroup::Segmented,
            ),
            run_search_base_refs(
                repo_path,
                &normalized_query,
                bounded_limit,
                &remotes,
                PatternGroup::BranchRoot,
            ),
        ];
        let mut groups = Vec::with_capacity(2);
        for run in runs {
            match run {
                Ok(stdout) => groups.push(parse_and_filter_search_ref_details(
                    &stdout,
                    bounded_limit,
                    &remotes,
                )),
                Err(error) => {
                    eprintln!(
                        "[ade-git] searchBaseRefs for-each-ref failed for '{repo_path}': {error}"
                    );
                    return Vec::new();
                }
            }
        }
        return merge_base_ref_search_result_groups(&groups, bounded_limit);
    }

    match run_search_base_refs(
        repo_path,
        &normalized_query,
        bounded_limit,
        &remotes,
        PatternGroup::All,
    ) {
        Ok(stdout) => parse_and_filter_search_ref_details(&stdout, bounded_limit, &remotes),
        Err(error) => {
            eprintln!("[ade-git] searchBaseRefs for-each-ref failed for '{repo_path}': {error}");
            Vec::new()
        }
    }
}

/// Strip glob metacharacters so the query can never inject a pattern
/// (oracle `normalizeRefSearchQuery`).
pub fn normalize_ref_search_query(query: &str) -> String {
    query
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, '*' | '?' | '[' | ']' | '\\'))
        .collect()
}

/// List configured remote names; a failed `git remote` answers `[]`
/// (oracle `listRemoteNames`).
pub fn list_remote_names(repo_path: &str) -> Vec<String> {
    match run_git(repo_path, &["remote"]) {
        Ok(stdout) => stdout
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatternGroup {
    All,
    Segmented,
    BranchRoot,
}

fn verified_origin_head_base_ref(repo_path: &str) -> Option<String> {
    let output = run_git_in(
        repo_path,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
        BASE_REF_PROBE_TIMEOUT,
        None,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let reference = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if reference.is_empty() || !ref_resolves(repo_path, &reference) {
        return None;
    }
    Some(
        reference
            .strip_prefix("refs/remotes/")
            .unwrap_or(&reference)
            .to_string(),
    )
}

fn ref_resolves(repo_path: &str, reference: &str) -> bool {
    matches!(
        run_git_in(
            repo_path,
            &["rev-parse", "--verify", "--quiet", reference],
            BASE_REF_PROBE_TIMEOUT,
            None
        ),
        Ok(output) if output.status.success()
    )
}

fn run_git(repo_path: &str, args: &[&str]) -> Result<String, String> {
    match run_git_in(repo_path, args, REMOTE_TIMEOUT, None) {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Err(error.to_string()),
    }
}

fn run_search_base_refs(
    repo_path: &str,
    normalized_query: &str,
    limit: u32,
    remotes: &[String],
    pattern_group: PatternGroup,
) -> Result<String, String> {
    let argv = build_search_base_refs_argv(normalized_query, limit, remotes, pattern_group);
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    match run_git_in(repo_path, &args, SEARCH_TIMEOUT, None) {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Err(error.to_string()),
    }
}

/// Build the bounded `for-each-ref` argv shared by both search entry points
/// (oracle `buildSearchBaseRefsArgv`).
fn build_search_base_refs_argv(
    normalized_query: &str,
    limit: u32,
    remotes: &[String],
    pattern_group: PatternGroup,
) -> Vec<String> {
    let bounded_scan_limit = limit.min(SEARCH_REFS_MAX_SCAN_LIMIT);
    let candidate_count = get_ref_search_candidate_count(bounded_scan_limit, true);
    let mut argv = vec![
        "for-each-ref".to_string(),
        "--format=%(refname)%00%(refname:short)".to_string(),
        "--sort=-committerdate".to_string(),
    ];
    argv.extend(get_remote_head_excludes(remotes));
    argv.push(format!("--count={candidate_count}"));

    let tokens: Vec<&str> = normalized_query
        .split('/')
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.len() <= 1 {
        let query = tokens.first().copied().unwrap_or("");
        argv.extend([
            format!("refs/heads/**/*{query}*"),
            format!("refs/heads/**/*{query}*/**"),
            format!("refs/remotes/**/*{query}*"),
            format!("refs/remotes/**/*{query}*/**"),
        ]);
        return argv;
    }

    let segmented = tokens
        .iter()
        .map(|token| format!("*{token}*"))
        .collect::<Vec<_>>()
        .join("/");
    let substring_query = tokens.join("/");
    let remote_branch_root_patterns: Vec<String> = if remotes.is_empty() {
        vec![
            format!("refs/remotes/*/{substring_query}*"),
            format!("refs/remotes/*/{substring_query}*/**"),
        ]
    } else {
        remotes
            .iter()
            .flat_map(|remote| {
                [
                    format!("refs/remotes/{remote}/{substring_query}*"),
                    format!("refs/remotes/{remote}/{substring_query}*/**"),
                ]
            })
            .collect()
    };
    let segmented_patterns = vec![
        format!("refs/remotes/{segmented}"),
        format!("refs/heads/{segmented}"),
    ];
    let mut branch_root_patterns = vec![
        format!("refs/heads/{substring_query}*"),
        format!("refs/heads/{substring_query}*/**"),
    ];
    branch_root_patterns.extend(remote_branch_root_patterns);

    match pattern_group {
        PatternGroup::Segmented => argv.extend(segmented_patterns),
        PatternGroup::BranchRoot => argv.extend(branch_root_patterns),
        PatternGroup::All => {
            argv.extend(segmented_patterns);
            argv.extend(branch_root_patterns);
        }
    }
    argv
}

/// Keep Git's `--count` finite (`getRefSearchCandidateCount`); the scan limit
/// is validated by the caller, so the multiplier cannot overflow. The legacy
/// headroom covers the no-`--exclude` fallback path, which this port never
/// takes because every modern git supports `--exclude`.
fn get_ref_search_candidate_count(limit: u32, excludes_remote_head: bool) -> u32 {
    let base_count = limit * REF_SEARCH_CANDIDATE_MULTIPLIER;
    if excludes_remote_head {
        base_count
    } else {
        base_count + REF_SEARCH_LEGACY_HEADROOM
    }
}

/// Build excludes for the symbolic `<remote>/HEAD` slot without hiding nested
/// branch names such as `<remote>/feature/HEAD` (oracle
/// `getRemoteHeadExcludes`).
fn get_remote_head_excludes(remotes: &[String]) -> Vec<String> {
    if remotes.is_empty() {
        return vec!["--exclude=refs/remotes/*/HEAD".to_string()];
    }
    let mut slash_remotes: Vec<&String> = remotes
        .iter()
        .filter(|remote| {
            remote.contains('/') && is_safe_git_ref_name(&format!("refs/remotes/{remote}/HEAD"))
        })
        .collect();
    slash_remotes.sort();
    slash_remotes.dedup();
    let mut excludes = vec!["--exclude=refs/remotes/*/HEAD".to_string()];
    excludes.extend(
        slash_remotes
            .into_iter()
            .map(|remote| format!("--exclude=refs/remotes/{remote}/HEAD")),
    );
    excludes
}

/// Mirrors `isSafeGitRefName` (`orca:src/shared/git-status-upstream-ref.ts:16-36`).
fn is_safe_git_ref_name(reference: &str) -> bool {
    if !reference.starts_with("refs/") || reference.ends_with('/') {
        return false;
    }
    if reference.contains("..") || reference.contains("@{") {
        return false;
    }
    for ch in reference.chars() {
        let code = ch as u32;
        if code <= 0x20 || code == 0x7f || "~^:?*[\\".contains(ch) {
            return false;
        }
    }
    let parts: Vec<&str> = reference.split('/').collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && !part.starts_with('.')
                && !part.ends_with('.')
                && !part.ends_with(".lock")
        })
}

/// Parse, filter, dedupe and slice `for-each-ref` output
/// (oracle `parseAndFilterSearchRefDetails`).
fn parse_and_filter_search_ref_details(
    stdout: &str,
    limit: u32,
    remotes: &[String],
) -> Vec<BaseRefSearchResult> {
    let mut sorted_remotes: Vec<&str> = remotes.iter().map(String::as_str).collect();
    sorted_remotes.sort_by_key(|remote| std::cmp::Reverse(remote.len()));

    let mut seen = HashSet::new();
    let mut results = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(nul) = line.find('\0') else {
            continue;
        };
        let full_ref = &line[..nul];
        let git_short_ref = &line[nul + 1..];
        if is_remote_head_ref(full_ref, &sorted_remotes) {
            continue;
        }
        let short_ref = canonical_short_ref(full_ref, git_short_ref);
        if !seen.insert(short_ref.clone()) {
            continue;
        }
        results.push(BaseRefSearchResult {
            local_branch_name: resolve_local_branch_name(full_ref, &short_ref, &sorted_remotes),
            ref_name: short_ref,
        });
        if results.len() >= limit as usize {
            break;
        }
    }
    results
}

/// Git's `refname:short` DWIM strips a trailing `/HEAD`
/// (`refs/remotes/origin/feature/HEAD` -> `origin/feature`); derive the display
/// name for that case only (oracle `canonicalShortRef`).
fn canonical_short_ref(full_ref: &str, git_short_ref: &str) -> String {
    if full_ref.starts_with("refs/remotes/")
        && full_ref.ends_with("/HEAD")
        && !git_short_ref.ends_with("/HEAD")
    {
        return full_ref["refs/remotes/".len()..].to_string();
    }
    git_short_ref.to_string()
}

/// Exclude only a remote's direct symbolic HEAD, preserving branches like
/// `feature/HEAD` (oracle `isRemoteHeadRef`).
fn is_remote_head_ref(full_ref: &str, longest_first_remotes: &[&str]) -> bool {
    let short_ref = full_ref.strip_prefix("refs/remotes/").unwrap_or(full_ref);
    if let Some(remote) = longest_first_remotes
        .iter()
        .find(|remote| short_ref.starts_with(&format!("{remote}/")))
    {
        return &short_ref[remote.len() + 1..] == "HEAD";
    }
    // A stale ref whose remote is no longer configured is unambiguous only in
    // the conventional two-component `<remote>/HEAD` shape.
    short_ref.split('/').count() == 2 && short_ref.ends_with("/HEAD")
}

/// Oracle `resolveLocalBranchName`: the branch part under the longest matching
/// remote, else the short ref for local branches.
fn resolve_local_branch_name(
    full_ref: &str,
    short_ref: &str,
    longest_first_remotes: &[&str],
) -> String {
    let Some(remote_and_branch) = full_ref.strip_prefix("refs/remotes/") else {
        return short_ref.to_string();
    };
    if let Some(remote) = longest_first_remotes
        .iter()
        .find(|remote| remote_and_branch.starts_with(&format!("{remote}/")))
    {
        let branch = &remote_and_branch[remote.len() + 1..];
        if !branch.is_empty() {
            return branch.to_string();
        }
    }
    let branch = remote_and_branch
        .split('/')
        .skip(1)
        .collect::<Vec<_>>()
        .join("/");
    if branch.is_empty() {
        short_ref.to_string()
    } else {
        branch
    }
}

/// Interleave pattern-group results round-robin, dedupe by `refName`, and stop
/// at `limit` (oracle `mergeBaseRefSearchResultGroups`).
fn merge_base_ref_search_result_groups(
    groups: &[Vec<BaseRefSearchResult>],
    limit: u32,
) -> Vec<BaseRefSearchResult> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    let max_length = groups.iter().map(Vec::len).max().unwrap_or(0);
    for index in 0..max_length {
        for group in groups {
            let Some(entry) = group.get(index) else {
                continue;
            };
            if !seen.insert(entry.ref_name.clone()) {
                continue;
            }
            merged.push(entry.clone());
            if merged.len() >= limit as usize {
                return merged;
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_query_strips_glob_metacharacters_and_whitespace() {
        assert_eq!(normalize_ref_search_query("  origin/main  "), "origin/main");
        assert_eq!(normalize_ref_search_query("feat*ur[e]\\x?"), "featurex");
    }

    #[test]
    fn parse_remote_count_ignores_blank_lines() {
        assert_eq!(parse_remote_count("origin\nupstream\n"), 2);
        assert_eq!(parse_remote_count(""), 0);
        assert_eq!(parse_remote_count("\n  \n"), 0);
    }

    #[test]
    fn parse_and_filter_dedupes_and_excludes_the_remote_head() {
        let stdout = "refs/remotes/origin/HEAD\0origin\nrefs/remotes/origin/main\0origin/main\nrefs/heads/main\0main\nrefs/heads/main\0main\n";
        assert_eq!(
            parse_and_filter_search_ref_details(stdout, 25, &[]),
            vec![
                BaseRefSearchResult {
                    ref_name: "origin/main".to_string(),
                    local_branch_name: "main".to_string(),
                },
                BaseRefSearchResult {
                    ref_name: "main".to_string(),
                    local_branch_name: "main".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_keeps_nested_head_branches_and_canonicalizes_their_short_name() {
        let stdout = "refs/remotes/origin/feature/HEAD\0origin/feature\n";
        assert_eq!(
            parse_and_filter_search_ref_details(stdout, 25, &[]),
            vec![BaseRefSearchResult {
                ref_name: "origin/feature/HEAD".to_string(),
                local_branch_name: "feature/HEAD".to_string(),
            }]
        );
    }

    #[test]
    fn merge_interleaves_groups_and_honors_the_limit() {
        let group_a = vec![
            BaseRefSearchResult {
                ref_name: "a1".to_string(),
                local_branch_name: "a1".to_string(),
            },
            BaseRefSearchResult {
                ref_name: "a2".to_string(),
                local_branch_name: "a2".to_string(),
            },
        ];
        let group_b = vec![
            BaseRefSearchResult {
                ref_name: "b1".to_string(),
                local_branch_name: "b1".to_string(),
            },
            BaseRefSearchResult {
                ref_name: "a1".to_string(),
                local_branch_name: "a1".to_string(),
            },
        ];
        assert_eq!(
            merge_base_ref_search_result_groups(&[group_a, group_b], 3)
                .iter()
                .map(|entry| entry.ref_name.as_str())
                .collect::<Vec<_>>(),
            vec!["a1", "b1", "a2"]
        );
    }

    #[test]
    fn search_limit_zero_is_an_invalid_request() {
        assert!(search_base_ref_details("/nonexistent", "", 0).is_empty());
    }

    #[test]
    fn candidate_count_multiplier_matches_oracle() {
        assert_eq!(get_ref_search_candidate_count(25, true), 100);
        assert_eq!(get_ref_search_candidate_count(25, false), 200);
    }

    #[test]
    fn safe_git_ref_name_matches_oracle_rules() {
        assert!(is_safe_git_ref_name("refs/remotes/origin/HEAD"));
        assert!(!is_safe_git_ref_name("refs/remotes/origin//HEAD"));
        assert!(!is_safe_git_ref_name("refs/remotes/origin/feature..x/HEAD"));
        assert!(!is_safe_git_ref_name("refs/remotes/origin/HEAD.lock"));
        assert!(!is_safe_git_ref_name("refs/remotes/origin/feature x/HEAD"));
    }
}
