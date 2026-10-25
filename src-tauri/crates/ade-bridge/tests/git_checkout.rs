use ade_bridge::commands::git::git_checkout_impl;
use ade_git::{GitCheckoutRefusal, GitCheckoutStatus, GitCheckoutTarget};
use std::process::Command;

// helper 照抄 tests/git_refs.rs 的同名实现。
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
    git(
        p,
        &["-c", "user.name=T", "-c", "user.email=t@example.com"],
        &["commit", "-m", "one"],
    );
    git(p, &[], &["branch", "feature"]);
    d
}

fn current_branch(dir: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn target(kind: &str, name: &str) -> GitCheckoutTarget {
    GitCheckoutTarget { kind: kind.to_string(), name: name.to_string() }
}

// bridge 层只测一件事：**透传不吞语义**。ade-git 已经测过行为本身，
// 这里守的是「拒绝必须以 Ok 的形式活着穿过 BridgeError 这一层」。
#[test]
fn git_checkout_impl_switches_and_reports_new_branch() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_checkout_impl(p, &target("local", "feature")).unwrap();
    assert_eq!(outcome.status, GitCheckoutStatus::Switched);
    assert_eq!(outcome.branch.as_deref(), Some("feature"));
    assert_eq!(current_branch(p), "feature");
}

#[test]
fn git_checkout_impl_surfaces_checked_out_elsewhere_with_path() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let other = d.path().join("wt-feature");
    assert!(git(p, &[], &["worktree", "add", other.to_str().unwrap(), "feature"]));

    let outcome = git_checkout_impl(p, &target("local", "feature")).unwrap();
    assert_eq!(
        outcome.status,
        GitCheckoutStatus::Refused(GitCheckoutRefusal::CheckedOutElsewhere),
        "拒绝必须是 Ok 而不是 Err —— BridgeError 会把 status 压成 {{message}}"
    );
    assert!(outcome.worktree_path.is_some());
    assert_eq!(current_branch(p), "main");
}

// 序列化形状必须与 TS 手抄镜像一致：`status` 是可判别联合，
// Refused 序列化成 `{"refused":"checked-out-elsewhere"}` 而不是别的表示。
// 这条是 `git-checkout-types.ts` 与生成绑定之间唯一的自动锁。
#[test]
fn refusal_serializes_as_discriminated_union() {
    let outcome = ade_git::GitCheckoutOutcome {
        status: GitCheckoutStatus::Refused(GitCheckoutRefusal::CheckedOutElsewhere),
        branch: Some("feature".to_string()),
        worktree_path: Some("/other".to_string()),
        message: None,
    };
    let json = serde_json::to_value(&outcome).unwrap();
    assert_eq!(json["status"]["refused"], "checked-out-elsewhere");
    assert_eq!(json["branch"], "feature");
    assert_eq!(json["worktreePath"], "/other");
    // skip_serializing_if 生效：None 字段整个键都不出现（TS 侧因此写 `?: T`）。
    assert!(json.get("message").is_none(), "actual: {json}");

    assert_eq!(
        serde_json::to_value(GitCheckoutStatus::AlreadyOnBranch).unwrap(),
        "already-on-branch"
    );
}
