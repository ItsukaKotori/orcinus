use ade_git::branch_ops::{checkout, GitCheckoutStatus, GitCheckoutTarget};
use std::process::Command;

// helper 逐字照抄 tests/history_query.rs 的同名实现（那套已验证可用）。
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

/// main: one → two；feature: 从 one 分叉出一个提交。当前 HEAD 在 main。
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
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `<短名>|up=<upstream短名>`；ref 不存在时返回空串。
fn ref_line(dir: &str, reference: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["for-each-ref", &format!("--format=%(refname:short)|up=%(upstream:short)"), reference])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn target(kind: &str, name: &str) -> GitCheckoutTarget {
    GitCheckoutTarget { kind: kind.to_string(), name: name.to_string() }
}

// 正常路径：本地分支存在且未被别的 worktree 占用 → Switched，HEAD 真的动了。
#[test]
fn checkout_local_branch_switches_head() {
    let d = repo();
    let p = d.path().to_str().unwrap();

    let outcome = checkout(p, &target("local", "feature")).unwrap();
    assert!(
        matches!(outcome.status, GitCheckoutStatus::Switched),
        "{:?}",
        outcome.status
    );
    assert_eq!(outcome.branch.as_deref(), Some("feature"));
    assert_eq!(current_branch(p), "feature");
}

// F1：分支被别的 worktree 检出时 git 会 fatal。我们必须先复核并回结构化拒绝，
// 而不是把 git 的 stderr 当环境级错误抛给前端。
#[test]
fn checkout_refuses_branch_already_checked_out_in_another_worktree() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let other = d.path().join("wt-feature");
    assert!(git(p, &[], &["worktree", "add", other.to_str().unwrap(), "feature"]));

    let outcome = checkout(p, &target("local", "feature")).unwrap();
    assert_eq!(
        outcome.status,
        ade_git::GitCheckoutStatus::Refused(ade_git::GitCheckoutRefusal::CheckedOutElsewhere)
    );
    assert_eq!(outcome.branch.as_deref(), Some("feature"));
    // 占用方路径必须回出来，前端要拿它做「跳到那个 workspace」的跳转。
    assert!(outcome.worktree_path.as_deref().unwrap().ends_with("wt-feature"));
    // 且 HEAD 不能被移动 —— 拒绝必须是**无副作用**的。
    assert_eq!(current_branch(p), "main");
}

// F2：远程目标必须建**本地跟踪分支**，不能把用户推进 detached HEAD。
//
// 注意本测试**不**守 `--track` 这个 flag：start point 是远程跟踪 ref 时，
// git 的 `branch.autoSetupMerge` 默认就会建跟踪，实测拿掉 `--track` 本用例仍绿。
// 守 `--track` 的是下面那条 no-configured-remote 用例。
//
// 两个非显然的 fixture 要求（缺任一条本测试就会“假通过”或假失败）：
//  1. 必须配 `remote.origin.url` + `remote.origin.fetch`（否则 --track 直接 fatal）。
//  2. 目标短名本地**不能已存在**。否则 resolve() 会走「检出已有本地分支」那条，
//     根本执行不到建跟踪分支的逻辑 —— 踩过。
#[test]
fn checkout_remote_creates_local_tracking_branch() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["config", "remote.origin.url", "/nonexistent-remote"]));
    assert!(git(
        p,
        &[],
        &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]
    ));
    // `newwork` 本地不存在，只在 origin 上。
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/newwork", "main"]));

    let outcome = checkout(p, &target("remote", "origin/newwork")).unwrap();
    assert!(
        matches!(outcome.status, GitCheckoutStatus::Switched),
        "{:?}",
        outcome.status
    );

    let line = ref_line(p, "refs/heads/newwork");
    // 短名是 newwork（不是 detached），且 upstream 真指向 origin/newwork。
    assert_eq!(line, "newwork|up=origin/newwork", "actual: {line}");
    assert_eq!(current_branch(p), "newwork", "绝不能是 detached");
}

// 远程 ref 存在但 remote 未配置时，git 的 --track 会 fatal。本命令把它作为
// **环境级 Err** 上抛（不谎报成 InvalidTarget，也不静默降级成不跟踪），
// 并把 git 那句人话留给前端显示。
//
// 这条才是 `--track` 的**唯一守卫**：实测拿掉 `--track` 后本用例会红，
// 因为 git 不再报错，而是**静默建出一个不跟踪的半成品分支**。
#[test]
fn remote_target_without_configured_remote_is_an_error_not_silent_tracking_loss() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/newwork", "main"]));

    let err = checkout(p, &target("remote", "origin/newwork")).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("tracking"), "actual: {text}");
    // 且不能留下一个“建好了但没跟踪”的半成品分支。
    assert_eq!(ref_line(p, "refs/heads/newwork"), "");
    assert_eq!(current_branch(p), "main");
}

// 已在目标分支上是幂等的：不执行 checkout，不报错。
#[test]
fn checkout_current_branch_is_noop() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = checkout(p, &target("local", "main")).unwrap();
    assert_eq!(outcome.status, ade_git::GitCheckoutStatus::AlreadyOnBranch);
    assert_eq!(current_branch(p), "main");
}

// 不存在的 ref / 非法 kind 一律 InvalidTarget，**不是** Err —— 否则前端把它显示成
// 一条环境级错误而不是一句「这个目标不能检出」。
#[test]
fn unknown_targets_are_invalid_target_not_error() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    for t in [
        target("local", "nope"),
        target("remote", "origin/nope"),
        target("tag", "v1"),
        target("local", ""),
        target("local", "--help"),
        target("local", "main..feature"),
        target("local", "HEAD@{0}"),
    ] {
        let outcome = checkout(p, &t).unwrap();
        assert_eq!(outcome.status, ade_git::GitCheckoutStatus::InvalidTarget, "{t:?}");
    }
    // 非法目标不能留下任何副作用。
    assert_eq!(current_branch(p), "main");
}

// 远程目标但本地已有同名分支 → 检出已有本地分支，不再 `-b`（否则 git 报 already exists）。
#[test]
fn remote_target_with_existing_local_branch_checks_out_local() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/feature", "feature"]));
    // refs/heads/feature 已存在但不是当前分支 → 普通检出，不走 --track。
    let outcome = checkout(p, &target("remote", "origin/feature")).unwrap();
    assert!(matches!(outcome.status, GitCheckoutStatus::Switched), "{:?}", outcome.status);
    assert_eq!(current_branch(p), "feature");
}

// 远程目标、本地无同名分支、但该短名被别的 worktree 检出 → 仍须走 F1 拒绝。
// （覆盖 resolve() 之后才做占用复核这个顺序。）
#[test]
fn remote_target_still_refuses_when_local_name_is_occupied_elsewhere() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let other = d.path().join("wt-feat2");
    assert!(git(p, &[], &["branch", "feat2"]));
    assert!(git(p, &[], &["worktree", "add", other.to_str().unwrap(), "feat2"]));
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/feat2", "feat2"]));

    let outcome = checkout(p, &target("remote", "origin/feat2")).unwrap();
    assert_eq!(
        outcome.status,
        ade_git::GitCheckoutStatus::Refused(ade_git::GitCheckoutRefusal::CheckedOutElsewhere)
    );
    assert_eq!(current_branch(p), "main");
}

// C1：分支短名可以含 `/`。`origin/feat/login`、`origin/release/1.2` 是最常见的
// 命名，旧写法用 `short.contains('/')` 把它们全判成 InvalidTarget（而本地分支
// 没这限制）。这条同时堵住“把 remote 名误当短名第一段”的回归。
#[test]
fn checkout_remote_supports_nested_branch_names() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["config", "remote.origin.url", "/nonexistent-remote"]));
    assert!(git(
        p,
        &[],
        &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]
    ));
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/feat/login", "main"]));

    let outcome = checkout(p, &target("remote", "origin/feat/login")).unwrap();
    assert!(
        matches!(outcome.status, GitCheckoutStatus::Switched),
        "嵌套名的远程分支必须能检出，实际 {:?}",
        outcome.status
    );
    assert_eq!(current_branch(p), "feat/login");
    assert_eq!(
        ref_line(p, "refs/heads/feat/login"),
        "feat/login|up=origin/feat/login"
    );
}

#[cfg(unix)]
// I2：`occupant()` 的路径比较必须走规范化。macOS 上 tempdir 是 `/var/...` 而
// `git worktree list` 报 `/private/var/...`，裸比字符串会让“排除自己”变成死代码。
// 这里用主 worktree 的 **symlink 拼写** 去调用，断言仍正确拒掉属于另一个 worktree
// 的分支、且占用方路径回的是那个 worktree 而不是主仓别名。
//
// 诚实声明：这条**不是**“自排除变死代码”那个 bug 的守卫 —— 裸比 `w.path != here`
// 下本用例同样通过（占用方 wt-sym 与 here 两种比较下都不等）。要触发那个 bug 需要
// `git branch --show-current` 失败回落，集成测试里造不出来。它的价值是锁住
// “symlink 拼写不改变检出语义”，以及占用方路径回传的正确性。
#[test]
fn occupant_detection_survives_symlinked_path_spelling() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let other = d.path().join("wt-sym");
    assert!(git(p, &[], &["worktree", "add", other.to_str().unwrap(), "feature"]));

    // 给主 worktree 造一个 symlink 别名，用别名去调 checkout。
    let link_dir = tempfile::tempdir().unwrap();
    let link = link_dir.path().join("repo-alias");
    std::os::unix::fs::symlink(p, &link).expect("symlink");

    let outcome = checkout(link.to_str().unwrap(), &target("local", "feature")).unwrap();
    assert_eq!(
        outcome.status,
        ade_git::GitCheckoutStatus::Refused(ade_git::GitCheckoutRefusal::CheckedOutElsewhere),
        "symlink 拼写不得让主 worktree 被误认为可检出 feature"
    );
    // 占用方路径必须是另一个 worktree，不能因为拼写差异把主仓自己吐回去。
    let occupant = outcome.worktree_path.unwrap();
    assert!(occupant.ends_with("wt-sym"), "actual: {occupant}");
    assert!(!occupant.contains("repo-alias"));
}

// 后端不发中文文案（I3）：message 是英文的可读回退，真正的用户文案由前端
// 按 status 走 i18n。扫遍 Rust 源码，这里是唯一一处会透出到 UI 的硬编码文案。
#[test]
fn refusal_messages_are_not_hardcoded_localizable_text() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let outcome = checkout(p, &target("local", "nope")).unwrap();
    let message = outcome.message.unwrap_or_default();
    assert!(
        message.is_ascii(),
        "后端 message 不得含非 ASCII 本地化文案，实际: {message}"
    );
}
