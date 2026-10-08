//! Integration coverage for the git command surface: argument mapping, the
//! cancel registry, and every `*_impl` thin wrapper against real temp repos.

use std::path::{Path, PathBuf};
use std::process::Command;

use ade_bridge::commands::git::{
    branch_compare_impl, branch_diff_impl, bulk_discard_impl, bulk_stage_impl, bulk_unstage_impl,
    commit_compare_impl, commit_diff_impl, commit_impl, conflict_operation_impl, diff_impl,
    discard_impl, git_read_impl, history_impl, is_allowed_git_read_args, remote_urls_impl,
    require_authorized_worktree, stage_impl, status_impl, unstage_impl, upstream_status_impl,
    GitStatusArgs,
};
use ade_bridge::state::GitCancelRegistry;
use ade_fs::FsService;
use ade_git::diff::GitDiffResult;
use ade_git::runner::CancelToken;
use ade_git::status::GitConflictOperation;
use ade_git::status_read::StatusOptions;
use serde_json::json;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("ade-bridge-git-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test dir");
        Self {
            path: path.canonicalize().expect("canonicalize test dir"),
        }
    }

    fn path_str(&self) -> &str {
        self.path.to_str().expect("utf-8 path")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// git with the user's environment stripped, so host config cannot leak into
/// fixtures.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ade Test")
        .env("GIT_AUTHOR_EMAIL", "ade-test@example.com")
        .env("GIT_COMMITTER_NAME", "Ade Test")
        .env("GIT_COMMITTER_EMAIL", "ade-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_repo(dir: &TestDir) {
    git(&dir.path, &["-c", "init.defaultBranch=main", "init"]);
}

fn commit_file(dir: &TestDir, name: &str, content: &str, message: &str) -> String {
    std::fs::write(dir.path.join(name), content).expect("write file");
    git(&dir.path, &["add", "--", name]);
    git(&dir.path, &["commit", "-m", message]);
    git(&dir.path, &["rev-parse", "HEAD"])
}

fn text_diff(result: GitDiffResult) -> (String, String) {
    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => (original_content, modified_content),
        other => panic!("expected a text diff, got {other:?}"),
    }
}

#[test]
fn status_args_map_to_status_options() {
    let args: GitStatusArgs = serde_json::from_value(json!({"worktreePath":"/tmp/x"})).unwrap();
    let options = args.to_status_options();
    assert_eq!(options.limit, None); // None 由 resolve_status_limit 落 1000
    assert!(!options.include_ignored);
    // Ruling: the renderer only sends the field when it is explicitly false,
    // so an absent `includeLineStats` must still turn line stats on.
    assert!(options.include_line_stats);
    assert_eq!(options.branch_line_total_merge_base, None);
}

#[test]
fn status_args_map_explicit_flags_and_merge_base() {
    let args: GitStatusArgs = serde_json::from_value(json!({
        "worktreePath": "/tmp/x",
        "includeIgnored": true,
        "includeLineStats": false,
        "branchLineTotalMergeBase": "0123456789abcdef0123456789abcdef01234567",
        "requestToken": "tok-1",
        "connectionId": "conn-1"
    }))
    .unwrap();
    let options = args.to_status_options();
    assert!(options.include_ignored);
    assert!(!options.include_line_stats);
    assert_eq!(
        options.branch_line_total_merge_base.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    assert_eq!(args.request_token.as_deref(), Some("tok-1"));
}

#[test]
fn cancel_registry_cancels_registered_token_once() {
    let registry = GitCancelRegistry::new();
    let token = registry.register("t1");
    assert!(!token.is_cancelled());
    assert!(registry.cancel("t1"));
    assert!(token.is_cancelled());
    assert!(!registry.cancel("t1")); // 已移除
}

#[test]
fn cancel_registry_supersedes_same_token_and_finish_deregisters() {
    let registry = GitCancelRegistry::new();
    let first = registry.register("t1");
    let second = registry.register("t1");
    assert!(first.is_cancelled(), "a superseded token is cancelled");
    assert!(!second.is_cancelled());

    registry.finish("t1");
    assert!(!registry.cancel("t1"), "finish released the registration");
}

#[test]
fn status_impl_returns_empty_entries_for_non_repo() {
    let dir = TestDir::new("status-non-repo");
    let result = status_impl(dir.path_str(), &StatusOptions::default(), None).unwrap();
    assert!(result.entries.is_empty());
    assert_eq!(result.conflict_operation, GitConflictOperation::Unknown);
}

#[test]
fn status_impl_reports_branch_and_line_stats() {
    let dir = TestDir::new("status-entries");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "one\n", "init");
    std::fs::write(dir.path.join("a.txt"), "one\ntwo\n").unwrap();

    let options = StatusOptions {
        include_line_stats: true,
        ..StatusOptions::default()
    };
    let result = status_impl(dir.path_str(), &options, None).unwrap();

    assert_eq!(result.branch.as_deref(), Some("refs/heads/main"));
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].path, "a.txt");
    assert_eq!(result.entries[0].added, Some(1));
    assert_eq!(result.entries[0].removed, Some(0));
}

#[test]
fn status_impl_cancelled_token_rejects() {
    let dir = TestDir::new("status-cancel");
    init_repo(&dir);
    let token = CancelToken::new();
    token.cancel();

    let error = status_impl(dir.path_str(), &StatusOptions::default(), Some(&token)).unwrap_err();

    assert!(
        error.to_string().contains("cancelled"),
        "unexpected error: {error}"
    );
}

#[test]
fn diff_impl_reads_the_unstaged_worktree_diff() {
    let dir = TestDir::new("diff");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "one\n", "init");
    std::fs::write(dir.path.join("a.txt"), "one\ntwo\n").unwrap();

    let result = diff_impl(dir.path_str(), "a.txt", false, false).unwrap();

    assert_eq!(
        text_diff(result),
        ("one\n".to_string(), "one\ntwo\n".to_string())
    );
}

#[test]
fn branch_diff_impl_uses_merge_base_as_left_rev() {
    let dir = TestDir::new("branch-diff");
    init_repo(&dir);
    commit_file(&dir, "file.txt", "base\n", "base");
    git(&dir.path, &["checkout", "-b", "feature"]);
    commit_file(&dir, "file.txt", "base\nfeature\n", "feature");
    git(&dir.path, &["checkout", "main"]);
    commit_file(&dir, "file.txt", "base\nmain side\n", "main side");

    let head_oid = git(&dir.path, &["rev-parse", "feature"]);
    let base_oid = git(&dir.path, &["rev-parse", "main"]);
    let merge_base = git(&dir.path, &["merge-base", "main", "feature"]);
    assert_ne!(merge_base, base_oid, "fixture must diverge after the fork");

    let result =
        branch_diff_impl(dir.path_str(), &merge_base, &head_oid, "file.txt", None).unwrap();

    assert_eq!(
        text_diff(result),
        ("base\n".to_string(), "base\nfeature\n".to_string())
    );
}

#[test]
fn commit_diff_impl_reads_the_parent_and_the_root_commit() {
    let dir = TestDir::new("commit-diff");
    init_repo(&dir);
    let first = commit_file(&dir, "a.txt", "one\n", "first");
    let second = commit_file(&dir, "a.txt", "one\ntwo\n", "second");

    let with_parent =
        commit_diff_impl(dir.path_str(), &second, Some(&first), "a.txt", None).unwrap();
    assert_eq!(
        text_diff(with_parent),
        ("one\n".to_string(), "one\ntwo\n".to_string())
    );

    let root = commit_diff_impl(dir.path_str(), &first, None, "a.txt", None).unwrap();
    assert_eq!(text_diff(root), (String::new(), "one\n".to_string()));
}

#[test]
fn stage_and_unstage_impls_move_paths_in_the_index() {
    let dir = TestDir::new("staging");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "a\n", "base");
    std::fs::write(dir.path.join("a.txt"), "a\nchanged\n").unwrap();
    std::fs::write(dir.path.join("b.txt"), "b\n").unwrap();

    stage_impl(dir.path_str(), "a.txt").unwrap();
    assert_eq!(
        git(&dir.path, &["diff", "--cached", "--name-only"]),
        "a.txt"
    );

    unstage_impl(dir.path_str(), "a.txt").unwrap();
    assert_eq!(git(&dir.path, &["diff", "--cached", "--name-only"]), "");

    let paths = vec!["a.txt".to_string(), "b.txt".to_string()];
    bulk_stage_impl(dir.path_str(), &paths).unwrap();
    let staged = git(&dir.path, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.contains("a.txt") && staged.contains("b.txt"),
        "{staged}"
    );

    bulk_unstage_impl(dir.path_str(), &paths).unwrap();
    assert_eq!(git(&dir.path, &["diff", "--cached", "--name-only"]), "");
}

#[test]
fn discard_impl_restores_tracked_and_removes_untracked() {
    let dir = TestDir::new("discard");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "a\n", "base");
    std::fs::write(dir.path.join("a.txt"), "a\nchanged\n").unwrap();

    discard_impl(dir.path_str(), "a.txt").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path.join("a.txt")).unwrap(),
        "a\n"
    );

    std::fs::write(dir.path.join("b.txt"), "b\n").unwrap();
    bulk_discard_impl(dir.path_str(), &["b.txt".to_string()]).unwrap();
    assert!(!dir.path.join("b.txt").exists());
}

#[test]
fn commit_impl_returns_the_domain_outcome() {
    let dir = TestDir::new("commit");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "a\n", "base");
    std::fs::write(dir.path.join("a.txt"), "a\nchanged\n").unwrap();
    stage_impl(dir.path_str(), "a.txt").unwrap();

    let outcome = commit_impl(dir.path_str(), "second").unwrap();
    assert!(outcome.success);
    assert_eq!(outcome.error, None);
    assert_eq!(git(&dir.path, &["log", "-1", "--format=%s"]), "second");

    let error = commit_impl(dir.path_str(), "   ").unwrap_err();
    assert!(error.to_string().contains("Commit message is required"));
}

#[test]
fn upstream_status_and_conflict_operation_report_clean_states() {
    let dir = TestDir::new("upstream-conflict");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "a\n", "base");

    let status = upstream_status_impl(dir.path_str()).unwrap();
    assert!(!status.has_upstream);
    assert_eq!(status.ahead, 0);
    assert_eq!(status.behind, 0);

    assert_eq!(
        conflict_operation_impl(dir.path_str()).unwrap(),
        GitConflictOperation::Unknown
    );
}

#[test]
fn compare_impls_are_thin_wrappers() {
    let dir = TestDir::new("compare");
    init_repo(&dir);
    let first = commit_file(&dir, "a.txt", "one\n", "first");
    let second = commit_file(&dir, "a.txt", "one\ntwo\n", "second");

    let branch = branch_compare_impl(dir.path_str(), "main").unwrap();
    assert_eq!(branch.summary.status, "ready");
    assert_eq!(branch.summary.head_oid.as_deref(), Some(second.as_str()));

    let commit = commit_compare_impl(dir.path_str(), &second).unwrap();
    assert_eq!(commit.summary.status, "ready");
    assert_eq!(commit.summary.parent_oid.as_deref(), Some(first.as_str()));
    assert_eq!(commit.entries.len(), 1);
}

/// The git command surface refuses worktree paths outside every authorized fs
/// root, so a renderer cannot point git at an arbitrary repository.
#[test]
fn worktree_authorization_guard_requires_an_authorized_root() {
    let dir = TestDir::new("auth-guard");
    let fs = FsService::new();
    fs.authorize_root(dir.path_str()).expect("authorize root");

    assert!(require_authorized_worktree(&fs, dir.path_str()).is_ok());
    let child = dir.path.join("nested");
    std::fs::create_dir_all(&child).expect("create nested dir");
    assert!(require_authorized_worktree(&fs, child.to_str().expect("utf-8")).is_ok());

    let sibling = format!("{}-sibling", dir.path_str());
    let error = require_authorized_worktree(&fs, &sibling).unwrap_err();
    assert!(
        error.to_string().contains("Access denied"),
        "unexpected error: {error}"
    );
    assert!(require_authorized_worktree(&fs, "/etc").is_err());
}

#[test]
fn remote_urls_returns_fetch_urls_deduped() {
    let dir = TestDir::new("remote-urls");
    init_repo(&dir);
    git(
        &dir.path,
        &["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    git(
        &dir.path,
        &[
            "remote",
            "add",
            "upstream",
            "https://github.com/up/repo.git",
        ],
    );

    let urls = remote_urls_impl(dir.path_str()).unwrap();
    assert_eq!(urls.len(), 2);
    assert_eq!(urls[0].name, "origin");
    assert_eq!(urls[0].url, "git@github.com:owner/repo.git");
    assert_eq!(urls[1].name, "upstream");
    assert_eq!(urls[1].url, "https://github.com/up/repo.git");
}

#[test]
fn remote_urls_is_empty_without_remotes() {
    let dir = TestDir::new("remote-urls-empty");
    init_repo(&dir);
    assert!(remote_urls_impl(dir.path_str()).unwrap().is_empty());
}

#[test]
fn git_read_whitelist_accepts_only_read_forms() {
    let ok = |args: &[&str]| {
        is_allowed_git_read_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    assert!(ok(&["rev-parse", "--abbrev-ref", "HEAD"]));
    assert!(ok(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]));
    assert!(ok(&["show-ref", "--verify", "--quiet", "refs/heads/main"]));
    assert!(ok(&["check-ref-format", "--branch", "feature/x"]));
    assert!(ok(&["config", "--get", "branch.main.remote"]));
    assert!(ok(&["config", "--get-all", "remote.origin.fetch"]));
    assert!(ok(&["config", "--get-regexp", "^branch\\."]));
    assert!(ok(&["config", "--list"]));
    // 写形式一律拒绝
    assert!(!ok(&["config", "user.name", "x"]));
    assert!(!ok(&["config", "--unset", "branch.main.remote"]));
    assert!(!ok(&["config", "--add", "remote.origin.fetch", "+refs/x"]));
    assert!(!ok(&["config", "--replace-all", "a", "b"]));
    assert!(!ok(&["config", "--edit"]));
    assert!(!ok(&["config", "--rename-section", "a", "b"]));
    assert!(!ok(&["config", "--remove-section", "a"]));
    // 非白名单子命令
    assert!(!ok(&["fetch", "--prune"]));
    assert!(!ok(&["status", "--porcelain"]));
    assert!(!ok(&["push", "origin", "HEAD"]));
    assert!(!ok(&[]));
}

#[test]
fn git_read_whitelist_restricts_symbolic_ref_to_read_forms() {
    let ok = |args: &[&str]| {
        is_allowed_git_read_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    assert!(ok(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]));
    assert!(ok(&["symbolic-ref", "--quiet", "--short", "HEAD"]));
    // 两个位置参数是写形式；`--delete`/`-d` 删除符号引用。
    assert!(!ok(&["symbolic-ref", "HEAD", "refs/heads/x"]));
    assert!(!ok(&["symbolic-ref", "--delete", "refs/x"]));
}

#[test]
fn git_read_runs_real_reads_and_passes_through_nonzero() {
    let dir = TestDir::new("git-read");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "one\n", "init");
    let path = dir.path_str();

    let args: Vec<String> = ["rev-parse", "--abbrev-ref", "HEAD"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let branch = git_read_impl(path, &args).unwrap();
    assert_eq!(branch.code, Some(0));
    assert!(!branch.stdout.trim().is_empty());

    let args: Vec<String> = ["show-ref", "--verify", "--quiet", "refs/heads/nope"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let missing = git_read_impl(path, &args).unwrap();
    assert_ne!(missing.code, Some(0));
    assert_eq!(missing.stdout, "");
}

#[test]
fn history_impl_limits_items_and_reports_more() {
    let dir = TestDir::new("history");
    init_repo(&dir);
    commit_file(&dir, "a.txt", "one\n", "first");
    commit_file(&dir, "a.txt", "one\ntwo\n", "second");

    let result = history_impl(dir.path_str(), Some(1), None).unwrap();

    assert_eq!(result.items.len(), 1);
    assert!(result.has_more);
    assert_eq!(
        result.current_ref.as_ref().map(|entry| entry.name.as_str()),
        Some("main")
    );
}
