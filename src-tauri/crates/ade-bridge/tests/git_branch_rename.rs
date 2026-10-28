use ade_bridge::commands::git::git_branch_rename_impl;
use ade_git::{BranchOperationRefusal, GitBranchRenameInput, GitBranchRenameStatus};
use std::process::Command;

// helper 照抄 tests/git_branch_create.rs。
fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git").arg("-C").arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath="])
        .args(extra).args(args).status().unwrap().success()
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["add", "."]);
    git(p, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["commit", "-m", "one"]);
    git(p, &[], &["branch", "feat"]);
    git(p, &[], &["branch", "taken"]);
    d
}

fn input(old: &str, new: &str) -> GitBranchRenameInput {
    GitBranchRenameInput { old_name: old.to_string(), new_name: new.to_string() }
}

#[test]
fn git_branch_rename_impl_renames_and_survives_ok() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_branch_rename_impl(p, &input("feat", "renamed")).unwrap();
    assert_eq!(outcome.status, GitBranchRenameStatus::Renamed);
}

#[test]
fn git_branch_rename_impl_surfaces_name_taken_as_ok() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = git_branch_rename_impl(p, &input("feat", "taken")).unwrap();
    assert_eq!(outcome.status, GitBranchRenameStatus::Refused(BranchOperationRefusal::NameTaken), "拒绝必须 Ok 不是 Err");
}

#[test]
fn rename_outcome_serializes_as_discriminated_union() {
    let json = serde_json::to_value(ade_git::GitBranchRenameOutcome {
        status: GitBranchRenameStatus::Refused(BranchOperationRefusal::NameTaken),
        old_name: Some("feat".to_string()),
        new_name: Some("taken".to_string()),
        message: None,
    })
    .unwrap();
    assert_eq!(json["status"]["refused"], "name-taken");
    assert_eq!(json["oldName"], "feat");
    assert_eq!(json["newName"], "taken");
    assert!(json.get("message").is_none());
    assert_eq!(serde_json::to_value(GitBranchRenameStatus::Renamed).unwrap(), "renamed");
}
