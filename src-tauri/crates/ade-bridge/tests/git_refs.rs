use ade_bridge::commands::git::git_refs_impl;
use std::process::Command;

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

#[test]
fn git_refs_lists_branches_not_truncated() {
    let d = repo();
    let result = git_refs_impl(d.path().to_str().unwrap()).unwrap();
    let names: Vec<&str> = result.refs.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"main"));
    assert!(names.contains(&"feature"));
    assert!(!result.truncated);
}
