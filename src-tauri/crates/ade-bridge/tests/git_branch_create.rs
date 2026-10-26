use ade_bridge::commands::git::git_branch_create_impl;
use ade_git::{BranchOperationRefusal, GitBranchCreateInput, GitBranchCreateStatus};
use std::process::Command;

// helper 照抄 tests/git_checkout.rs 的同名实现。
fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath="])
        .args(extra)
        .args(args)
        .status()
        .unwrap()
        .success()
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["add", "."]);
    git(p, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["commit", "-m", "one"]);
    git(p, &[], &["branch", "feature"]);
    d
}

fn head_oid(dir: &str) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn input(name: &str, start: &str) -> GitBranchCreateInput {
    GitBranchCreateInput { name: name.to_string(), start_point: start.to_string() }
}

// bridge 层只测「透传不吞语义」：ade-git 已测行为，这里守拒绝以 Ok 活着穿过 BridgeError。
#[test]
fn git_branch_create_impl_creates_without_checkout() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_branch_create_impl(p, &input("brand-new", &head_oid(p))).unwrap();
    assert_eq!(outcome.status, GitBranchCreateStatus::Created);
    assert_eq!(outcome.branch.as_deref(), Some("brand-new"));
}

#[test]
fn git_branch_create_impl_surfaces_name_taken_as_ok() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_branch_create_impl(p, &input("feature", &head_oid(p))).unwrap();
    assert_eq!(
        outcome.status,
        GitBranchCreateStatus::Refused(BranchOperationRefusal::NameTaken),
        "拒绝必须是 Ok，不是 Err —— BridgeError 会把 status 压成 {{message}}"
    );
    assert_eq!(outcome.branch.as_deref(), Some("feature"));
}

// 序列化形状必须与 TS 手抄镜像 git-branch-ops-types.ts 一致。
#[test]
fn create_outcome_serializes_as_discriminated_union() {
    let refused = ade_git::GitBranchCreateOutcome {
        status: GitBranchCreateStatus::Refused(BranchOperationRefusal::NameTaken),
        branch: Some("feature".to_string()),
        message: None,
    };
    let json = serde_json::to_value(&refused).unwrap();
    assert_eq!(json["status"]["refused"], "name-taken");
    assert_eq!(json["branch"], "feature");
    assert!(json.get("message").is_none(), "actual: {json}");
    assert_eq!(
        serde_json::to_value(GitBranchCreateStatus::Created).unwrap(),
        "created"
    );
}

// 共享 enum 的 CheckedOut 变体（本增量不发，但 rename/delete 增量 4 会用）必须序列化成
// externally-tagged 的 { "checked-out": { worktreePath } } —— 这是 git-branch-ops-types.ts
// 对 CheckedOut 形状声明的唯一自动锁。
#[test]
fn checked_out_refusal_serializes_with_worktree_path() {
    let json = serde_json::to_value(BranchOperationRefusal::CheckedOut {
        worktree_path: "/other".to_string(),
    })
    .unwrap();
    assert_eq!(json["checked-out"]["worktreePath"], "/other");
}
