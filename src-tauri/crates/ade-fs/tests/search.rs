use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use ade_fs::{
    FsError, FsService, SearchMatch, SearchOptions, SearchResult, MAX_RESULTS_CAP,
    MAX_RESULTS_DEFAULT, PER_FILE_MAX_MATCHES,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ade-fs-search-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn str(&self) -> &str {
        self.path.to_str().expect("temp dir path is UTF-8")
    }

    fn join(&self, name: impl AsRef<Path>) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("path is UTF-8")
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write file");
}

fn git_init(root: &Path) {
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .expect("git is available");
    assert!(status.success(), "git init failed");
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).expect("canonicalize")
}

fn options(root: &Path, query: &str) -> SearchOptions {
    SearchOptions {
        query: query.to_string(),
        root_path: path_str(root).to_string(),
        ..SearchOptions::default()
    }
}

fn search(service: &FsService, options: SearchOptions) -> Result<SearchResult, FsError> {
    service.search(options, service.cancel_registry())
}

fn relative_paths(result: &SearchResult) -> Vec<&str> {
    result
        .files
        .iter()
        .map(|file| file.relative_path.as_str())
        .collect()
}

#[test]
fn finds_matches_with_line_column_length_and_content() {
    let root = TempDir::new("basic");
    write(
        &root.join("src/app.ts"),
        "const needle = 1\nother\nneedle again\n",
    );

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    assert!(!result.truncated);
    assert_eq!(result.total_matches, 2);
    assert_eq!(relative_paths(&result), vec!["src/app.ts"]);
    let file = &result.files[0];
    assert_eq!(
        file.file_path,
        canonical(root.path()).join("src/app.ts").to_string_lossy()
    );
    assert_eq!(file.match_count, 2);
    assert_eq!(
        file.matches,
        vec![
            SearchMatch {
                line: 1,
                column: 7,
                match_length: 6,
                line_content: "const needle = 1".to_string(),
                display_column: None,
                display_match_length: None,
            },
            SearchMatch {
                line: 3,
                column: 1,
                match_length: 6,
                line_content: "needle again".to_string(),
                display_column: None,
                display_match_length: None,
            },
        ]
    );
}

#[test]
fn reports_every_match_on_a_line_in_column_order() {
    let root = TempDir::new("same-line");
    write(&root.join("a.txt"), "needle and needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    assert_eq!(result.total_matches, 2);
    let matches = &result.files[0].matches;
    assert_eq!(matches[0].line, 1);
    assert_eq!(matches[0].column, 1);
    assert_eq!(matches[1].line, 1);
    assert_eq!(matches[1].column, 12);
    assert_eq!(result.files[0].match_count, 2);
}

#[test]
fn splits_comma_separated_globs_and_applies_include_and_exclude() {
    let root = TempDir::new("globs");
    git_init(root.path());
    write(&root.join("keep.ts"), "needle\n");
    write(&root.join("skip.md"), "needle\n");
    write(&root.join("nested/deep/keep.ts"), "needle\n");
    write(&root.join("nested/deep/skip.md"), "needle\n");
    write(&root.join("nested/other.ts"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let mut include = options(root.path(), "needle");
    include.include_pattern = Some("*.ts".to_string());
    let result = search(&service, include).unwrap();
    let mut paths = relative_paths(&result);
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec!["keep.ts", "nested/deep/keep.ts", "nested/other.ts"]
    );

    let mut exclude = options(root.path(), "needle");
    exclude.exclude_pattern = Some("*.md".to_string());
    let result = search(&service, exclude).unwrap();
    let mut paths = relative_paths(&result);
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec!["keep.ts", "nested/deep/keep.ts", "nested/other.ts"]
    );

    let mut combined = options(root.path(), "needle");
    combined.include_pattern = Some("*.ts, nested/**".to_string());
    combined.exclude_pattern = Some("nested/deep/**".to_string());
    let result = search(&service, combined).unwrap();
    let mut paths = relative_paths(&result);
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec!["keep.ts", "nested/other.ts"],
        "exclude wins over include"
    );
}

#[test]
fn keeps_escaped_commas_inside_a_single_glob() {
    let root = TempDir::new("glob-escape");
    git_init(root.path());
    write(&root.join("foo,bar/x.ts"), "needle\n");
    write(&root.join("foo/x.ts"), "needle\n");
    write(&root.join("bar/x.ts"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let mut include = options(root.path(), "needle");
    include.include_pattern = Some("foo\\,bar/**".to_string());
    let result = search(&service, include).unwrap();
    assert_eq!(relative_paths(&result), vec!["foo,bar/x.ts"]);
}

#[test]
fn honors_case_sensitivity_whole_word_and_regex_options() {
    let root = TempDir::new("flags");
    write(&root.join("a.txt"), "Needle needle needled\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let insensitive = search(&service, options(root.path(), "needle")).unwrap();
    assert_eq!(insensitive.total_matches, 3);

    let mut case_sensitive = options(root.path(), "needle");
    case_sensitive.case_sensitive = true;
    let result = search(&service, case_sensitive).unwrap();
    assert_eq!(result.total_matches, 2);

    let mut whole_word = options(root.path(), "needle");
    whole_word.whole_word = true;
    let result = search(&service, whole_word).unwrap();
    assert_eq!(result.total_matches, 2);
    let columns: Vec<usize> = result.files[0]
        .matches
        .iter()
        .map(|m| m.column)
        .collect();
    assert_eq!(columns, vec![1, 8], "whole word must skip `needled`");

    let mut regex = options(root.path(), "need.e");
    regex.use_regex = true;
    let result = search(&service, regex).unwrap();
    assert_eq!(result.total_matches, 3);

    let mut literal = options(root.path(), "need.e");
    literal.use_regex = false;
    let result = search(&service, literal).unwrap();
    assert_eq!(result.total_matches, 0);
}

#[test]
fn respects_gitignore_dot_ignore_and_rgignore_files() {
    let root = TempDir::new("ignore-files");
    git_init(root.path());
    write(&root.join(".gitignore"), "ignored.txt\n");
    write(&root.join(".ignore"), "dot-ignored.txt\n");
    write(&root.join(".rgignore"), "rg-ignored.txt\n");
    write(&root.join("kept.txt"), "needle\n");
    write(&root.join("ignored.txt"), "needle\n");
    write(&root.join("dot-ignored.txt"), "needle\n");
    write(&root.join("rg-ignored.txt"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    assert_eq!(relative_paths(&result), vec!["kept.txt"]);
}

#[test]
fn includes_hidden_files_but_never_the_git_directory() {
    let root = TempDir::new("hidden");
    git_init(root.path());
    write(&root.join(".hidden/config.txt"), "needle\n");
    write(&root.join(".hidden-file.txt"), "needle\n");
    write(&root.join(".git/secret.txt"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    let mut paths = relative_paths(&result);
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec![".hidden-file.txt", ".hidden/config.txt"],
        "hidden files are searched, .git is not"
    );
}

#[test]
fn skips_binary_files_with_nul_bytes() {
    let root = TempDir::new("binary");
    fs::write(root.join("binary.dat"), b"\x00needle\n").unwrap();
    write(&root.join("text.txt"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    assert_eq!(relative_paths(&result), vec!["text.txt"]);
}

#[cfg(unix)]
#[test]
fn does_not_follow_or_search_symlinks() {
    let root = TempDir::new("symlink");
    write(&root.join("real/inner.txt"), "needle\n");
    std::os::unix::fs::symlink(root.join("real"), root.join("link-dir")).unwrap();
    std::os::unix::fs::symlink(root.join("real/inner.txt"), root.join("link-file.txt")).unwrap();

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    assert_eq!(relative_paths(&result), vec!["real/inner.txt"]);
}

#[test]
fn include_globs_override_gitignore_rules() {
    let root = TempDir::new("include-over-ignore");
    git_init(root.path());
    write(&root.join(".gitignore"), "generated.ts\n");
    write(&root.join("generated.ts"), "needle\n");
    write(&root.join("plain.ts"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let without_include = search(&service, options(root.path(), "needle")).unwrap();
    assert_eq!(relative_paths(&without_include), vec!["plain.ts"]);

    let mut include = options(root.path(), "needle");
    include.include_pattern = Some("*.ts".to_string());
    let result = search(&service, include).unwrap();
    let mut paths = relative_paths(&result);
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec!["generated.ts", "plain.ts"],
        "a whitelist glob re-includes a gitignored file"
    );
}

#[test]
fn skips_files_larger_than_five_mib() {
    let root = TempDir::new("file-size");
    let mut oversized = vec![b'a'; 5 * 1024 * 1024 + 1];
    oversized[..6].copy_from_slice(b"needle");
    fs::write(root.join("oversized.txt"), oversized).unwrap();

    let mut exact = vec![b'a'; 5 * 1024 * 1024];
    exact[..6].copy_from_slice(b"needle");
    fs::write(root.join("exact.txt"), exact).unwrap();
    write(&root.join("small.txt"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    let mut paths = relative_paths(&result);
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec!["exact.txt", "small.txt"],
        "only files strictly larger than 5 MiB are skipped"
    );
}

#[test]
fn caps_matches_per_file_at_one_hundred() {
    let root = TempDir::new("per-file-cap");
    let content: String = (0..150).map(|index| format!("needle {index}\n")).collect();
    write(&root.join("many.txt"), &content);

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    assert_eq!(PER_FILE_MAX_MATCHES, 100);
    assert_eq!(result.total_matches, 100);
    assert_eq!(result.files[0].match_count, 100);
    assert_eq!(result.files[0].matches.len(), 100);
    assert_eq!(result.files[0].matches.last().unwrap().line, 100);
    assert!(
        !result.truncated,
        "the per-file cap alone does not truncate the whole result"
    );
}

#[test]
fn marks_truncated_when_total_reaches_max_results_including_exactly() {
    let root = TempDir::new("total-cap");
    for index in 0..5 {
        write(&root.join(format!("f{index}.txt")), "needle\n");
    }

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let mut under = options(root.path(), "needle");
    under.max_results = Some(6);
    let result = search(&service, under).unwrap();
    assert_eq!(result.total_matches, 5);
    assert!(!result.truncated);

    let mut exact = options(root.path(), "needle");
    exact.max_results = Some(5);
    let result = search(&service, exact).unwrap();
    assert_eq!(result.total_matches, 5);
    assert!(
        result.truncated,
        "reaching maxResults exactly must set truncated"
    );

    let mut over = options(root.path(), "needle");
    over.max_results = Some(3);
    let result = search(&service, over).unwrap();
    assert_eq!(result.total_matches, 3);
    assert!(result.truncated);

    let mut zero = options(root.path(), "needle");
    zero.max_results = Some(0);
    let result = search(&service, zero).unwrap();
    assert_eq!(result.total_matches, 1, "maxResults is clamped to at least 1");
    assert!(result.truncated);
}

#[test]
fn clamps_max_results_to_the_two_thousand_cap() {
    let root = TempDir::new("cap-clamp");
    for file_index in 0..21 {
        let content: String = (0..100).map(|index| format!("needle {index}\n")).collect();
        write(&root.join(format!("f{file_index}.txt")), &content);
    }

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    assert_eq!(MAX_RESULTS_DEFAULT, 2000);
    assert_eq!(MAX_RESULTS_CAP, 2000);
    let default_capped = search(&service, options(root.path(), "needle")).unwrap();
    assert_eq!(default_capped.total_matches, 2000);
    assert!(default_capped.truncated);

    let mut huge = options(root.path(), "needle");
    huge.max_results = Some(999_999);
    let result = search(&service, huge).unwrap();
    assert_eq!(result.total_matches, 2000);
    assert!(result.truncated);
}

#[test]
fn reports_utf8_byte_columns_and_clamped_line_content() {
    let root = TempDir::new("byte-columns");
    write(&root.join("unicode.txt"), "日本語needle\n");
    let long_line = format!("{}needle{}", "x".repeat(600), "y".repeat(600));
    write(&root.join("long.txt"), &format!("{long_line}\n"));

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    let unicode = result
        .files
        .iter()
        .find(|file| file.relative_path == "unicode.txt")
        .expect("unicode file result");
    let m = &unicode.matches[0];
    assert_eq!(m.line, 1);
    assert_eq!(m.column, 10, "日本語 is 9 UTF-8 bytes");
    assert_eq!(m.match_length, 6);
    assert_eq!(m.line_content, "日本語needle");
    assert_eq!(m.display_column, None);
    assert_eq!(m.display_match_length, None);

    let long = result
        .files
        .iter()
        .find(|file| file.relative_path == "long.txt")
        .expect("long file result");
    let m = &long.matches[0];
    assert_eq!(m.line, 1);
    assert_eq!(m.column, 601, "column stays the byte offset + 1");
    assert_eq!(m.match_length, 6);
    assert!(m.line_content.chars().count() <= 502);
    assert!(m.line_content.starts_with('…'));
    assert!(m.line_content.ends_with('…'));
    let display_column = m.display_column.expect("display column for clamped content");
    let display_match_length = m
        .display_match_length
        .expect("display match length for clamped content");
    let snippet: String = m
        .line_content
        .chars()
        .skip(display_column - 1)
        .take(display_match_length)
        .collect();
    assert_eq!(snippet, "needle");
}

#[test]
fn returns_empty_line_content_for_non_utf8_lines() {
    let root = TempDir::new("non-utf8");
    fs::write(root.join("invalid.txt"), b"needle \xFF\xFE rest\n").unwrap();

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "needle")).unwrap();

    assert_eq!(result.total_matches, 1);
    let m = &result.files[0].matches[0];
    assert_eq!(m.line, 1);
    assert_eq!(m.column, 1);
    assert_eq!(m.match_length, 6);
    assert_eq!(m.line_content, "");
    assert_eq!(m.display_column, None);
    assert_eq!(m.display_match_length, None);
}

#[test]
fn empty_query_matches_every_line_with_a_navigable_fallback() {
    let root = TempDir::new("empty-query");
    write(&root.join("a.txt"), "alpha\nbeta\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let result = search(&service, options(root.path(), "")).unwrap();

    assert_eq!(result.total_matches, 2);
    let matches = &result.files[0].matches;
    assert_eq!((matches[0].line, matches[0].column, matches[0].match_length), (1, 1, 1));
    assert_eq!((matches[1].line, matches[1].column, matches[1].match_length), (2, 1, 1));
    assert_eq!(matches[0].line_content, "alpha");
}

#[test]
fn a_new_search_for_the_same_root_cancels_the_previous_run() {
    let root = TempDir::new("cancel");
    let service = Arc::new(FsService::new());
    service.authorize_root(root.str()).unwrap();
    for file_index in 0..400 {
        write(
            &root.join(format!("f{file_index}.txt")),
            "needle 1\nneedle 2\nneedle 3\nneedle 4\n",
        );
    }
    let total = 400 * 4;

    let first = {
        let service = Arc::clone(&service);
        let first_options = options(root.path(), "needle");
        thread::spawn(move || service.search(first_options, service.cancel_registry()))
    };
    let second = search(&service, options(root.path(), "needle")).unwrap();
    let first = first.join().expect("search thread").unwrap();

    // Whichever search registers its token second supersedes the other, so
    // exactly one of the racing searches runs to completion.
    let completed = [&first, &second]
        .iter()
        .filter(|result| result.total_matches == total)
        .count();
    assert_eq!(
        completed, 1,
        "exactly one racing search must complete (first={}, second={})",
        first.total_matches, second.total_matches
    );
}

#[test]
fn rejects_invalid_regex_and_glob_patterns() {
    let root = TempDir::new("invalid-patterns");
    write(&root.join("a.txt"), "needle\n");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let mut bad_regex = options(root.path(), "(");
    bad_regex.use_regex = true;
    assert!(matches!(
        search(&service, bad_regex).unwrap_err(),
        FsError::InvalidInput(_)
    ));

    let mut bad_glob = options(root.path(), "needle");
    bad_glob.include_pattern = Some("[".to_string());
    assert!(matches!(
        search(&service, bad_glob).unwrap_err(),
        FsError::InvalidInput(_)
    ));
}

#[test]
fn rejects_unauthorized_roots() {
    let root = TempDir::new("unauthorized");
    write(&root.join("a.txt"), "needle\n");

    let service = FsService::new();
    assert!(matches!(
        search(&service, options(root.path(), "needle")).unwrap_err(),
        FsError::PathAccessDenied
    ));
}

#[test]
fn serializes_results_with_camel_case_contract_fields() {
    let result = SearchResult {
        files: vec![ade_fs::SearchFileResult {
            file_path: "/r/a.ts".to_string(),
            relative_path: "a.ts".to_string(),
            matches: vec![SearchMatch {
                line: 1,
                column: 1,
                match_length: 3,
                line_content: "foo".to_string(),
                display_column: None,
                display_match_length: None,
            }],
            match_count: 1,
        }],
        total_matches: 1,
        truncated: false,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["totalMatches"], 1);
    assert_eq!(json["truncated"], false);
    assert_eq!(json["files"][0]["filePath"], "/r/a.ts");
    assert_eq!(json["files"][0]["relativePath"], "a.ts");
    assert_eq!(json["files"][0]["matchCount"], 1);
    assert_eq!(json["files"][0]["matches"][0]["line"], 1);
    assert_eq!(json["files"][0]["matches"][0]["column"], 1);
    assert_eq!(json["files"][0]["matches"][0]["matchLength"], 3);
    assert_eq!(json["files"][0]["matches"][0]["lineContent"], "foo");
    assert!(json["files"][0]["matches"][0].get("displayColumn").is_none());

    let options: SearchOptions = serde_json::from_value(serde_json::json!({
        "query": "q",
        "rootPath": "/r",
        "caseSensitive": true,
        "wholeWord": true,
        "useRegex": true,
        "includePattern": "*.ts",
        "excludePattern": "*.md",
        "maxResults": 5,
    }))
    .unwrap();
    assert_eq!(options.query, "q");
    assert_eq!(options.root_path, "/r");
    assert!(options.case_sensitive);
    assert!(options.whole_word);
    assert!(options.use_regex);
    assert_eq!(options.include_pattern.as_deref(), Some("*.ts"));
    assert_eq!(options.exclude_pattern.as_deref(), Some("*.md"));
    assert_eq!(options.max_results, Some(5));

    let defaults: SearchOptions =
        serde_json::from_value(serde_json::json!({"query": "q", "rootPath": "/r"})).unwrap();
    assert!(!defaults.case_sensitive);
    assert!(!defaults.whole_word);
    assert!(!defaults.use_regex);
    assert_eq!(defaults.include_pattern, None);
    assert_eq!(defaults.exclude_pattern, None);
    assert_eq!(defaults.max_results, None);

    let serialized = serde_json::to_value(&defaults).unwrap();
    assert_eq!(serialized["rootPath"], "/r");
    assert!(serialized.get("includePattern").is_none());
    assert!(serialized.get("maxResults").is_none());
}
