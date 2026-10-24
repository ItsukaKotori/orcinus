use ade_git::history::{history_with, list_refs, list_refs_capped, GitHistoryQuery, GitHistoryRefs};
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
    let has = |name: &str| refs.refs.iter().any(|r| r.name == name);
    assert!(has("main"));
    assert!(has("feature"));
    assert!(refs.refs.iter().any(|r| r.category == ade_git::GitRefCategory::Local));
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

#[test]
fn list_refs_peels_annotated_tag_to_its_commit() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(
        p,
        &["-c", "user.name=T", "-c", "user.email=t@example.com"],
        &["tag", "-a", "v1", "-m", "release", "HEAD"],
    ));
    let refs = list_refs(p).unwrap().refs;
    let main = refs.iter().find(|r| r.name == "main").expect("main ref");
    let v1 = refs.iter().find(|r| r.name == "v1").expect("v1 tag");
    // Annotated tag must report the peeled commit oid, matching the branch on
    // the same commit — not the tag-object oid.
    assert_eq!(v1.oid, main.oid);
    assert!(!v1.oid.is_empty());
    assert_eq!(v1.category, ade_git::GitRefCategory::Tag);
}

#[test]
fn list_refs_capped_limits_the_returned_count() {
    let d = repo();
    let p = d.path().to_str().unwrap();
    assert!(git(p, &[], &["branch", "third"]));
    assert!(list_refs(p).unwrap().refs.len() >= 3);
    let capped = list_refs_capped(p, 2).unwrap();
    assert_eq!(capped.refs.len(), 2);
    assert!(capped.truncated);
}

// 增量 1：%(upstream:track) 解析矩阵。计数是装饰，不是列 refs 的前置条件，
// 所以畸形输入必须降级为 None 而不是报错。
#[test]
fn parse_upstream_track_matrix() {
    use ade_git::ref_entry::parse_upstream_track;
    assert_eq!(parse_upstream_track(""), (None, None));
    assert_eq!(parse_upstream_track("[ahead 14]"), (Some(14), None));
    assert_eq!(parse_upstream_track("[behind 3]"), (None, Some(3)));
    assert_eq!(parse_upstream_track("[ahead 2, behind 1]"), (Some(2), Some(1)));
    assert_eq!(parse_upstream_track("[behind 1, ahead 2]"), (Some(2), Some(1)));
    // 畸形/未知形状 → 降级，不 panic、不报错
    assert_eq!(parse_upstream_track("[gone]"), (None, None));
    assert_eq!(parse_upstream_track("[ahead x]"), (None, None));
    assert_eq!(parse_upstream_track("garbage"), (None, None));
}

// 增量 1：list_refs 必须带出 isHead / category / fullName。
// 本机实测 git 2.54 输出：当前分支的 %(HEAD) 列为 "*"，无 upstream 时 upstream 列为空。
#[test]
fn list_refs_carries_head_flag_and_categories() {
    let d = repo();
    let result = list_refs(d.path().to_str().unwrap()).unwrap();
    let by_name: std::collections::HashMap<&str, _> =
        result.refs.iter().map(|r| (r.name.as_str(), r)).collect();

    let main = by_name.get("main").expect("main listed");
    assert_eq!(main.full_name, "refs/heads/main");
    assert_eq!(main.category, ade_git::GitRefCategory::Local);
    assert!(main.is_head, "当前分支 main 必须 is_head");
    assert_eq!(main.upstream, None);
    assert_eq!(main.remote, None);

    let feature = by_name.get("feature").expect("feature listed");
    assert!(!feature.is_head);

    assert!(
        !result.refs.iter().any(|r| r.full_name.ends_with("/HEAD")),
        "refs/remotes/*/HEAD symref 必须继续被跳过"
    );
}
