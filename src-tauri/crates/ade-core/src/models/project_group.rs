use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Fallback used when a create/update name normalizes to blank (oracle
/// `normalizeProjectGroupName`).
pub const DEFAULT_PROJECT_GROUP_NAME: &str = "Untitled group";

/// `ProjectGroup.createdFrom` (oracle `ProjectGroupCreatedFrom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ProjectGroupCreatedFrom {
    Manual,
    FolderScan,
    Migration,
}

impl ProjectGroupCreatedFrom {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::FolderScan => "folder-scan",
            Self::Migration => "migration",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "folder-scan" => Some(Self::FolderScan),
            "migration" => Some(Self::Migration),
            _ => None,
        }
    }
}

/// `trim(name) || fallback` (oracle `normalizeProjectGroupName`).
pub fn normalize_project_group_name(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build a new registry group row (spec §5.2): `id`, `name`, `parentPath`,
/// `connectionId`, `parentGroupId`, `createdFrom`, `tabOrder`,
/// `isCollapsed=false`, `color=null`, timestamps.
#[allow(clippy::too_many_arguments)]
pub fn new_project_group(
    id: &str,
    name: &str,
    parent_path: Option<&str>,
    connection_id: Option<&str>,
    parent_group_id: Option<&str>,
    created_from: ProjectGroupCreatedFrom,
    tab_order: u64,
    now: u64,
) -> Value {
    let mut group = Map::new();
    group.insert("id".to_string(), Value::String(id.to_string()));
    group.insert(
        "name".to_string(),
        Value::String(normalize_project_group_name(
            name,
            DEFAULT_PROJECT_GROUP_NAME,
        )),
    );
    group.insert("parentPath".to_string(), nullable_string(parent_path));
    group.insert("connectionId".to_string(), nullable_string(connection_id));
    group.insert(
        "parentGroupId".to_string(),
        nullable_string(parent_group_id),
    );
    group.insert(
        "createdFrom".to_string(),
        Value::String(created_from.as_str().to_string()),
    );
    group.insert("tabOrder".to_string(), Value::from(tab_order));
    group.insert("isCollapsed".to_string(), Value::Bool(false));
    group.insert("color".to_string(), Value::Null);
    group.insert("createdAt".to_string(), Value::from(now));
    group.insert("updatedAt".to_string(), Value::from(now));
    Value::Object(group)
}

/// `tabOrder = max(existing) + 1`, or `0` for the first group (oracle
/// `createProjectGroup`). Rows without a numeric order are ignored.
pub fn next_tab_order(groups: &[Value]) -> u64 {
    let mut max: Option<i64> = None;
    for group in groups {
        if let Some(order) = group.get("tabOrder").and_then(Value::as_i64) {
            max = Some(max.map_or(order, |current| current.max(order)));
        }
    }
    max.map_or(0, |value| (value + 1) as u64)
}

/// Every group id in the subtree rooted at `root_group_id` (oracle
/// `getProjectGroupSubtreeIds`); includes the root itself.
pub fn project_group_subtree_ids(groups: &[Value], root_group_id: &str) -> HashSet<String> {
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for group in groups {
        let (Some(id), Some(parent)) = (
            group.get("id").and_then(Value::as_str),
            group.get("parentGroupId").and_then(Value::as_str),
        ) else {
            continue;
        };
        children.entry(parent).or_default().push(id);
    }
    let mut subtree = HashSet::new();
    let mut pending = vec![root_group_id];
    while let Some(group_id) = pending.pop() {
        if !subtree.insert(group_id.to_string()) {
            continue;
        }
        if let Some(kids) = children.get(group_id) {
            pending.extend(kids.iter().copied());
        }
    }
    subtree
}

/// Next manual rank inside one group bucket (oracle `getNextProjectGroupOrder`):
/// `max(projectGroupOrder) + 1` over repos in the same bucket, `0` when empty.
pub fn next_project_group_order(repos: &[Value], group_id: Option<&str>) -> u64 {
    let mut max: i64 = -1;
    for repo in repos {
        let repo_group = repo.get("projectGroupId").and_then(Value::as_str);
        if repo_group != group_id {
            continue;
        }
        if let Some(order) = json_integer(repo.get("projectGroupOrder")) {
            max = max.max(order);
        }
    }
    (max + 1) as u64
}

/// Trim path separators the way the oracle's import path helpers do, keeping
/// POSIX/Windows roots intact (`/` -> `/`, `C:\` -> `C:/`).
pub fn trim_path_separators(path: &str) -> String {
    let folded = path.replace('\\', "/");
    if folded == "/" || is_windows_drive_root(&folded) {
        return folded;
    }
    let trimmed = folded.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_windows_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    match bytes.len() {
        2 => drive,
        3 => drive && bytes[2] == b'/',
        _ => false,
    }
}

fn nullable_string(value: Option<&str>) -> Value {
    match value {
        Some(value) => Value::String(value.to_string()),
        None => Value::Null,
    }
}

fn json_integer(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(integer) = value.as_i64() {
        return Some(integer);
    }
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .map(|number| number.floor() as i64)
}

/// Default bounded-scan depth (oracle `DEFAULT_MAX_DEPTH`).
pub const NESTED_SCAN_DEFAULT_MAX_DEPTH: u64 = 3;
/// Default bounded-scan repo cap (oracle `DEFAULT_MAX_REPOS`).
pub const NESTED_SCAN_DEFAULT_MAX_REPOS: u64 = 100;
/// Directory names the scan never descends into (oracle `SKIPPED_DIRS`).
pub const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    ".next",
    "dist",
    "build",
    ".cache",
    "vendor",
    "__pycache__",
    ".turbo",
    ".parcel-cache",
];
/// VCS metadata directories the scan never descends into (oracle
/// `VCS_METADATA_DIRS`).
pub const VCS_METADATA_DIRS: &[&str] = &[".git", ".svn", ".hg", ".jj", ".sl", ".repo", "CVS"];

/// Normalized bounded-scan options (oracle
/// `NormalizedNestedRepoScanOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestedRepoScanOptions {
    pub max_depth: u64,
    pub max_repos: u64,
    pub timeout_ms: Option<u64>,
}

/// Normalize renderer-supplied scan options exactly like the oracle
/// (`nested-repo-scan-rules.ts:55-73`): `maxDepth` default 3 clamp 1..8,
/// `maxRepos` default 100 clamp 1..500, `timeoutMs` default `null` clamp
/// 500..30000; non-numbers fall back to the defaults, `null` stays `null`.
pub fn normalize_nested_repo_scan_options(options: &Value) -> NestedRepoScanOptions {
    let field = |name: &str| options.as_object().and_then(|map| map.get(name));
    let max_depth = field("maxDepth")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map_or(NESTED_SCAN_DEFAULT_MAX_DEPTH, |value| {
            clamp_floor(value, 1, 8)
        });
    let max_repos = field("maxRepos")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map_or(NESTED_SCAN_DEFAULT_MAX_REPOS, |value| {
            clamp_floor(value, 1, 500)
        });
    let timeout_ms = match field("timeoutMs") {
        None | Some(Value::Null) => None,
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(|number| clamp_floor(number, 500, 30_000)),
    };
    NestedRepoScanOptions {
        max_depth,
        max_repos,
        timeout_ms,
    }
}

fn clamp_floor(value: f64, min: u64, max: u64) -> u64 {
    value.floor().clamp(min as f64, max as f64) as u64
}

/// `shouldSkipDirectory` (oracle `nested-repo-scan-rules.ts:75-83`): VCS
/// metadata and dependency/build directories at any depth, plus dot-directories
/// below the scan root.
pub fn should_skip_nested_repo_directory(name: &str, depth: u64) -> bool {
    VCS_METADATA_DIRS.contains(&name)
        || SKIPPED_DIRS.contains(&name)
        || (depth > 0 && name.starts_with('.'))
}

/// One parsed `.gitignore` rule (oracle `IgnoreRule`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedRepoIgnoreRule {
    pub pattern: String,
    pub negate: bool,
    pub basename_only: bool,
    pub base_segments: Vec<String>,
}

/// Parse `.gitignore` content relative to the directory's segment path (oracle
/// `parseGitignoreRules`).
pub fn parse_nested_repo_gitignore_rules(
    content: &str,
    base_segments: &[String],
) -> Vec<NestedRepoIgnoreRule> {
    let mut rules = Vec::new();
    for raw_line in content.split('\n') {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (negate, unprefixed) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let anchored = unprefixed.starts_with('/');
        let pattern = unprefixed
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string();
        if pattern.is_empty() {
            continue;
        }
        rules.push(NestedRepoIgnoreRule {
            basename_only: !anchored && !pattern.contains('/'),
            pattern,
            negate,
            base_segments: base_segments.to_vec(),
        });
    }
    rules
}

/// Whether `name` at `segments` is ignored by the accumulated rules, or is a
/// directory the scan always skips (oracle `isIgnoredNestedRepoDirectory`).
pub fn is_ignored_nested_repo_directory(
    name: &str,
    segments: &[String],
    rules: &[NestedRepoIgnoreRule],
) -> bool {
    let mut ignored = false;
    for rule in rules {
        if segments.len() <= rule.base_segments.len() {
            continue;
        }
        let relative = &segments[rule.base_segments.len()..];
        let matches = if rule.basename_only {
            relative
                .iter()
                .any(|segment| glob_segment_matches(&rule.pattern, segment))
        } else {
            let pattern_segments: Vec<&str> = rule.pattern.split('/').collect();
            let candidate_segments: Vec<&str> = relative.iter().map(String::as_str).collect();
            path_segments_match(&pattern_segments, &candidate_segments)
        };
        if matches {
            ignored = !rule.negate;
        }
    }
    ignored || should_skip_nested_repo_directory(name, segments.len() as u64 - 1)
}

/// Glob match for one path segment: only `*` and `?` are wildcards, everything
/// else is literal (oracle `globSegmentMatches`).
pub fn glob_segment_matches(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == value;
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut matched = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    matched[0][0] = true;
    for index in 1..=pattern.len() {
        if pattern[index - 1] == '*' {
            matched[index][0] = matched[index - 1][0];
        }
    }
    for index in 1..=pattern.len() {
        for position in 1..=value.len() {
            matched[index][position] = match pattern[index - 1] {
                '*' => matched[index - 1][position] || matched[index][position - 1],
                '?' => matched[index - 1][position - 1],
                literal => matched[index - 1][position - 1] && literal == value[position - 1],
            };
        }
    }
    matched[pattern.len()][value.len()]
}

/// Multi-segment match supporting `**` (oracle `pathSegmentsMatch`).
pub fn path_segments_match(pattern_segments: &[&str], candidate_segments: &[&str]) -> bool {
    fn matches_from(
        pattern: &[&str],
        candidate: &[&str],
        pattern_index: usize,
        candidate_index: usize,
    ) -> bool {
        if pattern_index >= pattern.len() {
            return candidate_index >= candidate.len();
        }
        if pattern[pattern_index] == "**" {
            return matches_from(pattern, candidate, pattern_index + 1, candidate_index)
                || (candidate_index < candidate.len()
                    && matches_from(pattern, candidate, pattern_index, candidate_index + 1));
        }
        candidate_index < candidate.len()
            && glob_segment_matches(pattern[pattern_index], candidate[candidate_index])
            && matches_from(pattern, candidate, pattern_index + 1, candidate_index + 1)
    }
    matches_from(pattern_segments, candidate_segments, 0, 0)
}

/// One repository discovered by the nested scan (oracle
/// `NestedRepoCandidate`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NestedRepoCandidate {
    pub path: String,
    pub display_name: String,
    pub depth: u64,
}

/// Whether the scan root itself is a repository (oracle
/// `NestedRepoScanResult['selectedPathKind']`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum NestedRepoSelectedPathKind {
    GitRepo,
    NonGitFolder,
}

/// Bounded nested-repo scan result (oracle `NestedRepoScanResult`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NestedRepoScanResult {
    pub selected_path: String,
    pub selected_path_kind: NestedRepoSelectedPathKind,
    pub repos: Vec<NestedRepoCandidate>,
    pub truncated: bool,
    pub timed_out: bool,
    pub stopped: bool,
    pub duration_ms: u64,
    pub max_depth: u64,
    pub max_repos: u64,
    pub timeout_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn created_from_round_trips_kebab_case() {
        assert_eq!(
            ProjectGroupCreatedFrom::parse("manual"),
            Some(ProjectGroupCreatedFrom::Manual)
        );
        assert_eq!(
            ProjectGroupCreatedFrom::parse("folder-scan"),
            Some(ProjectGroupCreatedFrom::FolderScan)
        );
        assert_eq!(
            ProjectGroupCreatedFrom::parse("migration"),
            Some(ProjectGroupCreatedFrom::Migration)
        );
        assert_eq!(ProjectGroupCreatedFrom::parse("nonsense"), None);
        assert_eq!(ProjectGroupCreatedFrom::FolderScan.as_str(), "folder-scan");
        assert_eq!(
            serde_json::to_value(ProjectGroupCreatedFrom::FolderScan).unwrap(),
            json!("folder-scan")
        );
    }

    #[test]
    fn new_group_uses_spec_defaults() {
        let group = new_project_group(
            "g1",
            "  My Group  ",
            Some("/root"),
            None,
            Some("parent"),
            ProjectGroupCreatedFrom::Manual,
            2,
            42,
        );
        assert_eq!(
            group,
            json!({
                "id": "g1",
                "name": "My Group",
                "parentPath": "/root",
                "connectionId": null,
                "parentGroupId": "parent",
                "createdFrom": "manual",
                "tabOrder": 2,
                "isCollapsed": false,
                "color": null,
                "createdAt": 42,
                "updatedAt": 42
            })
        );
    }

    #[test]
    fn blank_name_falls_back_to_the_default() {
        let group = new_project_group(
            "g1",
            "   ",
            None,
            None,
            None,
            ProjectGroupCreatedFrom::Manual,
            0,
            0,
        );
        assert_eq!(group["name"], DEFAULT_PROJECT_GROUP_NAME);
    }

    #[test]
    fn next_tab_order_ignores_missing_orders() {
        assert_eq!(next_tab_order(&[]), 0);
        assert_eq!(next_tab_order(&[json!({ "id": "a" })]), 0);
        assert_eq!(
            next_tab_order(&[
                json!({ "id": "a", "tabOrder": 0 }),
                json!({ "id": "b", "tabOrder": 4 }),
                json!({ "id": "c" }),
            ]),
            5
        );
    }

    #[test]
    fn subtree_ids_follow_parent_links() {
        let groups = vec![
            json!({ "id": "root" }),
            json!({ "id": "child", "parentGroupId": "root" }),
            json!({ "id": "grandchild", "parentGroupId": "child" }),
            json!({ "id": "other" }),
        ];
        let subtree = project_group_subtree_ids(&groups, "root");
        assert_eq!(subtree.len(), 3);
        assert!(subtree.contains("root"));
        assert!(subtree.contains("child"));
        assert!(subtree.contains("grandchild"));
        assert!(!subtree.contains("other"));
        assert_eq!(project_group_subtree_ids(&groups, "missing").len(), 1);
    }

    #[test]
    fn next_project_group_order_matches_oracle_bucket_rules() {
        let repos = vec![
            json!({ "id": "a", "projectGroupId": "g1", "projectGroupOrder": 1 }),
            json!({ "id": "b", "projectGroupId": "g1" }),
            json!({ "id": "c", "projectGroupId": null, "projectGroupOrder": 9 }),
            json!({ "id": "d" }),
        ];
        assert_eq!(next_project_group_order(&repos, Some("g1")), 2);
        assert_eq!(next_project_group_order(&repos, None), 10);
        assert_eq!(next_project_group_order(&repos, Some("missing")), 0);
    }

    #[test]
    fn trim_path_separators_keeps_roots() {
        assert_eq!(trim_path_separators("/a/b/"), "/a/b");
        assert_eq!(trim_path_separators("/"), "/");
        assert_eq!(trim_path_separators("C:\\a\\b\\"), "C:/a/b");
        assert_eq!(trim_path_separators("C:\\"), "C:/");
    }

    #[test]
    fn scan_options_normalize_like_the_oracle() {
        let defaults = normalize_nested_repo_scan_options(&json!({}));
        assert_eq!(defaults.max_depth, 3);
        assert_eq!(defaults.max_repos, 100);
        assert_eq!(defaults.timeout_ms, None);

        let clamped = normalize_nested_repo_scan_options(&json!({
            "maxDepth": 99.9,
            "maxRepos": 0,
            "timeoutMs": 10
        }));
        assert_eq!(clamped.max_depth, 8);
        assert_eq!(clamped.max_repos, 1);
        assert_eq!(clamped.timeout_ms, Some(500));

        let negative = normalize_nested_repo_scan_options(&json!({
            "maxDepth": -4,
            "maxRepos": -1,
            "timeoutMs": 90_000
        }));
        assert_eq!(negative.max_depth, 1);
        assert_eq!(negative.max_repos, 1);
        assert_eq!(negative.timeout_ms, Some(30_000));

        let explicit_null = normalize_nested_repo_scan_options(&json!({ "timeoutMs": null }));
        assert_eq!(explicit_null.timeout_ms, None);

        let junk = normalize_nested_repo_scan_options(&json!({
            "maxDepth": "3",
            "maxRepos": true,
            "timeoutMs": "soon"
        }));
        assert_eq!(junk, defaults);
    }

    #[test]
    fn skipped_directory_rules_match_the_oracle() {
        assert!(should_skip_nested_repo_directory("node_modules", 1));
        assert!(should_skip_nested_repo_directory("dist", 2));
        assert!(should_skip_nested_repo_directory(".git", 1));
        assert!(should_skip_nested_repo_directory(".svn", 3));
        assert!(should_skip_nested_repo_directory(".hidden", 1));
        assert!(!should_skip_nested_repo_directory(".hidden", 0));
        assert!(!should_skip_nested_repo_directory("src", 1));
        assert!(should_skip_nested_repo_directory("CVS", 0));
    }

    #[test]
    fn gitignore_rules_ignore_and_negate() {
        let rules =
            parse_nested_repo_gitignore_rules("# comment\n\nignored/\n!keep\ndocs/*.md\n", &[]);
        assert_eq!(rules.len(), 3);
        let segments = |parts: &[&str]| {
            parts
                .iter()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
        };

        assert!(is_ignored_nested_repo_directory(
            "ignored",
            &segments(&["ignored"]),
            &rules
        ));
        assert!(!is_ignored_nested_repo_directory(
            "keep",
            &segments(&["keep"]),
            &rules
        ));
        assert!(is_ignored_nested_repo_directory(
            "readme.md",
            &segments(&["docs", "readme.md"]),
            &rules
        ));
        assert!(!is_ignored_nested_repo_directory(
            "src",
            &segments(&["src"]),
            &rules
        ));
    }

    #[test]
    fn gitignore_anchored_rules_stay_scoped_to_their_directory() {
        let rules = parse_nested_repo_gitignore_rules("/ignored\n", &[]);
        let segments = |parts: &[&str]| {
            parts
                .iter()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
        };
        assert!(is_ignored_nested_repo_directory(
            "ignored",
            &segments(&["ignored"]),
            &rules
        ));
        assert!(!is_ignored_nested_repo_directory(
            "ignored",
            &segments(&["active", "ignored"]),
            &rules
        ));
    }

    #[test]
    fn glob_segment_matches_supports_star_and_question() {
        assert!(glob_segment_matches("*.md", "readme.md"));
        assert!(glob_segment_matches("read?e.md", "readme.md"));
        assert!(!glob_segment_matches("*.md", "readme.txt"));
        assert!(glob_segment_matches("plain", "plain"));
        assert!(!glob_segment_matches("plain", "plainly"));
    }

    #[test]
    fn scan_result_serializes_camel_case() {
        let result = NestedRepoScanResult {
            selected_path: "/workspace".to_string(),
            selected_path_kind: NestedRepoSelectedPathKind::NonGitFolder,
            repos: vec![NestedRepoCandidate {
                path: "/workspace/api".to_string(),
                display_name: "api".to_string(),
                depth: 1,
            }],
            truncated: false,
            timed_out: false,
            stopped: true,
            duration_ms: 5,
            max_depth: 3,
            max_repos: 100,
            timeout_ms: None,
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({
                "selectedPath": "/workspace",
                "selectedPathKind": "non_git_folder",
                "repos": [{ "path": "/workspace/api", "displayName": "api", "depth": 1 }],
                "truncated": false,
                "timedOut": false,
                "stopped": true,
                "durationMs": 5,
                "maxDepth": 3,
                "maxRepos": 100,
                "timeoutMs": null
            })
        );
    }
}
