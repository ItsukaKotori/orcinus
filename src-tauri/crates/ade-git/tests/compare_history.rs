use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ade_git::compare::{
    branch_compare, branch_diff, commit_compare, commit_diff, GitBranchChangeStatus,
};
use ade_git::diff::GitDiffResult;
use ade_git::history::{
    history, GitHistoryRefCategory, GIT_HISTORY_DEFAULT_LIMIT, GIT_HISTORY_MAX_LIMIT,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ade-git-{name}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// git with the user's environment stripped, so host config cannot leak into fixtures.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ade Test")
        .env("GIT_AUTHOR_EMAIL", "ade-test@example.com")
        .env("GIT_COMMITTER_NAME", "Ade Test")
        .env("GIT_COMMITTER_EMAIL", "ade-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let output = git_command(dir).args(args).output().expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            message,
        ],
    );
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    String::from_utf8(git(dir, &["rev-parse", rev]).stdout)
        .expect("oid is UTF-8")
        .trim()
        .to_string()
}

fn repo_str(repo: &Path) -> &str {
    repo.to_str().expect("repo path is UTF-8")
}

/// `main` branch with one commit containing `README.md` (`line1\n`).
fn init_repo_with_commit(dir: &TempDir) -> PathBuf {
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(repo.join("README.md"), "line1\n").expect("write README");
    commit_all(&repo, "init");
    repo
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
fn branch_compare_lists_changed_files_and_summary() {
    let dir = TempDir::new("compare-branch");
    let repo = init_repo_with_commit(&dir);
    let a = rev_parse(&repo, "HEAD");
    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    std::fs::write(repo.join("README.md"), "line1\nline2\n").unwrap();
    std::fs::write(repo.join("file.txt"), "new\n").unwrap();
    commit_all(&repo, "feature work");
    let b = rev_parse(&repo, "HEAD");

    let result = branch_compare(repo_str(&repo), "main").unwrap();

    assert_eq!(result.summary.status, "ready");
    assert_eq!(result.summary.base_ref, "main");
    assert_eq!(result.summary.base_oid.as_deref(), Some(a.as_str()));
    assert_eq!(result.summary.compare_ref, "feature");
    assert_eq!(result.summary.head_oid.as_deref(), Some(b.as_str()));
    assert_eq!(result.summary.merge_base.as_deref(), Some(a.as_str()));
    assert_eq!(result.summary.changed_files, 2);
    assert_eq!(result.summary.commits_ahead, Some(1));
    assert_eq!(result.summary.commits_behind, Some(0));
    assert_eq!(result.summary.error_message, None);
    assert_eq!(result.entries.len(), 2);

    let readme = result
        .entries
        .iter()
        .find(|entry| entry.path == "README.md")
        .unwrap();
    assert_eq!(readme.status, GitBranchChangeStatus::Modified);
    assert_eq!(readme.old_path, None);
    assert_eq!(readme.added, Some(1));
    assert_eq!(readme.removed, Some(0));

    let file = result
        .entries
        .iter()
        .find(|entry| entry.path == "file.txt")
        .unwrap();
    assert_eq!(file.status, GitBranchChangeStatus::Added);
    assert_eq!(file.added, Some(1));
    assert_eq!(file.removed, Some(0));
}

#[test]
fn branch_compare_invalid_base_reports_status() {
    let dir = TempDir::new("compare-invalid-base");
    let repo = init_repo_with_commit(&dir);
    let head = rev_parse(&repo, "HEAD");

    let result = branch_compare(repo_str(&repo), "does-not-exist").unwrap();

    assert_eq!(result.summary.status, "invalid-base");
    assert_eq!(result.summary.base_ref, "does-not-exist");
    assert_eq!(result.summary.base_oid, None);
    assert_eq!(result.summary.head_oid.as_deref(), Some(head.as_str()));
    assert_eq!(result.summary.merge_base, None);
    assert_eq!(result.summary.changed_files, 0);
    assert_eq!(
        result.summary.error_message.as_deref(),
        Some("Base ref does-not-exist could not be resolved in this repository.")
    );
    assert!(result.entries.is_empty());
}

#[test]
fn branch_compare_unborn_head_reports_status() {
    let dir = TempDir::new("compare-unborn");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let result = branch_compare(repo_str(&repo), "main").unwrap();

    assert_eq!(result.summary.status, "unborn-head");
    assert_eq!(result.summary.base_oid, None);
    assert_eq!(result.summary.head_oid, None);
    assert_eq!(
        result.summary.error_message.as_deref(),
        Some("This branch does not have a committed HEAD yet, so compare-to-base is unavailable.")
    );
    assert!(result.entries.is_empty());
}

#[test]
fn branch_compare_ready_with_empty_entries_when_base_resolves_but_head_is_unborn() {
    let dir = TempDir::new("compare-unborn-head");
    let repo = init_repo_with_commit(&dir);
    let base = rev_parse(&repo, "main");
    git(&repo, &["checkout", "--quiet", "--orphan", "fresh"]);

    let result = branch_compare(repo_str(&repo), "main").unwrap();

    assert_eq!(result.summary.status, "ready");
    assert_eq!(result.summary.compare_ref, "fresh");
    assert_eq!(result.summary.base_oid.as_deref(), Some(base.as_str()));
    assert_eq!(result.summary.head_oid, None);
    assert_eq!(result.summary.merge_base, None);
    assert_eq!(result.summary.changed_files, 0);
    assert_eq!(result.summary.commits_ahead, Some(0));
    assert_eq!(result.summary.commits_behind, Some(0));
    assert_eq!(result.summary.error_message, None);
    assert!(result.entries.is_empty());
}

#[test]
fn branch_compare_no_merge_base_reports_status() {
    let dir = TempDir::new("compare-no-merge-base");
    let repo = init_repo_with_commit(&dir);
    let head = rev_parse(&repo, "main");
    git(&repo, &["checkout", "--quiet", "--orphan", "other"]);
    std::fs::write(repo.join("other.txt"), "other\n").unwrap();
    commit_all(&repo, "orphan root");
    let orphan = rev_parse(&repo, "HEAD");
    git(&repo, &["checkout", "--quiet", "main"]);

    let result = branch_compare(repo_str(&repo), "other").unwrap();

    assert_eq!(result.summary.status, "no-merge-base");
    assert_eq!(result.summary.base_oid.as_deref(), Some(orphan.as_str()));
    assert_eq!(result.summary.head_oid.as_deref(), Some(head.as_str()));
    assert_eq!(result.summary.merge_base, None);
    assert_eq!(
        result.summary.error_message.as_deref(),
        Some("This branch and other do not share a merge base, so compare-to-base is unavailable.")
    );
    assert!(result.entries.is_empty());
}

#[test]
fn branch_compare_reports_rename_old_path_and_stats() {
    let dir = TempDir::new("compare-rename");
    let repo = init_repo_with_commit(&dir);
    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    git(&repo, &["mv", "README.md", "docs.md"]);
    commit_all(&repo, "rename");

    let result = branch_compare(repo_str(&repo), "main").unwrap();

    assert_eq!(result.summary.status, "ready");
    assert_eq!(result.summary.changed_files, 1);
    let entry = &result.entries[0];
    assert_eq!(entry.path, "docs.md");
    assert_eq!(entry.status, GitBranchChangeStatus::Renamed);
    assert_eq!(entry.old_path.as_deref(), Some("README.md"));
    assert_eq!(entry.added, Some(0));
    assert_eq!(entry.removed, Some(0));
}

#[test]
fn commit_compare_lists_single_commit_files() {
    let dir = TempDir::new("compare-commit");
    let repo = init_repo_with_commit(&dir);
    let a = rev_parse(&repo, "HEAD");
    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    std::fs::write(repo.join("README.md"), "line1\nline2\n").unwrap();
    std::fs::write(repo.join("file.txt"), "new\n").unwrap();
    commit_all(&repo, "feature work");
    let b = rev_parse(&repo, "HEAD");

    let result = commit_compare(repo_str(&repo), &b).unwrap();

    assert_eq!(result.summary.status, "ready");
    assert_eq!(result.summary.commit_oid, b);
    assert_eq!(result.summary.parent_oid.as_deref(), Some(a.as_str()));
    assert_eq!(result.summary.compare_ref, b[..7]);
    assert_eq!(result.summary.base_ref, a[..7]);
    assert_eq!(result.summary.changed_files, 2);
    assert_eq!(result.summary.error_message, None);
    assert_eq!(result.entries.len(), 2);

    let readme = result
        .entries
        .iter()
        .find(|entry| entry.path == "README.md")
        .unwrap();
    assert_eq!(readme.status, GitBranchChangeStatus::Modified);
    assert_eq!(readme.added, Some(1));
    assert_eq!(readme.removed, Some(0));
    let file = result
        .entries
        .iter()
        .find(|entry| entry.path == "file.txt")
        .unwrap();
    assert_eq!(file.status, GitBranchChangeStatus::Added);
}

#[test]
fn commit_compare_root_commit_uses_the_empty_tree_side() {
    let dir = TempDir::new("compare-root");
    let repo = init_repo_with_commit(&dir);
    let root = rev_parse(&repo, "HEAD");

    let result = commit_compare(repo_str(&repo), &root).unwrap();

    assert_eq!(result.summary.status, "ready");
    assert_eq!(result.summary.commit_oid, root);
    assert_eq!(result.summary.parent_oid, None);
    assert_eq!(result.summary.base_ref, "empty tree");
    assert_eq!(result.summary.changed_files, 1);
    let entry = &result.entries[0];
    assert_eq!(entry.path, "README.md");
    assert_eq!(entry.status, GitBranchChangeStatus::Added);
    assert_eq!(entry.added, Some(1));
    assert_eq!(entry.removed, Some(0));

    let (original, modified) =
        text_diff(commit_diff(repo_str(&repo), &root, None, "README.md", None).unwrap());
    assert_eq!(original, "");
    assert_eq!(modified, "line1\n");
}

#[test]
fn commit_compare_invalid_commit_reports_status() {
    let dir = TempDir::new("compare-invalid-commit");
    let repo = init_repo_with_commit(&dir);

    let result = commit_compare(repo_str(&repo), "does-not-exist").unwrap();

    assert_eq!(result.summary.status, "invalid-commit");
    assert_eq!(result.summary.commit_oid, "");
    assert_eq!(result.summary.parent_oid, None);
    assert_eq!(result.summary.compare_ref, "does-not-exist");
    assert_eq!(result.summary.base_ref, "parent");
    assert_eq!(result.summary.changed_files, 0);
    assert_eq!(
        result.summary.error_message.as_deref(),
        Some("Commit does-not-exist could not be resolved in this repository.")
    );
    assert!(result.entries.is_empty());
}

#[test]
fn branch_diff_and_commit_diff_return_whole_file_contents() {
    let dir = TempDir::new("compare-diff");
    let repo = init_repo_with_commit(&dir);
    let a = rev_parse(&repo, "HEAD");
    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    std::fs::write(repo.join("README.md"), "line1\nline2\n").unwrap();
    commit_all(&repo, "feature work");
    let b = rev_parse(&repo, "HEAD");

    let (original, modified) =
        text_diff(branch_diff(repo_str(&repo), &a, &b, "README.md", None).unwrap());
    assert_eq!(original, "line1\n");
    assert_eq!(modified, "line1\nline2\n");

    let (original, modified) =
        text_diff(commit_diff(repo_str(&repo), &b, Some(&a), "README.md", None).unwrap());
    assert_eq!(original, "line1\n");
    assert_eq!(modified, "line1\nline2\n");
}

#[test]
fn branch_diff_reads_the_rename_preimage_via_old_path() {
    let dir = TempDir::new("compare-diff-rename");
    let repo = init_repo_with_commit(&dir);
    let a = rev_parse(&repo, "HEAD");
    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    git(&repo, &["mv", "README.md", "docs.md"]);
    commit_all(&repo, "rename");
    let b = rev_parse(&repo, "HEAD");

    let (original, modified) =
        text_diff(branch_diff(repo_str(&repo), &a, &b, "docs.md", Some("README.md")).unwrap());
    assert_eq!(original, "line1\n");
    assert_eq!(modified, "line1\n");
}

#[test]
fn history_returns_items_with_refs_and_limit() {
    let dir = TempDir::new("history-limit");
    let repo = init_repo_with_commit(&dir);
    for index in 2..=5 {
        std::fs::write(repo.join(format!("f{index}.txt")), format!("{index}\n")).unwrap();
        commit_all(&repo, &format!("commit {index}"));
    }
    let head = rev_parse(&repo, "HEAD");
    let parent = rev_parse(&repo, "HEAD~1");

    let result = history(repo_str(&repo), Some(2), None).unwrap();

    assert_eq!(result.limit, 2);
    assert_eq!(result.items.len(), 2);
    assert!(result.has_more);
    assert_eq!(result.items[0].id, head);
    assert_eq!(result.items[0].parent_ids, vec![parent]);
    assert_eq!(result.items[0].subject, "commit 5");
    assert_eq!(result.items[0].message, "commit 5");
    assert_eq!(result.items[0].display_id.as_deref(), Some(&head[..7]));
    assert_eq!(result.items[0].author.as_deref(), Some("Ade Test"));
    assert_eq!(
        result.items[0].author_email.as_deref(),
        Some("ade-test@example.com")
    );
    assert!(result.items[0].timestamp.unwrap() > 0);
    assert_eq!(result.items[0].statistics, None);

    let current = result.current_ref.as_ref().expect("current ref");
    assert_eq!(current.id, "refs/heads/main");
    assert_eq!(current.name, "main");
    assert_eq!(current.revision.as_deref(), Some(head.as_str()));
    assert_eq!(current.category, Some(GitHistoryRefCategory::Branches));
    assert!(result.remote_ref.is_none());
    assert!(result.base_ref.is_none());
    assert!(result.merge_base.is_none());
    assert!(!result.has_incoming_changes);
    assert!(!result.has_outgoing_changes);

    let references = result.items[0].references.as_ref().expect("references");
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].id, "refs/heads/main");
    assert_eq!(references[0].name, "main");
    assert_eq!(references[0].category, Some(GitHistoryRefCategory::Branches));
}

#[test]
fn history_clamps_limits_and_reports_more() {
    let dir = TempDir::new("history-clamp");
    let repo = init_repo_with_commit(&dir);
    for index in 2..=5 {
        std::fs::write(repo.join(format!("f{index}.txt")), format!("{index}\n")).unwrap();
        commit_all(&repo, &format!("commit {index}"));
    }

    let smallest = history(repo_str(&repo), Some(0), None).unwrap();
    assert_eq!(smallest.limit, 1);
    assert_eq!(smallest.items.len(), 1);
    assert!(smallest.has_more);

    let largest = history(repo_str(&repo), Some(999), None).unwrap();
    assert_eq!(largest.limit, GIT_HISTORY_MAX_LIMIT);
    assert_eq!(largest.items.len(), 5);
    assert!(!largest.has_more);

    let default = history(repo_str(&repo), None, None).unwrap();
    assert_eq!(default.limit, GIT_HISTORY_DEFAULT_LIMIT);
    assert_eq!(default.items.len(), 5);
}

#[test]
fn history_returns_empty_result_for_unborn_repository() {
    let dir = TempDir::new("history-empty");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let result = history(repo_str(&repo), None, None).unwrap();

    assert!(result.items.is_empty());
    assert_eq!(result.limit, GIT_HISTORY_DEFAULT_LIMIT);
    assert!(result.current_ref.is_none());
    assert!(result.remote_ref.is_none());
    assert!(result.base_ref.is_none());
    assert!(result.merge_base.is_none());
    assert!(!result.has_incoming_changes);
    assert!(!result.has_outgoing_changes);
    assert!(!result.has_more);
}

#[test]
fn history_reports_upstream_merge_base_and_change_flags() {
    let dir = TempDir::new("history-upstream");
    let source = dir.path().join("source");
    std::fs::create_dir_all(&source).expect("create source dir");
    git(&source, &["init", "--quiet"]);
    git(&source, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(source.join("README.md"), "base\n").unwrap();
    commit_all(&source, "base");
    let base_oid = rev_parse(&source, "HEAD");

    let origin = dir.path().join("origin.git");
    git(
        dir.path(),
        &[
            "clone",
            "--bare",
            "--quiet",
            repo_str(&source),
            repo_str(&origin),
        ],
    );
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        &["clone", "--quiet", repo_str(&origin), repo_str(&repo)],
    );

    std::fs::write(repo.join("local.txt"), "local\n").unwrap();
    commit_all(&repo, "local work");
    let local_oid = rev_parse(&repo, "HEAD");

    let result = history(repo_str(&repo), None, None).unwrap();
    let current = result.current_ref.as_ref().expect("current ref");
    assert_eq!(current.revision.as_deref(), Some(local_oid.as_str()));
    let remote = result.remote_ref.as_ref().expect("remote ref");
    assert_eq!(remote.id, "refs/remotes/origin/main");
    assert_eq!(remote.name, "origin/main");
    assert_eq!(remote.category, Some(GitHistoryRefCategory::RemoteBranches));
    assert_eq!(remote.revision.as_deref(), Some(base_oid.as_str()));
    assert_eq!(result.merge_base.as_deref(), Some(base_oid.as_str()));
    assert!(!result.has_incoming_changes);
    assert!(result.has_outgoing_changes);

    git(&source, &["remote", "add", "origin", repo_str(&origin)]);
    std::fs::write(source.join("remote.txt"), "remote\n").unwrap();
    commit_all(&source, "remote work");
    let remote_oid = rev_parse(&source, "HEAD");
    git(&source, &["push", "--quiet", "origin", "main"]);
    git(&repo, &["fetch", "--quiet", "origin"]);

    let result = history(repo_str(&repo), None, None).unwrap();
    assert_eq!(
        result.remote_ref.as_ref().unwrap().revision.as_deref(),
        Some(remote_oid.as_str())
    );
    assert_eq!(result.merge_base.as_deref(), Some(base_oid.as_str()));
    assert!(result.has_incoming_changes);
    assert!(result.has_outgoing_changes);

    git(&repo, &["branch", "base", &base_oid]);
    let with_base = history(repo_str(&repo), None, Some("base")).unwrap();
    let base_ref = with_base.base_ref.as_ref().expect("base ref");
    assert_eq!(base_ref.id, "refs/heads/base");
    assert_eq!(base_ref.name, "base");
    assert_eq!(base_ref.revision.as_deref(), Some(base_oid.as_str()));
    assert_eq!(base_ref.category, Some(GitHistoryRefCategory::Branches));

    assert!(history(repo_str(&repo), None, Some("origin/main"))
        .unwrap()
        .base_ref
        .is_none());
    assert!(history(repo_str(&repo), None, Some("main"))
        .unwrap()
        .base_ref
        .is_none());
}

#[test]
fn compare_and_history_wire_shapes_match_the_contract() {
    let dir = TempDir::new("wire-shapes");
    let repo = init_repo_with_commit(&dir);
    let a = rev_parse(&repo, "HEAD");
    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    git(&repo, &["mv", "README.md", "docs.md"]);
    commit_all(&repo, "rename");
    let b = rev_parse(&repo, "HEAD");

    let result = branch_compare(repo_str(&repo), "main").unwrap();
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["summary"]["baseRef"], "main");
    assert_eq!(json["summary"]["baseOid"], a);
    assert_eq!(json["summary"]["compareRef"], "feature");
    assert_eq!(json["summary"]["headOid"], b);
    assert_eq!(json["summary"]["mergeBase"], a);
    assert_eq!(json["summary"]["changedFiles"], 1);
    assert_eq!(json["summary"]["commitsAhead"], 1);
    assert_eq!(json["summary"]["commitsBehind"], 0);
    assert_eq!(json["summary"]["status"], "ready");
    assert!(json["summary"].get("errorMessage").is_none());
    assert_eq!(json["entries"][0]["path"], "docs.md");
    assert_eq!(json["entries"][0]["status"], "renamed");
    assert_eq!(json["entries"][0]["oldPath"], "README.md");
    assert_eq!(json["entries"][0]["added"], 0);
    assert_eq!(json["entries"][0]["removed"], 0);

    let invalid = branch_compare(repo_str(&repo), "does-not-exist").unwrap();
    let json = serde_json::to_value(&invalid).unwrap();
    assert!(json["summary"]["baseOid"].is_null());
    assert!(json["summary"]["mergeBase"].is_null());

    let root = commit_compare(repo_str(&repo), &a).unwrap();
    let json = serde_json::to_value(&root).unwrap();
    assert_eq!(json["summary"]["commitOid"], a);
    assert!(json["summary"]["parentOid"].is_null());
    assert_eq!(json["summary"]["baseRef"], "empty tree");

    let history = history(repo_str(&repo), Some(1), None).unwrap();
    let json = serde_json::to_value(&history).unwrap();
    assert_eq!(json["limit"], 1);
    assert_eq!(json["hasMore"], true);
    assert_eq!(json["hasIncomingChanges"], false);
    assert_eq!(json["hasOutgoingChanges"], false);
    assert_eq!(json["currentRef"]["id"], "refs/heads/feature");
    assert_eq!(json["items"][0]["id"], b);
    assert_eq!(json["items"][0]["displayId"], b[..7]);
    assert_eq!(json["items"][0]["parentIds"][0], a);
    assert!(json["items"][0]["authorEmail"].is_string());
    assert!(json["items"][0].get("statistics").is_none());
    assert_eq!(json["items"][0]["references"][0]["category"], "branches");
}
