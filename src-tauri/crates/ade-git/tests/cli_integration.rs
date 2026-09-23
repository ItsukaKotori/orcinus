use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ade_core::errors::CoreError;

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

    fn path_str(&self) -> &str {
        self.path.to_str().expect("temp dir path is UTF-8")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn init_repo_with_commit(repo: &Path) {
    git(repo, &["init", "--quiet"]);
    git(
        repo,
        &[
            "-c",
            "user.email=ade-test@example.com",
            "-c",
            "user.name=Ade Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
}

#[test]
fn is_available_detects_git_on_path() {
    assert!(ade_git::is_available());
}

#[test]
fn worktree_list_returns_single_main_worktree_for_fresh_repo() {
    let dir = TempDir::new("main");
    init_repo_with_commit(&dir.path);

    let entries = ade_git::worktree_list(dir.path_str()).expect("worktree_list");

    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert!(entry.is_main_worktree);
    assert!(!entry.is_bare);
    assert!(!entry.head.is_empty());
    assert!(entry.head.chars().all(|c| c.is_ascii_hexdigit()));
    let branch = entry.branch.as_deref().expect("branch ref");
    assert!(branch.starts_with("refs/heads/"));
    assert_eq!(
        std::fs::canonicalize(&entry.path).expect("canonicalize entry path"),
        std::fs::canonicalize(&dir.path).expect("canonicalize temp dir")
    );
}

#[test]
fn worktree_list_reports_linked_and_detached_worktrees() {
    let root = TempDir::new("linked");
    let repo = root.path.join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    init_repo_with_commit(&repo);

    let linked = root.path.join("wt-feature");
    let detached = root.path.join("wt-detached");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature/x",
            linked.to_str().expect("linked path is UTF-8"),
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            detached.to_str().expect("detached path is UTF-8"),
        ],
    );

    let entries =
        ade_git::worktree_list(repo.to_str().expect("repo path is UTF-8")).expect("worktree_list");

    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_main_worktree);
    assert_eq!(
        std::fs::canonicalize(&entries[0].path).expect("canonicalize main path"),
        std::fs::canonicalize(&repo).expect("canonicalize repo dir")
    );
    assert!(entries[1..].iter().all(|entry| !entry.is_main_worktree));

    let linked_entry = entries
        .iter()
        .find(|entry| {
            std::fs::canonicalize(&entry.path).expect("canonicalize entry path")
                == std::fs::canonicalize(&linked).expect("canonicalize linked dir")
        })
        .expect("linked worktree entry");
    assert_eq!(linked_entry.branch.as_deref(), Some("refs/heads/feature/x"));

    let detached_entry = entries
        .iter()
        .find(|entry| {
            std::fs::canonicalize(&entry.path).expect("canonicalize entry path")
                == std::fs::canonicalize(&detached).expect("canonicalize detached dir")
        })
        .expect("detached worktree entry");
    assert_eq!(detached_entry.branch, None);
    assert!(!detached_entry.head.is_empty());
}

#[test]
fn rev_parse_toplevel_resolves_repo_root_from_subdirectory() {
    let dir = TempDir::new("rev-parse");
    init_repo_with_commit(&dir.path);
    let subdir = dir.path.join("nested");
    std::fs::create_dir_all(&subdir).expect("create subdir");
    let subdir = subdir.to_str().expect("subdir path is UTF-8");

    let toplevel = ade_git::rev_parse_toplevel(subdir).expect("rev_parse_toplevel");

    assert_eq!(
        std::fs::canonicalize(toplevel).expect("canonicalize toplevel"),
        std::fs::canonicalize(&dir.path).expect("canonicalize temp dir")
    );
    assert!(ade_git::is_inside_work_tree(subdir));
}

#[test]
fn worktree_list_fails_outside_git_repository() {
    let dir = TempDir::new("nonrepo");
    let path = dir.path_str();

    assert!(matches!(
        ade_git::worktree_list(path),
        Err(CoreError::NotAGitRepository(_))
    ));
    assert!(matches!(
        ade_git::rev_parse_toplevel(path),
        Err(CoreError::NotAGitRepository(_))
    ));
    assert!(!ade_git::is_inside_work_tree(path));
}
