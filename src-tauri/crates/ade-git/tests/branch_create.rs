use ade_git::branch_ops::{create, GitBranchCreateInput, GitBranchCreateStatus};
use std::process::Command;

// helper 照抄 tests/branch_checkout.rs 的同名实现（已验证可用），另加 head_oid / ref_exists。
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

/// main: one → two；feature: 从 one 分叉。当前 HEAD 在 main。
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "T", "one");
    git(p, &[], &["checkout", "-b", "feature"]);
    std::fs::write(d.path().join("b"), "2\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "Lin", "feat two");
    git(p, &[], &["checkout", "main"]);
    std::fs::write(d.path().join("a"), "1\n2\n").unwrap();
    git(p, &[], &["add", "."]);
    commit(p, "T", "two on main");
    d
}

fn current_branch(dir: &str) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(["branch", "--show-current"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn head_oid(dir: &str) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ref_exists(dir: &str, reference: &str) -> bool {
    Command::new("git")
        .arg("-C").arg(dir)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output().unwrap().status.success()
}

fn args(name: &str, start_point: &str) -> GitBranchCreateInput {
    GitBranchCreateInput { name: name.to_string(), start_point: start_point.to_string() }
}

// 正常：在 start-point 建分支 ref，但 HEAD 绝不动（create ≠ checkout）。
#[test]
fn creates_branch_at_start_point_without_checking_it_out() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let start = head_oid(p);

    let outcome = create(p, &args("new-branch", &start)).unwrap();
    assert!(matches!(outcome.status, GitBranchCreateStatus::Created), "{:?}", outcome.status);
    assert_eq!(outcome.branch.as_deref(), Some("new-branch"));
    assert!(ref_exists(p, "refs/heads/new-branch"), "ref 必须建出来");
    assert_eq!(current_branch(p), "main", "create 不得检出，HEAD 保持在 main");
}

// 名已占用 → 结构化拒绝 NameTaken（不是 Err，也不是静默成功）。
#[test]
fn rejects_taken_name() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let start = head_oid(p);

    let outcome = create(p, &args("feature", &start)).unwrap();
    assert!(
        matches!(outcome.status, GitBranchCreateStatus::Refused(ade_git::BranchOperationRefusal::NameTaken)),
        "{:?}", outcome.status
    );
    assert_eq!(current_branch(p), "main");
}

// 非法名（含空格 / `-` 前缀）→ 环境级 Err（走 resolve_create_branch_name 校验），不建 ref。
#[test]
fn rejects_invalid_name_via_check_ref_format() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let start = head_oid(p);

    for bad in ["bad name", "-leading-dash", "..", ""] {
        let res = create(p, &args(bad, &start));
        assert!(res.is_err(), "非法名 {bad:?} 必须 Err，实际 {:?}", res.map(|o| o.status));
    }
    assert_eq!(current_branch(p), "main");
}

// start-point 解析不到 → 环境级 Err（不接受任意 revision，防注入），且不留下半成品 ref。
#[test]
fn rejects_unresolvable_start_point() {
    let d = repo();
    let p = d.path().to_str().unwrap();

    let res = create(p, &args("fresh", "this-rev-does-not-exist"));
    assert!(res.is_err());
    assert!(!ref_exists(p, "refs/heads/fresh"), "失败的 create 不得留下分支 ref");
    assert_eq!(current_branch(p), "main");
}

// 后端不发非 ASCII 本地化文案（I3，照 branch_checkout 同类守卫）。
#[test]
fn outcome_messages_are_not_hardcoded_localizable_text() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = create(p, &args("feature", &head_oid(p))).unwrap();
    if let Some(message) = outcome.message {
        assert!(message.is_ascii(), "后端 message 不得含非 ASCII：{message}");
    }
}
