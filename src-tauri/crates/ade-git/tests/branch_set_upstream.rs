use ade_git::branch_ops::{GitBranchSetUpstreamInput, GitBranchSetUpstreamStatus, set_upstream};
use std::process::Command;

// helper 照抄 tests/branch_checkout.rs 的同名实现。
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
    assert!(
        git(dir, &["-c", &format!("user.name={author}"), "-c", &format!("user.email={id}")],
            &["commit", "-m", msg]),
        "commit {msg}"
    );
}

/// main（有提交）+ 本地 feature + 远程跟踪 refs/remotes/origin/feature。当前 HEAD 在 main。
/// 必须真配 `remote.origin`：git `--set-upstream-to` 会拒绝仅存在 remote ref
/// 而无 remote 配置的情况（"not a branch"）。
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "T", "one");
    git(p, &[], &["branch", "feature"]);
    assert!(git(p, &[], &["remote", "add", "origin", "/nonexistent-remote"]));
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/feature", "feature"]));
    d
}

fn current_branch(dir: &str) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(["branch", "--show-current"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn upstream_of(dir: &str, branch: &str) -> String {
    let out = Command::new("git")
        .arg("-C").arg(dir)
        .args(["for-each-ref", "--format=%(upstream:short)", &format!("refs/heads/{branch}")])
        .output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn input(branch: &str, upstream: &str) -> GitBranchSetUpstreamInput {
    GitBranchSetUpstreamInput { branch: branch.to_string(), upstream: upstream.to_string() }
}

// 正常：给本地 feature 设 origin/feature 为 upstream；HEAD 不动。
#[test]
fn sets_upstream_for_existing_branch_and_remote_ref() {
    let d = repo();
    let p = d.path().to_str().unwrap();

    let outcome = set_upstream(p, &input("feature", "origin/feature")).unwrap();
    assert!(matches!(outcome.status, GitBranchSetUpstreamStatus::Set), "{:?}", outcome.status);
    assert_eq!(upstream_of(p, "feature"), "origin/feature");
    assert_eq!(current_branch(p), "main", "set-upstream 不得切换 HEAD");
}

// upstream 不在 refs/remotes/* → 结构化拒绝 UpstreamMissing（Ok，不是 Err），且不落任何改动。
#[test]
fn refuses_missing_upstream() {
    let d = repo();
    let p = d.path().to_str().unwrap();

    let outcome = set_upstream(p, &input("feature", "origin/ghost")).unwrap();
    assert!(
        matches!(outcome.status, GitBranchSetUpstreamStatus::Refused(ade_git::BranchOperationRefusal::UpstreamMissing)),
        "{:?}", outcome.status
    );
    assert_eq!(upstream_of(p, "feature"), "", "被拒时不得设置 upstream");
}

// 本地分支不存在 → 环境级 Err（调用方只会从真实本地分支发起）。
#[test]
fn rejects_unknown_local_branch() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(set_upstream(p, &input("nope", "origin/feature")).is_err());
}

// 嵌套分支名（feature/login）也能设上游。
#[test]
fn supports_nested_branch_and_upstream_names() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "T", "one");
    git(p, &[], &["branch", "feat/login"]);
    assert!(git(p, &[], &["remote", "add", "origin", "/nonexistent-remote"]));
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/feat/login", "feat/login"]));

    let outcome = set_upstream(p, &input("feat/login", "origin/feat/login")).unwrap();
    assert!(matches!(outcome.status, GitBranchSetUpstreamStatus::Set), "{:?}", outcome.status);
    assert_eq!(upstream_of(p, "feat/login"), "origin/feat/login");
}
