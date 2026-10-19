use ade_git::history::{history_with, list_refs, GitHistoryQuery, GitHistoryRefs};
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

fn commit(dir: &str, author: &str, msg: &str) {
    let id = format!("{author}@example.com");
    assert!(
        git(dir, &["-c", &format!("user.name={author}"), "-c", &format!("user.email={id}")],
            &["commit", "-m", msg]),
        "commit {msg}"
    );
}

/// main: one(T) → two(T); feature: from one → feat two(Lin)
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

fn head(limit: u32) -> GitHistoryQuery {
    GitHistoryQuery { refs: GitHistoryRefs::Head, limit, ..Default::default() }
}

#[test]
fn head_scope_excludes_feature_only_commit() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let r = history_with(p, &head(50)).unwrap();
    assert!(!r.items.iter().any(|i| i.subject == "feat two"));
    assert!(r.items.iter().any(|i| i.subject == "two on main"));
}

#[test]
fn all_scope_includes_both_branches() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let q = GitHistoryQuery { refs: GitHistoryRefs::All, limit: 50, ..Default::default() };
    let r = history_with(p, &q).unwrap();
    assert!(r.items.iter().any(|i| i.subject == "feat two"));
    assert!(r.items.iter().any(|i| i.subject == "two on main"));
}

#[test]
fn author_filter_matches_only_that_author() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let q = GitHistoryQuery {
        refs: GitHistoryRefs::All,
        author: Some("Lin".into()),
        limit: 50,
        ..Default::default()
    };
    let r = history_with(p, &q).unwrap();
    assert!(!r.items.is_empty());
    assert!(r.items.iter().all(|i| i.author.as_deref() == Some("Lin")));
}

#[test]
fn skip_paging_sets_has_more_and_advances() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let page0 = history_with(p, &head(1)).unwrap();
    assert_eq!(page0.items.len(), 1);
    assert!(page0.has_more);
    let q1 = GitHistoryQuery { refs: GitHistoryRefs::Head, limit: 1, skip: 1, ..Default::default() };
    let page1 = history_with(p, &q1).unwrap();
    assert_ne!(page0.items[0].id, page1.items[0].id);
}

#[test]
fn list_refs_categorizes_branches() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let refs = list_refs(p).unwrap();
    let has = |name: &str| refs.iter().any(|r| r.name == name);
    assert!(has("main"));
    assert!(has("feature"));
    assert!(refs.iter().any(|r| format!("{:?}", r.category).contains("Branches")));
}

#[test]
fn rev_scope_unknown_revision_is_error() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    let q = GitHistoryQuery {
        refs: GitHistoryRefs::Rev("does-not-exist".into()),
        limit: 50,
        ..Default::default()
    };
    assert!(history_with(p, &q).is_err());
}
