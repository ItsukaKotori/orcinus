use ade_git::fork_sync::{fork_sync, ForkSyncBlockedReason, ForkSyncStatus, GitForkSyncExpectedUpstream, GitForkSyncInput};
use std::process::Command;

fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git").arg("-C").arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath=", "-c", "init.templateDir="])
        .args(extra).args(args).status().unwrap().success()
}
fn commit(dir: &str, msg: &str) -> bool {
    git(dir, &["-c", "user.name=T", "-c", "user.email=t@example.com"], &["commit", "-m", msg])
}
fn head_of(repo: &str, branch: &str) -> String {
    let o = Command::new("git").arg("-C").arg(repo).args(["rev-parse", branch]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// 一个 fork 形拓扑：upstream U 与 origin O 都有 `base`，U 额外领先一个 `upstream extra`。
/// W 同时配 origin/upstream 两个 remote（本地路径）。返回 (W, O, U)。
fn fork_fixture() -> (tempfile::TempDir, tempfile::TempDir, tempfile::TempDir) {
    let upstream = tempfile::tempdir().unwrap();
    let origin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let seed = tempfile::tempdir().unwrap();
    let up = upstream.path().to_str().unwrap();
    let or = origin.path().to_str().unwrap();
    let sd = seed.path().to_str().unwrap();

    git(up, &[], &["init", "--bare", "-b", "main", "."]);
    git(or, &[], &["init", "--bare", "-b", "main", "."]);

    // seed：一个 commit 同时推给 origin 和 upstream（fork 起点一致）。
    git(sd, &[], &["init", "-b", "main", "."]);
    std::fs::write(seed.path().join("a"), "1\n").unwrap();
    git(sd, &[], &["add", "."]);
    assert!(commit(sd, "base"));
    assert!(git(sd, &[], &["push", or, "main"]));
    assert!(git(sd, &[], &["push", up, "main"]));
    // upstream 额外前进一个（origin 落后 → 可同步）。
    std::fs::write(seed.path().join("b"), "2\n").unwrap();
    git(sd, &[], &["add", "."]);
    assert!(commit(sd, "upstream extra"));
    assert!(git(sd, &[], &["push", up, "main"]));

    let w = work.path().to_str().unwrap();
    git(w, &[], &["init", "-b", "main", "."]);
    assert!(git(w, &[], &["remote", "add", "origin", or]));
    assert!(git(w, &[], &["remote", "add", "upstream", up]));
    (work, origin, upstream)
}

fn input(expected: Option<(&str, &str)>) -> GitForkSyncInput {
    GitForkSyncInput {
        expected_upstream: expected.map(|(o, r)| GitForkSyncExpectedUpstream { owner: o.to_string(), repo: r.to_string() }),
    }
}

#[test]
fn syncs_behind_origin_to_upstream_tip() {
    let (w, origin, upstream) = fork_fixture();
    let wp = w.path().to_str().unwrap();
    let result = fork_sync(wp, &input(None)).unwrap();
    assert_eq!(result.status, ForkSyncStatus::Synced, "{result:?}");
    assert_eq!(result.behind, 1);
    // origin 的 main 现等于 upstream 的 tip。
    assert_eq!(head_of(origin.path().to_str().unwrap(), "main"), head_of(upstream.path().to_str().unwrap(), "main"));
}

#[test]
fn reports_up_to_date_when_origin_already_has_tip() {
    let (w, _origin, _upstream) = fork_fixture();
    let wp = w.path().to_str().unwrap();
    assert_eq!(fork_sync(wp, &input(None)).unwrap().status, ForkSyncStatus::Synced);
    // 第二次：origin 已含 tip → up-to-date。
    assert_eq!(fork_sync(wp, &input(None)).unwrap().status, ForkSyncStatus::UpToDate);
}

#[test]
fn blocked_missing_upstream_when_no_upstream_remote() {
    let work = tempfile::tempdir().unwrap();
    let wp = work.path().to_str().unwrap();
    git(wp, &[], &["init", "-b", "main", "."]);
    let origin = tempfile::tempdir().unwrap();
    git(origin.path().to_str().unwrap(), &[], &["init", "--bare", "-b", "main", "."]);
    git(wp, &[], &["remote", "add", "origin", origin.path().to_str().unwrap()]);
    let result = fork_sync(wp, &input(None)).unwrap();
    assert_eq!(result.status, ForkSyncStatus::Blocked);
    assert_eq!(result.reason, Some(ForkSyncBlockedReason::MissingUpstream));
}

#[test]
fn blocked_diverged_when_origin_ahead() {
    let (w, origin, _upstream) = fork_fixture();
    let wp = w.path().to_str().unwrap();
    // 给 origin 推一个 upstream 没有的提交（用第二个 seed）。
    let seed2 = tempfile::tempdir().unwrap();
    let s2 = seed2.path().to_str().unwrap();
    git(s2, &[], &["clone", origin.path().to_str().unwrap(), "."]);
    std::fs::write(seed2.path().join("o"), "x\n").unwrap();
    git(s2, &[], &["add", "."]);
    assert!(commit(s2, "origin-only"));
    assert!(git(s2, &[], &["push", "origin", "main"]));
    let result = fork_sync(wp, &input(None)).unwrap();
    assert_eq!(result.status, ForkSyncStatus::Blocked);
    assert_eq!(result.reason, Some(ForkSyncBlockedReason::Diverged), "origin 有独有提交 → diverged");
}

#[test]
fn blocked_upstream_mismatch_for_local_path_upstream() {
    let (w, _origin, _upstream) = fork_fixture();
    let wp = w.path().to_str().unwrap();
    // upstream remote 是本地路径（非 GitHub URL）→ 与任何 owner/repo 期望都不匹配。
    let result = fork_sync(wp, &input(Some(("acme", "widgets")))).unwrap();
    assert_eq!(result.status, ForkSyncStatus::Blocked);
    assert_eq!(result.reason, Some(ForkSyncBlockedReason::UpstreamMismatch));
}
