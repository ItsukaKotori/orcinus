use ade_bridge::commands::git::git_branch_delete_impl;
use ade_git::{BranchOperationRefusal, GitBranchDeleteInput, GitBranchDeleteStatus};
use std::process::Command;

// helper 照抄 tests/git_branch_create.rs。
fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git").arg("-C").arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath="])
        .args(extra).args(args).status().unwrap().success()
}

fn commit(dir: &str, msg: &str) {
    assert!(git(dir, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["commit", "-m", msg]));
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "one");
    git(p, &[], &["checkout", "-b", "feature"]);
    std::fs::write(d.path().join("b"), "x\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "feat work");
    git(p, &[], &["checkout", "main"]);
    d
}

fn input(branch: &str, force: bool) -> GitBranchDeleteInput {
    GitBranchDeleteInput { branch: branch.to_string(), force }
}

#[test]
fn git_branch_delete_impl_surfaces_not_mergeable_as_ok() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_branch_delete_impl(p, &input("feature", false)).unwrap();
    match outcome.status {
        GitBranchDeleteStatus::Refused(BranchOperationRefusal::NotMergeable { unmerged_count }) => {
            assert_eq!(unmerged_count, 1);
        }
        other => panic!("expected NotMergeable, got {other:?}"),
    }
}

// NotMergeable 的可判别联合形状（含 unmergedCount）必须与 TS 手抄镜像 git-branch-ops-types.ts 一致——
// 这是那份手抄对 not-mergeable 变体的唯一自动锁。
#[test]
fn delete_outcome_serializes_not_mergeable_with_count() {
    let json = serde_json::to_value(ade_git::GitBranchDeleteOutcome {
        status: GitBranchDeleteStatus::Refused(BranchOperationRefusal::NotMergeable { unmerged_count: 3 }),
        branch: Some("feature".to_string()),
        message: None,
    })
    .unwrap();
    assert_eq!(json["status"]["refused"]["not-mergeable"]["unmergedCount"], 3);
    assert_eq!(json["branch"], "feature");
    assert!(json.get("message").is_none());
    assert_eq!(serde_json::to_value(GitBranchDeleteStatus::Deleted).unwrap(), "deleted");
}
