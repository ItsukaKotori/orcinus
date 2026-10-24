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
    // 换型后仍要能认出当前分支：ade-bridge 侧的契约测试是 bindings 之外的第二道锁。
    assert_eq!(
        result.refs.iter().filter(|r| r.is_head).count(),
        1,
        "恰好一条 is_head"
    );
}

// 有 upstream 时 ahead 必须落进结构化字段，而不是留在 [ahead N] 文本里。
#[test]
fn git_refs_reports_ahead_against_upstream() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    // 造一个假 origin remote + upstream 配置，不联网：只写 ref 和 config。
    //
    // Why 三条 config 缺一不可：git 靠 `remote.origin.fetch` 的 refspec 才能把
    // `branch.main.merge = refs/heads/main` 反向映射到 `refs/remotes/origin/main`。
    // 少了它，`%(upstream:short)` 恒为空（git 原话："upstream branch 'refs/heads/main'
    // not stored as a remote-tracking branch"），ahead/behind 也就无从计算。
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/main", "HEAD"]));
    assert!(git(p, &[], &["config", "remote.origin.url", "/nonexistent-remote"]));
    assert!(git(
        p,
        &[],
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*"
        ]
    ));
    assert!(git(p, &[], &["config", "branch.main.remote", "origin"]));
    assert!(git(p, &[], &["config", "branch.main.merge", "refs/heads/main"]));
    // 新文件必须显式 add：`commit -a` 只暂存**已跟踪**文件，
    // 直接 `-am` 会「nothing to commit」，分支根本没前进，ahead 永远是 None。
    std::fs::write(d.path().join("b"), "2\n").unwrap();
    assert!(git(p, &[], &["add", "b"]));
    assert!(git(
        p,
        &["-c", "user.name=T", "-c", "user.email=t@example.com"],
        &["commit", "-m", "two"],
    ));

    let result = git_refs_impl(p).unwrap();
    let main = result
        .refs
        .iter()
        .find(|r| r.name == "main")
        .expect("main listed");
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!(main.ahead, Some(1));
    assert_eq!(main.behind, None);
    assert_eq!(main.category, ade_git::GitRefCategory::Local);
    assert!(main.is_head, "main 仍是当前分支");
    // 远程条目自身归到 Remote 类，并带出 remote 名。
    let origin_main = result
        .refs
        .iter()
        .find(|r| r.full_name == "refs/remotes/origin/main")
        .expect("origin/main listed");
    assert_eq!(origin_main.category, ade_git::GitRefCategory::Remote);
    assert_eq!(origin_main.remote.as_deref(), Some("origin"));
    assert!(!origin_main.is_head);
    // 无 upstream 的分支不能被判成 0/0 —— 缺省与「同步」是两回事。
    let feature = result
        .refs
        .iter()
        .find(|r| r.name == "feature")
        .expect("feature listed");
    assert_eq!(feature.upstream, None);
    assert_eq!(feature.ahead, None);
    assert_eq!(feature.behind, None);
}

// Task 1 review 指出 history_query.rs 的 symref 跳过断言在其 fixture 下是空断言
// （那个 repo() 根本没有 refs/remotes/*/HEAD）。这里造一个真的 symref 把它守住：
// 少了这条，`origin/HEAD` 会作为一条名为 `origin/HEAD` 的假分支出现在分支树里。
#[test]
fn git_refs_skips_remote_head_symref() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["update-ref", "refs/remotes/origin/main", "HEAD"]));
    assert!(git(
        p,
        &[],
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main"
        ]
    ));
    // 前置条件：symref 确实存在，否则本测试又退化成空断言。
    assert!(git(p, &[], &["show-ref", "--verify", "refs/remotes/origin/HEAD"]));

    let result = git_refs_impl(p).unwrap();
    assert!(
        result
            .refs
            .iter()
            .any(|r| r.full_name == "refs/remotes/origin/main"),
        "origin/main 要在列表里，才能证明不是整体没取到远程 ref"
    );
    assert!(
        !result
            .refs
            .iter()
            .any(|r| r.full_name == "refs/remotes/origin/HEAD"),
        "refs/remotes/*/HEAD symref 必须被跳过"
    );
    assert!(
        !result.refs.iter().any(|r| r.name == "origin/HEAD"),
        "短名形式也不能漏出来"
    );
}
