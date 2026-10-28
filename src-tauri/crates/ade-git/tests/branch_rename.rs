use ade_git::branch_ops::{rename, BranchOperationRefusal, GitBranchRenameInput, GitBranchRenameStatus};
use std::process::Command;

// helper 照抄 tests/branch_delete.rs 的同名实现。
fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C").arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath=", "-c", "init.templateDir="])
        .args(extra).args(args).status().unwrap().success()
}

fn commit(dir: &str, msg: &str) {
    assert!(git(dir, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["commit", "-m", msg]), "commit {msg}");
}

/// main + `feat` + `other`（都未检出）。HEAD 在 main。
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "one");
    git(p, &[], &["branch", "feat"]);
    git(p, &[], &["branch", "other"]);
    d
}

fn current_branch(dir: &str) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(["branch", "--show-current"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ref_exists(dir: &str, reference: &str) -> bool {
    Command::new("git").arg("-C").arg(dir).args(["rev-parse", "--verify", "--quiet", reference]).output().unwrap().status.success()
}

fn input(old: &str, new: &str) -> GitBranchRenameInput {
    GitBranchRenameInput { old_name: old.to_string(), new_name: new.to_string() }
}

#[test]
fn renames_local_branch_without_moving_head() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = rename(p, &input("feat", "renamed")).unwrap();
    assert!(matches!(outcome.status, GitBranchRenameStatus::Renamed), "{:?}", outcome.status);
    assert!(ref_exists(p, "refs/heads/renamed"));
    assert!(!ref_exists(p, "refs/heads/feat"), "旧名应消失");
    assert_eq!(current_branch(p), "main", "rename 不得移动 HEAD");
}

#[test]
fn refuses_name_taken() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = rename(p, &input("feat", "other")).unwrap();
    assert!(
        matches!(outcome.status, GitBranchRenameStatus::Refused(BranchOperationRefusal::NameTaken)),
        "{:?}", outcome.status
    );
    // 无副作用：两个分支都还在。
    assert!(ref_exists(p, "refs/heads/feat") && ref_exists(p, "refs/heads/other"));
    assert_eq!(current_branch(p), "main");
}

#[test]
fn refuses_checked_out_source() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let wt = d.path().join("wt-feat");
    assert!(git(p, &[], &["worktree", "add", wt.to_str().unwrap(), "feat"]));
    let outcome = rename(p, &input("feat", "renamed")).unwrap();
    match outcome.status {
        GitBranchRenameStatus::Refused(BranchOperationRefusal::CheckedOut { worktree_path }) => {
            assert!(worktree_path.ends_with("wt-feat"), "actual: {worktree_path}");
        }
        other => panic!("expected CheckedOut, got {other:?}"),
    }
    assert!(ref_exists(p, "refs/heads/feat"));
    assert_eq!(current_branch(p), "main");
}

#[test]
fn rejects_invalid_or_missing_names() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    // 非法新名 → resolve_create_branch_name 的 check-ref-format 拒。
    let e = rename(p, &input("feat", "bad name")).unwrap_err();
    assert!(e.to_string().contains("check-ref-format"), "实际: {e}");
    // 空新名 → 自己的 non-empty 门。
    let e = rename(p, &input("feat", "")).unwrap_err();
    assert!(e.to_string().contains("empty"), "实际: {e}");
    // 未知源分支 → unknown local branch。
    let e = rename(p, &input("nope", "x")).unwrap_err();
    assert!(e.to_string().contains("unknown local branch"), "实际: {e}");
    assert_eq!(current_branch(p), "main");
}
