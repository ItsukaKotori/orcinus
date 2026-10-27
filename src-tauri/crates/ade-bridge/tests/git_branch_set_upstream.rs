use ade_bridge::commands::git::git_branch_set_upstream_impl;
use ade_git::{BranchOperationRefusal, GitBranchSetUpstreamInput, GitBranchSetUpstreamStatus};
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
    assert!(git(p, &[], &["remote", "add", "origin", "/nonexistent-remote"]));
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/feature", "feature"]));
    d
}

fn input(branch: &str, upstream: &str) -> GitBranchSetUpstreamInput {
    GitBranchSetUpstreamInput { branch: branch.to_string(), upstream: upstream.to_string() }
}

// bridge 层测「透传不吞语义」：拒绝必须以 Ok 活着穿过 BridgeError。
#[test]
fn set_upstream_impl_sets_and_reports_upstream() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_branch_set_upstream_impl(p, &input("feature", "origin/feature")).unwrap();
    assert_eq!(outcome.status, GitBranchSetUpstreamStatus::Set);
    assert_eq!(outcome.upstream.as_deref(), Some("origin/feature"));
}

#[test]
fn set_upstream_impl_surfaces_upstream_missing_as_ok() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_branch_set_upstream_impl(p, &input("feature", "origin/ghost")).unwrap();
    assert_eq!(
        outcome.status,
        GitBranchSetUpstreamStatus::Refused(BranchOperationRefusal::UpstreamMissing),
        "拒绝必须是 Ok，不是 Err"
    );
}

#[test]
fn set_upstream_outcome_serializes_as_discriminated_union() {
    let refused = ade_git::GitBranchSetUpstreamOutcome {
        status: GitBranchSetUpstreamStatus::Refused(BranchOperationRefusal::UpstreamMissing),
        branch: Some("feature".to_string()),
        upstream: Some("origin/ghost".to_string()),
        message: None,
    };
    let json = serde_json::to_value(&refused).unwrap();
    assert_eq!(json["status"]["refused"], "upstream-missing");
    assert_eq!(json["branch"], "feature");
    assert_eq!(json["upstream"], "origin/ghost");
    assert!(json.get("message").is_none(), "actual: {json}");
    assert_eq!(serde_json::to_value(GitBranchSetUpstreamStatus::Set).unwrap(), "set");
}
