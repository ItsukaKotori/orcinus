use ade_bridge::commands::git::{git_log_query_impl, GitLogQueryIn};
use std::process::Command;

fn git(dir: &str, extra: &[&str], args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
        ])
        .args(extra)
        .args(args)
        .status()
        .unwrap()
        .success()
}

/// main: one(T) → two(T); feature: from one → feat two(T). All authored "T".
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_str().unwrap();
    git(p, &[], &["init", "-b", "main", "."]);
    std::fs::write(d.path().join("a"), "1\n").unwrap();
    git(p, &[], &["add", "."]);
    git(p, &[], &["commit", "-m", "one"]);
    git(p, &[], &["checkout", "-b", "feature"]);
    std::fs::write(d.path().join("b"), "2\n").unwrap();
    git(p, &[], &["add", "."]);
    git(p, &[], &["commit", "-m", "feat two"]);
    git(p, &[], &["checkout", "main"]);
    std::fs::write(d.path().join("a"), "1\n2\n").unwrap();
    git(p, &[], &["add", "."]);
    git(p, &[], &["commit", "-m", "two on main"]);
    d
}

fn q(refs: Option<&str>, author: Option<&str>, skip: u32, limit: u32) -> GitLogQueryIn {
    GitLogQueryIn {
        refs: refs.map(str::to_string),
        author: author.map(str::to_string),
        since: None,
        until: None,
        grep: None,
        path: None,
        skip: Some(skip),
        limit: Some(limit),
    }
}

#[test]
fn head_scope_excludes_feature_only_commit() {
    let d = repo();
    let r = git_log_query_impl(d.path().to_str().unwrap(), &q(None, None, 0, 50)).unwrap();
    assert!(!r.items.iter().any(|i| i.subject == "feat two"));
    assert!(r.items.iter().any(|i| i.subject == "two on main"));
}

#[test]
fn all_scope_includes_both_branches() {
    let d = repo();
    let r = git_log_query_impl(d.path().to_str().unwrap(), &q(Some("all"), None, 0, 50)).unwrap();
    assert!(r.items.iter().any(|i| i.subject == "feat two"));
    assert!(r.items.iter().any(|i| i.subject == "two on main"));
}

#[test]
fn author_filter_reaches_backend() {
    let d = repo();
    let r = git_log_query_impl(d.path().to_str().unwrap(), &q(Some("all"), Some("T"), 0, 50)).unwrap();
    assert!(!r.items.is_empty());
    assert!(r.items.iter().all(|i| i.author.as_deref() == Some("T")));
}

#[test]
fn path_filter_reaches_backend() {
    // Why: guards the native view's path filter end-to-end. Repo has 3 commits on
    // `all`; only "feat two" touches file `b`, so a dropped `path` field (→ 3) vs
    // forwarded (→ 1) is the discriminating assertion the retired bridge test held.
    let d = repo();
    let base = q(Some("all"), None, 0, 50);
    let g = GitLogQueryIn { path: Some("b".into()), ..base };
    let r = git_log_query_impl(d.path().to_str().unwrap(), &g).unwrap();
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].subject, "feat two");
}

#[test]
fn grep_filter_reaches_backend() {
    // Why: same invariant for the grep field — only "feat two" matches "feat".
    let d = repo();
    let base = q(Some("all"), None, 0, 50);
    let g = GitLogQueryIn { grep: Some("feat".into()), ..base };
    let r = git_log_query_impl(d.path().to_str().unwrap(), &g).unwrap();
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].subject, "feat two");
}

#[test]
fn unknown_revision_is_error() {
    let d = repo();
    assert!(git_log_query_impl(d.path().to_str().unwrap(), &q(Some("nope"), None, 0, 50)).is_err());
}

#[test]
fn non_iso_date_is_error() {
    let d = repo();
    let g = GitLogQueryIn {
        refs: None,
        author: None,
        since: Some("last week".into()),
        until: None,
        grep: None,
        path: None,
        skip: None,
        limit: None,
    };
    assert!(git_log_query_impl(d.path().to_str().unwrap(), &g).is_err());
}
