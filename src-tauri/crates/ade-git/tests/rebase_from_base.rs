use ade_git::rebase_ops::{abort_merge, abort_rebase, rebase_from_base};
use std::path::Path;
use std::process::Command;

fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C").arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath=", "-c", "init.templateDir="])
        .args(extra).args(args).status().unwrap().success()
}
fn commit(dir: &str, msg: &str) -> bool {
    git(dir, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["commit", "-m", msg])
}
fn rebase_state_exists(dir: &str) -> bool {
    let p = Path::new(dir).join(".git");
    p.join("rebase-merge").exists() || p.join("rebase-apply").exists()
}

/// main + `feat`（从 main 分叉）。HEAD 在 feat。
fn repo_with_feat() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    assert!(commit(p, "base"));
    git(p, &[], &["checkout", "-b", "feat"]);
    std::fs::write(d.path().join("a"), "1\nfeat\n").unwrap();
    git(p, &[], &["add", "."]);
    assert!(commit(p, "feat work"));
    d
}

#[test]
fn rebases_feat_onto_advanced_local_main() {
    let d = repo_with_feat();
    let p = d.path().to_str().unwrap();
    // main 前进一个不冲突提交。
    git(p, &[], &["checkout", "main"]);
    std::fs::write(d.path().join("b"), "2\n").unwrap();
    git(p, &[], &["add", "."]);
    assert!(commit(p, "main two"));
    git(p, &[], &["checkout", "feat"]);

    assert!(rebase_from_base(p, "main", None).is_ok());
    // 线性化：feat 的父链现在包含 main two，且无 merge 提交。
    let log = Command::new("git").arg("-C").arg(p).args(["log", "--format=%s", "feat"]).output().unwrap();
    let subj = String::from_utf8_lossy(&log.stdout);
    assert!(subj.contains("main two") && subj.contains("feat work"), "rebase 后 feat 应含两侧提交: {subj}");
    assert!(!rebase_state_exists(p));
}

#[test]
fn conflict_leaves_rebase_state_and_abort_restores_it() {
    let d = repo_with_feat();
    let p = d.path().to_str().unwrap();
    // main 与 feat 改同一行制造冲突。
    git(p, &[], &["checkout", "main"]);
    std::fs::write(d.path().join("a"), "1\nMAIN\n").unwrap();
    git(p, &[], &["add", "."]);
    assert!(commit(p, "main conflicting"));
    let head_before = Command::new("git").arg("-C").arg(p).args(["rev-parse", "feat"]).output().unwrap();
    git(p, &[], &["checkout", "feat"]);

    let err = rebase_from_base(p, "main", None).unwrap_err();
    assert!(err.to_string().contains("rebase") || err.to_string().contains("conflict"), "实际: {err}");
    assert!(rebase_state_exists(p), "冲突后应留在 rebase 态（半完成，可被 abort 恢复）");

    // abort 回滚到 rebase 前的 feat。
    assert!(abort_rebase(p, None).is_ok());
    assert!(!rebase_state_exists(p));
    let head_after = Command::new("git").arg("-C").arg(p).args(["rev-parse", "feat"]).output().unwrap();
    assert_eq!(head_before.stdout, head_after.stdout, "abort 后 feat 应回到 rebase 前");
}

#[test]
fn rejects_unsafe_base_ref_without_running_git() {
    let d = repo_with_feat();
    let p = d.path().to_str().unwrap();
    for bad in ["", "-b", "  ", "feat\n--exec=evil"] {
        let e = rebase_from_base(p, bad, None).unwrap_err();
        assert!(matches!(e, ade_core::errors::CoreError::InvalidInput(_)), "base={bad:?} 实际: {e:?}");
    }
}

#[test]
fn abort_merge_is_ok_when_nothing_to_abort_or_errors_cleanly() {
    let d = repo_with_feat();
    let p = d.path().to_str().unwrap();
    // 无进行中的 merge：git merge --abort 会失败；abort_merge 应把它作为可操作错误原样上抛（不 panic）。
    let result = abort_merge(p, None);
    assert!(result.is_ok() || result.unwrap_err().to_string().contains("abort"));
}
