use ade_git::branch_ops::{delete, BranchOperationRefusal, GitBranchDeleteInput, GitBranchDeleteStatus};
use std::process::Command;

// helper 照抄 tests/branch_create.rs 的同名实现。
fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath=", "-c", "init.templateDir="])
        .args(extra)
        .args(args)
        .status()
        .unwrap()
        .success()
}

fn commit(dir: &str, author: &str, msg: &str) {
    let id = format!("{author}@example.com");
    assert!(git(dir, &["-c", &format!("user.name={author}"), "-c", &format!("user.email={id}")], &["commit", "-m", msg]), "commit {msg}");
}

/// main（one→two）。`merged` 与 main 同点（已并入 HEAD）；`feature` 领先一个提交（未合并）。HEAD 在 main。
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "T", "one");
    git(p, &[], &["branch", "merged"]);
    std::fs::write(d.path().join("a"), "1\n2\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "T", "two on main");
    git(p, &[], &["checkout", "-b", "feature"]);
    std::fs::write(d.path().join("b"), "x\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "Lin", "feature work");
    git(p, &[], &["checkout", "main"]);
    d
}

fn current_branch(dir: &str) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(["branch", "--show-current"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ref_exists(dir: &str, reference: &str) -> bool {
    Command::new("git").arg("-C").arg(dir)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output().unwrap().status.success()
}

fn input(branch: &str, force: bool) -> GitBranchDeleteInput {
    GitBranchDeleteInput { branch: branch.to_string(), force }
}

// 已并入 HEAD 的分支：-d 直接删除，HEAD 不动。
#[test]
fn deletes_merged_branch() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = delete(p, &input("merged", false)).unwrap();
    assert!(matches!(outcome.status, GitBranchDeleteStatus::Deleted), "{:?}", outcome.status);
    assert!(!ref_exists(p, "refs/heads/merged"));
    assert_eq!(current_branch(p), "main");
}

// 未合并 + 非 force → 结构化拒绝 NotMergeable，带未合入 HEAD 的提交数（feature=1）；分支不被删。
#[test]
fn refuses_unmerged_with_count() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = delete(p, &input("feature", false)).unwrap();
    match outcome.status {
        GitBranchDeleteStatus::Refused(BranchOperationRefusal::NotMergeable { unmerged_count }) => {
            assert_eq!(unmerged_count, 1, "feature 领先 main 一个提交");
        }
        other => panic!("expected NotMergeable, got {other:?}"),
    }
    assert!(ref_exists(p, "refs/heads/feature"), "被拒时不得删除分支");
    assert_eq!(current_branch(p), "main");
}

// force=true → -D 强删未合并分支。
#[test]
fn force_deletes_unmerged() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = delete(p, &input("feature", true)).unwrap();
    assert!(matches!(outcome.status, GitBranchDeleteStatus::Deleted), "{:?}", outcome.status);
    assert!(!ref_exists(p, "refs/heads/feature"));
}

// 被别的 worktree 检出 → CheckedOut{worktreePath}，不删。用结构化 worktree_list，不靠 stderr 文本。
#[test]
fn refuses_checked_out_elsewhere() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let other = d.path().join("wt-feat");
    assert!(git(p, &[], &["worktree", "add", other.to_str().unwrap(), "feature"]));
    let outcome = delete(p, &input("feature", false)).unwrap();
    match outcome.status {
        GitBranchDeleteStatus::Refused(BranchOperationRefusal::CheckedOut { worktree_path }) => {
            assert!(worktree_path.ends_with("wt-feat"), "actual: {worktree_path}");
        }
        other => panic!("expected CheckedOut, got {other:?}"),
    }
    assert!(ref_exists(p, "refs/heads/feature"));
    assert_eq!(current_branch(p), "main");
}

// 未知分支 → 环境级 Err（create 自己的 InvalidInput 门，含 "unknown local branch"），不是 git 报错。
#[test]
fn unknown_branch_is_invalid_input() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let e = delete(p, &input("nope", false)).unwrap_err();
    assert!(e.to_string().contains("unknown local branch"), "实际: {e}");
}

// 空名 → InvalidInput 门。
#[test]
fn empty_name_is_invalid_input() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let e = delete(p, &input("", false)).unwrap_err();
    assert!(e.to_string().contains("empty"), "实际: {e}");
}
