//! Worktree creation support: name sanitizing, branch prefix/name building,
//! git username resolution and base-ref resolution.
//!
//! Mirrors `orca:src/main/ipc/worktree-logic.ts` (`sanitizeWorktreeName`,
//! `computeWorktreePath`/`ensurePathWithinWorkspace`),
//! `orca:src/shared/branch-prefix.ts`, `orca:src/main/git/git-username.ts`
//! (minus the `gh` CLI probe), `orca:src/main/git/repo-default-base-ref.ts`
//! and `orca:src/main/worktree-create-base.ts`.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use ade_core::errors::CoreError;

use crate::git_command_failed;
use crate::runner::run_git_in;

/// Read-only config probe timeout, mirroring `LOCAL_GIT_READ_TIMEOUT_MS`
/// (`orca:src/main/git/git-username.ts:21`).
const CONFIG_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Mirrors `DEFAULT_BASE_REF_PROBE_TIMEOUT_MS`
/// (`orca:src/main/git/repo-default-base-ref.ts:12`).
const BASE_REF_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
/// Explicit config keys, in priority order (`orca:src/main/git/git-username.ts:6`).
const EXPLICIT_USERNAME_CONFIG_KEYS: [&str; 2] = ["github.user", "user.username"];

/// Probe order for the default base ref. Mirrors the oracle's
/// `DEFAULT_BASE_REF_PROBES` ordering (`orca:src/main/git/repo-default-base-ref.ts:21-26`):
/// `origin/main` outranks `origin/master`, and remote-tracking refs outrank
/// local branches. `refs/remotes/origin/HEAD` leads because a verified
/// symbolic origin HEAD is authoritative.
pub const DEFAULT_BASE_REF_CANDIDATES: &[&str] = &[
    "refs/remotes/origin/HEAD",
    "refs/remotes/origin/main",
    "origin/main",
    "refs/remotes/origin/master",
    "origin/master",
    "refs/heads/main",
    "main",
    "refs/heads/master",
    "master",
];

/// Sanitize a worktree name for use in branch names and directory paths.
///
/// Mirrors `sanitizeWorktreeName` (`orca:src/main/ipc/worktree-logic.ts:36-62`)
/// minus the emoji shortcode catalog: Unicode letters/numbers and `._-` are
/// kept, every other run of characters collapses to one `-`, `..` runs collapse
/// to `.`, and leading/trailing `[.-]` are trimmed. Inputs that sanitize to
/// nothing are rejected.
pub fn sanitize_worktree_name(name: &str) -> Result<String, CoreError> {
    let mut replaced = String::with_capacity(name.len());
    let mut in_invalid_run = false;
    for ch in name.trim().chars() {
        if ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            in_invalid_run = false;
            replaced.push(ch);
        } else if !in_invalid_run {
            in_invalid_run = true;
            replaced.push('-');
        }
    }

    // Why: git check-ref-format rejects any ref containing `..`, so collapse
    // runs (including internal ones) before the edge trim.
    let mut collapsed = String::with_capacity(replaced.len());
    for ch in replaced.chars() {
        if (ch == '-' && collapsed.ends_with('-')) || (ch == '.' && collapsed.ends_with('.')) {
            continue;
        }
        collapsed.push(ch);
    }

    let sanitized = collapsed.trim_matches(|ch| ch == '.' || ch == '-');
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        return Err(CoreError::InvalidInput("Invalid worktree name".to_string()));
    }
    Ok(sanitized.to_string())
}

/// Normalize a configured branch prefix before the `/` join.
///
/// Mirrors `normalizeBranchPrefix` (`orca:src/shared/branch-prefix.ts:38-43`).
pub fn normalize_branch_prefix(raw: &str) -> String {
    raw.trim()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Pick the raw prefix value the configured strategy contributes.
///
/// Mirrors `selectBranchPrefixInput` (`orca:src/shared/branch-prefix.ts:14-26`);
/// unknown strategies read as no prefix, like the oracle's `undefined` return.
pub fn select_branch_prefix_input(
    strategy: &str,
    custom: Option<&str>,
    git_username: Option<&str>,
) -> Option<String> {
    match strategy {
        "git-username" => git_username.map(str::to_string),
        "custom" => custom.map(str::to_string),
        _ => None,
    }
}

/// Join the configured prefix and the sanitized name with exactly one `/`.
///
/// Mirrors `computeBranchName` (`orca:src/main/ipc/worktree-branch-name.ts:30-37`);
/// an empty normalized prefix yields the bare name.
pub fn build_branch_name(prefix: Option<&str>, name: &str) -> String {
    let prefix = prefix.map(normalize_branch_prefix).unwrap_or_default();
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Resolve the branch name a create should use: an explicit override is
/// validated and kept verbatim, otherwise the configured prefix joins the
/// sanitized name.
///
/// Mirrors `resolveCreateBranchName`
/// (`orca:src/main/ipc/worktree-remote.ts:681-697`): a `-`-leading override is
/// rejected before git's `check-ref-format --branch` validation, and the
/// branch prefix never applies to an override.
pub fn resolve_create_branch_name(
    repo_path: &str,
    branch_name_override: Option<&str>,
    prefix: Option<&str>,
    name: &str,
) -> Result<String, CoreError> {
    let Some(override_name) = branch_name_override.filter(|value| !value.is_empty()) else {
        return Ok(build_branch_name(prefix, name));
    };
    if override_name.starts_with('-') {
        return Err(CoreError::InvalidInput(
            "Branch name must not start with \"-\"".to_string(),
        ));
    }
    let args = ["check-ref-format", "--branch", override_name];
    let output = run_git_in(repo_path, &args, CONFIG_PROBE_TIMEOUT, None)?;
    if !output.status.success() {
        return Err(git_command_failed(&args, &output));
    }
    Ok(override_name.to_string())
}

/// Resolve the branch-prefix username from explicit git config.
///
/// Mirrors the explicit-config half of `resolveLocalGitUsernameDetailed`
/// (`orca:src/main/git/git-username.ts:303-336`): `github.user` then
/// `user.username`, each normalized to a branch-safe login. The oracle's
/// GitHub-remote-gated `gh` CLI probe is intentionally not ported.
pub fn resolve_git_username(repo_path: &str) -> Option<String> {
    for key in EXPLICIT_USERNAME_CONFIG_KEYS {
        let output = match run_git_in(
            repo_path,
            &["config", "--get", key],
            CONFIG_PROBE_TIMEOUT,
            None,
        ) {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        let value = String::from_utf8_lossy(&output.stdout);
        if let Some(username) = normalize_configured_login(&value) {
            return Some(username);
        }
    }
    None
}

/// Resolve the repository's default base ref.
///
/// Verifies [`DEFAULT_BASE_REF_CANDIDATES`] in order with
/// `git rev-parse --verify --quiet <candidate>^{commit}` and returns the first
/// candidate that resolves. Mirrors `getDefaultBaseRef`
/// (`orca:src/main/git/repo-default-base-ref.ts:52-75`); the failure message is
/// verbatim from `orca:src/main/ipc/worktree-remote.ts:2374-2379`.
pub fn resolve_default_base_ref(repo_path: &str) -> Result<String, CoreError> {
    for candidate in DEFAULT_BASE_REF_CANDIDATES {
        if ref_resolves(repo_path, candidate) {
            return Ok((*candidate).to_string());
        }
    }
    Err(CoreError::InvalidInput(
        "Could not resolve a default base ref for this repo. Pick a base branch explicitly and try again."
            .to_string(),
    ))
}

/// Resolve the base ref a worktree create should start from.
///
/// Priority mirrors `resolveWorktreeCreateBase`
/// (`orca:src/main/worktree-create-base.ts:8-29`): an explicit request is
/// authoritative, a persisted repo base ref is used when it still resolves,
/// and otherwise the detected default wins. Errors when nothing resolves.
pub fn resolve_create_base(
    repo_path: &str,
    explicit: Option<&str>,
    repo_base_ref: Option<&str>,
) -> Result<String, CoreError> {
    if let Some(explicit) = explicit.filter(|value| !value.is_empty()) {
        return Ok(explicit.to_string());
    }
    if let Some(repo_ref) = repo_base_ref.filter(|value| !value.is_empty()) {
        if ref_resolves(repo_path, repo_ref) {
            return Ok(repo_ref.to_string());
        }
    }
    resolve_default_base_ref(repo_path)
}

/// Compute the filesystem path where the worktree directory will be created.
///
/// Mirrors `computeWorktreePath` (`orca:src/main/ipc/worktree-logic.ts:103-131`)
/// plus `ensurePathWithinWorkspace` (`:81-91`): with nesting the worktree lives
/// under `<root>/<repo basename>/<name>`, otherwise under `<root>/<name>`. The
/// root must be absolute, and the normalized target must stay inside it.
pub fn compute_worktree_path(
    root: &str,
    repo_basename: &str,
    nest: bool,
    name: &str,
) -> Result<String, CoreError> {
    let root_path = Path::new(root);
    if !root_path.is_absolute() {
        return Err(CoreError::InvalidInput(
            "Worktree root must be an absolute path".to_string(),
        ));
    }
    let root_path = normalize_lexically(root_path);

    let target = if nest {
        let repo_name = repo_basename.strip_suffix(".git").unwrap_or(repo_basename);
        root_path.join(repo_name).join(name)
    } else {
        root_path.join(name)
    };
    let target = normalize_lexically(&target);

    if !target.starts_with(&root_path) {
        return Err(CoreError::InvalidInput("Invalid worktree path".to_string()));
    }
    Ok(target.to_string_lossy().into_owned())
}

/// Mirrors `normalizeGitUsername` (`orca:src/main/git/git-username.ts:23-31`):
/// trim, drop the `@host` suffix, and strip a leading `123+` prefix.
fn normalize_git_username(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let local_part = trimmed.split('@').next().unwrap_or(trimmed);
    let digits_end = local_part
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(local_part.len());
    if digits_end > 0 && local_part[digits_end..].starts_with('+') {
        local_part[digits_end + 1..].to_string()
    } else {
        local_part.to_string()
    }
}

/// Mirrors `isBranchSafeHostedLogin` (`orca:src/main/git/git-username.ts:56-65`):
/// a single slash-free token with no dot placement git refs reject.
fn is_branch_safe_hosted_login(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 {
        return false;
    }
    let mut chars = value.chars();
    let first = chars.next().expect("value is non-empty");
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) {
        return false;
    }
    !value.contains("..") && !value.ends_with('.') && !value.ends_with(".lock")
}

/// Mirrors `normalizeConfiguredLogin` (`orca:src/main/git/git-username.ts:72-76`).
fn normalize_configured_login(value: &str) -> Option<String> {
    let normalized = normalize_git_username(value);
    if is_branch_safe_hosted_login(&normalized) {
        Some(normalized)
    } else {
        None
    }
}

/// `git rev-parse --verify --quiet <reference>^{commit}` succeeds.
fn ref_resolves(repo_path: &str, reference: &str) -> bool {
    let spec = format!("{reference}^{{commit}}");
    matches!(
        run_git_in(
            repo_path,
            &["rev-parse", "--verify", "--quiet", &spec],
            BASE_REF_PROBE_TIMEOUT,
            None
        ),
        Ok(output) if output.status.success()
    )
}

/// Lexical path normalization: drops `.` components and resolves `..` by
/// popping, never climbing above the root (mirrors Node's `path.resolve`).
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_invalid_input(result: Result<String, CoreError>) -> String {
        match result {
            Err(CoreError::InvalidInput(message)) => message,
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn sanitize_keeps_unicode_word_chars_and_collapses_others() {
        assert_eq!(
            sanitize_worktree_name("feature/login").unwrap(),
            "feature-login"
        );
        assert_eq!(
            sanitize_worktree_name(" 我的 工作..区 ").unwrap(),
            "我的-工作.区"
        );
        assert_eq!(sanitize_worktree_name("a...b").unwrap(), "a.b");
        assert_eq!(sanitize_worktree_name(" -a- ").unwrap(), "a");
        assert_eq!(sanitize_worktree_name("a  -  b").unwrap(), "a-b");
        assert_eq!(
            sanitize_worktree_name("under_score.ok").unwrap(),
            "under_score.ok"
        );
    }

    #[test]
    fn sanitize_rejects_input_that_collapses_to_nothing() {
        // oracle worktree-logic.ts:36-62 only falls back to "workspace" when the
        // untouched input contains emoji; every other input that sanitizes to
        // nothing throws 'Invalid worktree name' (the ade port has no emoji
        // shortcode catalog, so those inputs stay errors too).
        for input in ["", "   ", "***", ".", "..", "...", "-", "/", "./.."] {
            assert_eq!(
                expect_invalid_input(sanitize_worktree_name(input)),
                "Invalid worktree name"
            );
        }
    }

    #[test]
    fn normalize_branch_prefix_strips_and_collapses_slashes() {
        assert_eq!(
            normalize_branch_prefix(" team//frontend/ "),
            "team/frontend"
        );
        assert_eq!(normalize_branch_prefix("///"), "");
        assert_eq!(normalize_branch_prefix("team/frontend"), "team/frontend");
        assert_eq!(
            normalize_branch_prefix("//team///frontend//"),
            "team/frontend"
        );
    }

    #[test]
    fn select_branch_prefix_input_maps_strategy_to_its_field() {
        assert_eq!(
            select_branch_prefix_input("git-username", None, Some("alice")),
            Some("alice".to_string())
        );
        assert_eq!(
            select_branch_prefix_input("git-username", Some("team"), Some("")),
            Some(String::new())
        );
        assert_eq!(select_branch_prefix_input("git-username", None, None), None);
        assert_eq!(
            select_branch_prefix_input("custom", Some("team/"), Some("alice")),
            Some("team/".to_string())
        );
        assert_eq!(
            select_branch_prefix_input("custom", Some(""), Some("alice")),
            Some(String::new())
        );
        assert_eq!(
            select_branch_prefix_input("custom", None, Some("alice")),
            None
        );
        assert_eq!(
            select_branch_prefix_input("none", Some("team"), Some("alice")),
            None
        );
        assert_eq!(
            select_branch_prefix_input("unknown", Some("team"), Some("alice")),
            None
        );
    }

    #[test]
    fn build_branch_name_joins_with_single_slash() {
        assert_eq!(
            build_branch_name(Some("alice"), "fix-auth"),
            "alice/fix-auth"
        );
        assert_eq!(build_branch_name(None, "fix-auth"), "fix-auth");
        assert_eq!(build_branch_name(Some(""), "fix-auth"), "fix-auth");
        assert_eq!(
            build_branch_name(Some("/team//"), "fix-auth"),
            "team/fix-auth"
        );
        assert_eq!(build_branch_name(Some("///"), "fix-auth"), "fix-auth");
    }

    #[test]
    fn resolve_create_branch_name_keeps_overrides_verbatim_and_rejects_dash_leading() {
        assert_eq!(
            resolve_create_branch_name("/nonexistent", None, Some("team"), "fix").unwrap(),
            "team/fix"
        );
        let error = resolve_create_branch_name("/nonexistent", Some("-bad"), Some("team"), "fix")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Invalid input: Branch name must not start with \"-\""
        );
    }

    #[test]
    fn compute_worktree_path_nests_under_repo_basename() {
        assert_eq!(
            compute_worktree_path("/ws", "my-repo", true, "fix").unwrap(),
            "/ws/my-repo/fix"
        );
        assert_eq!(
            compute_worktree_path("/ws", "my-repo", false, "fix").unwrap(),
            "/ws/fix"
        );
        assert_eq!(
            compute_worktree_path("/ws/", "my-repo", false, "fix").unwrap(),
            "/ws/fix"
        );
        assert_eq!(
            compute_worktree_path("/ws", "my-repo.git", true, "fix").unwrap(),
            "/ws/my-repo/fix"
        );
        assert_eq!(
            compute_worktree_path("/ws/./nested/..", "my-repo", false, "sub/fix").unwrap(),
            "/ws/sub/fix"
        );
    }

    #[test]
    fn compute_worktree_path_rejects_escapes_and_relative_roots() {
        assert_eq!(
            expect_invalid_input(compute_worktree_path("/ws", "my-repo", false, "../escape")),
            "Invalid worktree path"
        );
        assert!(compute_worktree_path("/ws", "my-repo", true, "../../escape").is_err());
        assert!(compute_worktree_path("/ws", "my-repo", false, "/etc/passwd").is_err());
        assert!(compute_worktree_path("/ws", "../escape", true, "fix").is_err());
        assert!(compute_worktree_path("ws", "my-repo", false, "fix").is_err());
        assert!(compute_worktree_path("", "my-repo", false, "fix").is_err());
    }

    #[test]
    fn default_base_ref_candidates_match_oracle_order() {
        assert_eq!(
            DEFAULT_BASE_REF_CANDIDATES,
            [
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
                "origin/main",
                "refs/remotes/origin/master",
                "origin/master",
                "refs/heads/main",
                "main",
                "refs/heads/master",
                "master",
            ]
            .as_slice()
        );
    }

    #[test]
    fn normalize_configured_login_matches_branch_safe_rules() {
        assert_eq!(
            normalize_configured_login("alice").as_deref(),
            Some("alice")
        );
        assert_eq!(
            normalize_configured_login(" 123+alice@example.com \n").as_deref(),
            Some("alice")
        );
        assert_eq!(
            normalize_configured_login("alice-x_1.b").as_deref(),
            Some("alice-x_1.b")
        );
        assert_eq!(normalize_configured_login(""), None);
        assert_eq!(normalize_configured_login("   "), None);
        assert_eq!(normalize_configured_login("bad name"), None);
        assert_eq!(normalize_configured_login(".hidden"), None);
        assert_eq!(normalize_configured_login("team/alice"), None);
        assert_eq!(normalize_configured_login("a..b"), None);
        assert_eq!(normalize_configured_login("alice."), None);
        assert_eq!(normalize_configured_login("alice.lock"), None);
        assert_eq!(normalize_configured_login(&"a".repeat(256)), None);
        assert_eq!(
            normalize_configured_login(&"a".repeat(255)).as_deref(),
            Some("a".repeat(255).as_str())
        );
    }
}
