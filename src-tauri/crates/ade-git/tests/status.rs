use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ade_git::status::{
    GitConflictKind, GitConflictOperation, GitConflictResolutionStatus, GitFileStatus,
    GitStagingArea,
};
use ade_git::status_read::{conflict_operation, status, StatusOptions};

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

    fn path_str(&self) -> &str {
        self.path.to_str().expect("temp dir path is UTF-8")
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

/// `main` and `feature` both modify `conflict.txt`, then the merge is left conflicted (UU).
fn init_conflicted_repo(dir: &TempDir) -> PathBuf {
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(repo.join("conflict.txt"), "base\n").expect("write base");
    commit_all(&repo, "base");

    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    std::fs::write(repo.join("conflict.txt"), "feature\n").expect("write feature");
    commit_all(&repo, "feature");

    git(&repo, &["checkout", "--quiet", "main"]);
    std::fs::write(repo.join("conflict.txt"), "main\n").expect("write main");
    commit_all(&repo, "main");

    let merge = git_command(&repo)
        .args([
            "-c",
            "commit.gpgsign=false",
            "merge",
            "--no-edit",
            "feature",
        ])
        .output()
        .expect("run merge");
    assert!(
        !merge.status.success(),
        "merge should stop on a conflict: {}",
        String::from_utf8_lossy(&merge.stderr)
    );
    repo
}

#[test]
fn status_reports_branch_staged_unstaged_and_untracked() {
    let dir = TempDir::new("status-basic");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "changed\n").unwrap();
    std::fs::write(repo.join("new.txt"), "new\n").unwrap();

    let result = status(repo.to_str().unwrap(), &StatusOptions::default(), None).unwrap();

    assert_eq!(result.branch.as_deref(), Some("refs/heads/main"));
    assert_eq!(result.conflict_operation, GitConflictOperation::Unknown);
    let paths: Vec<&str> = result.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"README.md"));
    assert!(paths.contains(&"new.txt"));
    let readme = result
        .entries
        .iter()
        .find(|e| e.path == "README.md")
        .unwrap();
    assert_eq!(readme.area, GitStagingArea::Unstaged);
    assert_eq!(readme.status, GitFileStatus::Modified);
    let new = result.entries.iter().find(|e| e.path == "new.txt").unwrap();
    assert_eq!(new.area, GitStagingArea::Untracked);
    assert_eq!(new.status, GitFileStatus::Untracked);
    assert!(!result.upstream_status.unwrap().has_upstream);
}

#[test]
fn status_limit_truncates_and_flags() {
    let dir = TempDir::new("status-limit");
    let repo = init_repo_with_commit(&dir);
    for index in 0..5 {
        std::fs::write(repo.join(format!("f{index}.txt")), "x").unwrap();
    }

    let options = StatusOptions {
        limit: Some(2),
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    assert_eq!(result.entries.len(), 2);
    assert_eq!(result.did_hit_limit, Some(true));
    assert_eq!(result.status_length, Some(5));
}

#[test]
fn status_on_non_repo_is_unsigned_empty_and_conflict_unknown() {
    let dir = TempDir::new("status-nonrepo");

    let result = status(dir.path_str(), &StatusOptions::default(), None).unwrap();

    assert!(result.entries.is_empty());
    assert!(result.branch.is_none());
    assert!(result.head.is_none());
    assert!(result.upstream_status.is_none());
    assert_eq!(result.conflict_operation, GitConflictOperation::Unknown);
}

#[test]
fn unmerged_entry_maps_conflict_kind_and_compatibility_status() {
    let dir = TempDir::new("status-conflict");
    let repo = init_conflicted_repo(&dir);

    let result = status(repo.to_str().unwrap(), &StatusOptions::default(), None).unwrap();

    let entry = result
        .entries
        .iter()
        .find(|e| e.path == "conflict.txt")
        .unwrap();
    assert_eq!(entry.conflict_kind, Some(GitConflictKind::BothModified));
    assert_eq!(
        entry.conflict_status,
        Some(GitConflictResolutionStatus::Unresolved)
    );
    assert_eq!(entry.status, GitFileStatus::Modified);
    assert_eq!(entry.area, GitStagingArea::Unstaged);
    assert_eq!(result.conflict_operation, GitConflictOperation::Merge);
}

#[test]
fn line_stats_attach_numbers_for_modified_files() {
    let dir = TempDir::new("status-stats");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "line1\nline2\nline3\n").unwrap();

    let options = StatusOptions {
        include_line_stats: true,
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    let entry = result
        .entries
        .iter()
        .find(|e| e.path == "README.md")
        .unwrap();
    assert_eq!(entry.added, Some(2));
    assert_eq!(entry.removed, Some(0));
}

#[test]
fn line_stats_use_cached_diff_for_staged_entries() {
    let dir = TempDir::new("status-stats-staged");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "line1\nline2\n").unwrap();
    git(&repo, &["add", "README.md"]);

    let options = StatusOptions {
        include_line_stats: true,
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    let entry = result
        .entries
        .iter()
        .find(|e| e.path == "README.md" && e.area == GitStagingArea::Staged)
        .unwrap();
    assert_eq!(entry.added, Some(1));
    assert_eq!(entry.removed, Some(0));
}

#[test]
fn line_stats_count_untracked_files_as_additions() {
    let dir = TempDir::new("status-stats-untracked");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("new.txt"), "a\nb\n").unwrap();
    std::fs::write(repo.join("tail.txt"), "no trailing newline").unwrap();

    let options = StatusOptions {
        include_line_stats: true,
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    let new = result.entries.iter().find(|e| e.path == "new.txt").unwrap();
    assert_eq!(new.added, Some(2));
    assert_eq!(new.removed, None);
    let tail = result
        .entries
        .iter()
        .find(|e| e.path == "tail.txt")
        .unwrap();
    assert_eq!(tail.added, Some(1));
}

#[test]
fn line_stats_skip_binary_and_oversized_untracked_files() {
    let dir = TempDir::new("status-stats-binary");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
    std::fs::write(repo.join("big.txt"), vec![b'a'; 2 * 1024 * 1024 + 1]).unwrap();

    let options = StatusOptions {
        include_line_stats: true,
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    let binary = result.entries.iter().find(|e| e.path == "bin.dat").unwrap();
    assert_eq!(binary.added, None);
    assert_eq!(binary.removed, None);
    let big = result.entries.iter().find(|e| e.path == "big.txt").unwrap();
    assert_eq!(big.added, None);
    assert_eq!(big.removed, None);
}

#[test]
fn status_limit_keeps_entries_in_git_output_order() {
    let dir = TempDir::new("status-order");
    let repo = init_repo_with_commit(&dir);
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(repo.join(name), "x\n").unwrap();
    }

    let options = StatusOptions {
        limit: Some(1),
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].path, "a.txt");
    assert_eq!(result.entries[0].area, GitStagingArea::Untracked);
    assert_eq!(result.did_hit_limit, Some(true));
    assert_eq!(result.status_length, Some(3));
}

#[test]
fn status_limit_zero_disables_the_cap() {
    let dir = TempDir::new("status-no-limit");
    let repo = init_repo_with_commit(&dir);
    for index in 0..3 {
        std::fs::write(repo.join(format!("f{index}.txt")), "x").unwrap();
    }

    let options = StatusOptions {
        limit: Some(0),
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    assert_eq!(result.entries.len(), 3);
    assert_eq!(result.did_hit_limit, None);
    assert_eq!(result.status_length, None);
}

#[test]
fn ignored_paths_are_reported_only_when_requested() {
    let dir = TempDir::new("status-ignored");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(repo.join("ignored.txt"), "x\n").unwrap();

    let without = status(repo.to_str().unwrap(), &StatusOptions::default(), None).unwrap();
    assert!(without.ignored_paths.is_none());

    let options = StatusOptions {
        include_ignored: true,
        ..StatusOptions::default()
    };
    let with = status(repo.to_str().unwrap(), &options, None).unwrap();
    assert_eq!(
        with.ignored_paths,
        Some(vec!["ignored.txt".to_string()]),
        "`--ignored=matching` should report the matching ignored path"
    );
}

#[test]
fn branch_line_total_sums_against_the_merge_base() {
    let dir = TempDir::new("status-branch-total");
    let repo = init_repo_with_commit(&dir);
    let base = String::from_utf8(git(&repo, &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    std::fs::write(repo.join("README.md"), "line1\nline2\nline3\n").unwrap();

    let options = StatusOptions {
        branch_line_total_merge_base: Some(base.clone()),
        ..StatusOptions::default()
    };
    let result = status(repo.to_str().unwrap(), &options, None).unwrap();

    let total = result.branch_line_total.expect("branch line total");
    assert_eq!(total.added, 2);
    assert_eq!(total.removed, 0);
    assert_eq!(total.merge_base, base);
    assert_eq!(total.test, None);
    assert_eq!(total.generated, None);
}

#[test]
fn detached_head_reports_head_without_branch() {
    let dir = TempDir::new("status-detached");
    let repo = init_repo_with_commit(&dir);
    git(&repo, &["checkout", "--quiet", "--detach"]);

    let result = status(repo.to_str().unwrap(), &StatusOptions::default(), None).unwrap();

    assert!(result.branch.is_none());
    assert!(result.head.is_some());
}

#[test]
fn conflict_operation_reads_linked_worktree_gitdir_file() {
    let dir = TempDir::new("status-linked-conflict");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "left\n").unwrap();
    commit_all(&repo, "left");
    git(&repo, &["checkout", "--quiet", "-b", "feature"]);
    std::fs::write(repo.join("README.md"), "right\n").unwrap();
    commit_all(&repo, "right");
    let feature = String::from_utf8(git(&repo, &["rev-parse", "feature"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    git(&repo, &["checkout", "--quiet", "main"]);
    std::fs::write(repo.join("README.md"), "main-side\n").unwrap();
    commit_all(&repo, "main-side");

    let linked = dir.path().join("wt");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    let cherry_pick = git_command(&linked)
        .args(["-c", "commit.gpgsign=false", "cherry-pick", &feature])
        .output()
        .expect("run cherry-pick");
    assert!(
        !cherry_pick.status.success(),
        "cherry-pick should stop on a conflict: {}",
        String::from_utf8_lossy(&cherry_pick.stderr)
    );

    assert_eq!(
        conflict_operation(linked.to_str().unwrap()).unwrap(),
        GitConflictOperation::CherryPick,
        "the linked worktree's .git file must resolve to its per-worktree metadata dir"
    );
    assert_eq!(
        conflict_operation(repo.to_str().unwrap()).unwrap(),
        GitConflictOperation::Unknown
    );
}

#[test]
fn branch_line_total_is_omitted_without_a_merge_base() {
    let dir = TempDir::new("status-branch-total-absent");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "changed\n").unwrap();

    let result = status(repo.to_str().unwrap(), &StatusOptions::default(), None).unwrap();

    assert!(result.branch_line_total.is_none());
}
