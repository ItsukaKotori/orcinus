use ade_git::remote_ops::{
    fetch, fetch_args, is_safe_remote_name, pull, pull_args, GIT_NETWORK_ENVS,
};
use std::process::Command;

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

fn commit(dir: &str, msg: &str) -> bool {
    git(
        dir,
        &["-c", "user.name=T", "-c", "user.email=t@example.com"],
        &["commit", "-m", msg],
    )
}

/// 一个**本地 bare 仓当 remote** —— 网络类命令能在无网、不弹窗的前提下真跑。
/// 返回 (工作仓, bare 远端)。
fn repo_with_origin() -> (tempfile::TempDir, tempfile::TempDir) {
    let remote = tempfile::tempdir().unwrap();
    let rp = remote.path().to_str().unwrap();
    assert!(git(rp, &[], &["init", "--bare", "-b", "main", "."]));

    let work = tempfile::tempdir().unwrap();
    let p = work.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(work.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    assert!(commit(p, "one"));
    assert!(git(p, &[], &["remote", "add", "origin", rp]));
    assert!(git(p, &[], &["push", "-u", "origin", "main"]));
    (work, remote)
}

fn head_subject(dir: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ---- 纯参数函数 ----

#[test]
fn fetch_args_prunes_by_default() {
    // --prune 是必须的：分支树的 Remote 组就是靠这些 ref 列出来的，
    // 不 prune 会一直显示远端已删的幽灵分支。
    assert_eq!(fetch_args(None), ["fetch", "--prune"]);
    assert_eq!(fetch_args(Some("origin")), ["fetch", "--prune", "origin"]);
}

#[test]
fn pull_args_defer_to_git_config() {
    // spec §12.2：不问 merge/rebase 偏好，让 pull.ff / pull.rebase 生效。
    assert_eq!(pull_args(), ["pull"]);
}

#[test]
fn remote_name_guard_blocks_injection_shapes() {
    assert!(is_safe_remote_name("origin"));
    assert!(is_safe_remote_name("my-remote.v2"));
    assert!(!is_safe_remote_name(""));
    assert!(!is_safe_remote_name("-b"));
    assert!(!is_safe_remote_name("origin\n--upload-pack=evil"));
    assert!(!is_safe_remote_name(&"a".repeat(101)));
}

// ---- 真跑（本地 bare remote，无网、无弹窗）----

#[test]
fn fetch_pulls_down_a_commit_the_other_side_pushed() {
    let (work, remote) = repo_with_origin();
    let p = work.path().to_str().unwrap();
    let rp = remote.path().to_str().unwrap();

    // 在远端直接提交一个本地没有的 commit（模拟别人推了东西）。
    let seed = tempfile::tempdir().unwrap();
    let sp = seed.path().to_str().unwrap();
    assert!(git(sp, &[], &["clone", rp, "."]));
    std::fs::write(seed.path().join("b"), "2\n").unwrap();
    assert!(git(sp, &[], &["add", "."]));
    assert!(commit(sp, "upstream two"));
    assert!(git(sp, &[], &["push", "origin", "main"]));

    assert!(fetch(p, Some("origin"), None).is_ok());
    // fetch 只更新 remote-tracking ref，不动本地分支。
    assert_eq!(head_subject(p), "one");
    let show = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["rev-parse", "--short", "origin/main"])
        .output()
        .unwrap();
    let remote_oid = String::from_utf8_lossy(&show.stdout).trim().to_string();
    let local = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["rev-parse", "--short", "main"])
        .output()
        .unwrap();
    assert_ne!(remote_oid, String::from_utf8_lossy(&local.stdout).trim());
}

#[test]
fn pull_fast_forwards_the_local_branch() {
    let (work, remote) = repo_with_origin();
    let p = work.path().to_str().unwrap();
    let rp = remote.path().to_str().unwrap();

    let seed = tempfile::tempdir().unwrap();
    let sp = seed.path().to_str().unwrap();
    assert!(git(sp, &[], &["clone", rp, "."]));
    std::fs::write(seed.path().join("b"), "2\n").unwrap();
    assert!(git(sp, &[], &["add", "."]));
    assert!(commit(sp, "upstream two"));
    assert!(git(sp, &[], &["push", "origin", "main"]));

    assert!(pull(p, None).is_ok());
    // 可快进（本地无分叉提交），所以 HEAD 真的前移了。
    assert_eq!(head_subject(p), "upstream two");
}

#[test]
fn fetch_prunes_deleted_remote_branches() {
    let (work, remote) = repo_with_origin();
    let p = work.path().to_str().unwrap();
    let rp = remote.path().to_str().unwrap();
    assert!(git(rp, &[], &["branch", "-f", "stale", "main"]));
    assert!(fetch(p, Some("origin"), None).is_ok());
    let listed = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["for-each-ref", "refs/remotes/origin/stale"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&listed.stdout).trim().is_empty(),
        "前置：stale 应先被 fetch 下来"
    );

    assert!(git(rp, &[], &["branch", "-D", "stale"]));
    assert!(fetch(p, Some("origin"), None).is_ok());
    let after = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["for-each-ref", "refs/remotes/origin/stale"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&after.stdout).trim().is_empty(),
        "--prune 必须抹掉远端已删的跟踪 ref"
    );
}

#[test]
fn unsafe_remote_name_is_rejected_without_running_git() {
    let (work, _remote) = repo_with_origin();
    let p = work.path().to_str().unwrap();
    let err = fetch(p, Some("-b"), None).unwrap_err();
    assert!(
        matches!(err, ade_core::errors::CoreError::InvalidInput(_)),
        "实际: {err:?}"
    );
    // 且拒绝必须发生在任何 git 进程之前 —— 用不存在的 remote 名反证：
    // 若真跑了 git，错误会是 GitCommandFailed 而不是 InvalidInput。
}

#[test]
fn fetch_against_unreachable_origin_fails_fast_not_hanging() {
    let work = tempfile::tempdir().unwrap();
    let p = work.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(work.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    assert!(commit(p, "one"));
    assert!(git(p, &[], &["remote", "add", "origin", "/nonexistent/nope.git"]));

    let err = fetch(p, Some("origin"), None).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("fetch"), "实际: {text}");
}

// 诚实声明：上面这条**不**守 `GIT_TERMINAL_PROMPT=0`。它用的是本地路径 remote
// （`/nonexistent/nope.git`），git 对本地路径不会弹凭证提示，去掉那个 env
// 本用例照样绿。它真正锁的是：不可达远端返回 Err 而不是 panic/卡死，
// 且错误里带上了命令名（与 push 的 BridgeError::message 不同，那边没有）。
//
// 为什么不能直接测 env：要区分它需要一个会要凭证的 HTTPS 远端，
// 无网环境下造不出来。`GIT_TERMINAL_PROMPT=0` 的正确性靠所有网络命令
// 共用 `GIT_NETWORK_ENVS` 常量 + `run_git_in_with_env` 单一入口保证。
#[test]
fn network_envs_constant_disables_terminal_prompt() {
    // 能自动化的只有这一层：常量内容本身。
    assert_eq!(GIT_NETWORK_ENVS, [("GIT_TERMINAL_PROMPT", "0")]);
}
