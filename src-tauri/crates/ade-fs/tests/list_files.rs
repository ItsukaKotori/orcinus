use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ade_fs::read::compare_file_names;
use ade_fs::{FsError, FsService, MarkdownDocument};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ade-fs-list-{name}-{}-{unique}",
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

    fn join(&self, name: &str) -> PathBuf {
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

fn list_files(
    service: &FsService,
    root: &Path,
    exclude_paths: &[String],
    max_results: usize,
) -> Result<Vec<String>, FsError> {
    service.list_files(
        path_str(root),
        exclude_paths,
        max_results,
        "ade-fs-list-files-test",
        service.cancel_registry(),
    )
}

#[test]
fn includes_hidden_files_and_prunes_blocklisted_directories() {
    let root = TempDir::new("hidden-prune");
    git_init(root.path());
    write(&root.join("src/app.ts"), "x");
    write(&root.join(".hidden"), "x");
    write(&root.join(".github/workflows/ci.yml"), "x");
    write(&root.join("node_modules/pkg/index.js"), "x");
    write(&root.join("vendor/node_modules/nested.js"), "x");
    write(&root.join(".cache/blob"), "x");
    write(&root.join(".git/objects/leak"), "x");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let files = list_files(&service, root.path(), &[], 1000).unwrap();

    for included in ["src/app.ts", ".hidden", ".github/workflows/ci.yml"] {
        assert!(
            files.iter().any(|path| path == included),
            "expected {included} in {files:?}"
        );
    }
    for excluded in [
        "node_modules/pkg/index.js",
        "vendor/node_modules/nested.js",
        ".cache/blob",
        ".git/objects/leak",
    ] {
        assert!(
            !files.iter().any(|path| path == excluded),
            "expected {excluded} to be pruned, got {files:?}"
        );
    }

    for path in &files {
        assert!(!path.is_empty(), "empty path in {files:?}");
        assert!(!path.starts_with('/'), "absolute leak {path} in {files:?}");
        assert!(
            !path.contains('\\'),
            "backslash separator {path} in {files:?}"
        );
        assert!(Path::new(path).is_relative(), "not relative: {path}");
    }
}

#[test]
fn primary_pass_respects_gitignore_and_ignored_pass_appends_ignored_paths() {
    let root = TempDir::new("ignored-pass");
    git_init(root.path());
    write(&root.join(".gitignore"), "ignored.txt\nbuild/\n");
    for name in ["src/main.rs", "src/lib.rs", "README.md", ".hidden-file"] {
        write(&root.join(name), "x");
    }
    write(&root.join("ignored.txt"), "x");
    write(&root.join("build/out.js"), "x");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let files = list_files(&service, root.path(), &[], 1000).unwrap();

    let primary = [
        ".gitignore",
        "src/main.rs",
        "src/lib.rs",
        "README.md",
        ".hidden-file",
    ];
    let ignored = ["ignored.txt", "build/out.js"];
    for path in primary.iter().chain(ignored.iter()) {
        assert!(
            files.iter().any(|entry| entry == path),
            "missing {path}: {files:?}"
        );
    }

    let position = |needle: &str| {
        files
            .iter()
            .position(|path| path == needle)
            .unwrap_or_else(|| panic!("{needle} not in {files:?}"))
    };
    let first_ignored = ignored.iter().map(|path| position(path)).min().unwrap();
    for path in primary {
        assert!(
            position(path) < first_ignored,
            "{path} must precede ignored-only paths: {files:?}"
        );
    }

    // A cap filled by the primary pass never reaches the ignored pass, which
    // proves gitignored entries are absent from pass 1 (not merely deferred).
    let bounded = list_files(&service, root.path(), &[], primary.len()).unwrap();
    assert_eq!(bounded.len(), primary.len(), "{bounded:?}");
    assert!(
        bounded.iter().all(|path| primary.contains(&path.as_str())),
        "{bounded:?}"
    );
}

#[test]
fn caps_results_at_max_results() {
    let root = TempDir::new("cap");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        write(&root.join(name), "x");
    }

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let capped = list_files(&service, root.path(), &[], 3).unwrap();
    assert_eq!(capped.len(), 3, "{capped:?}");
    assert!(
        capped.iter().all(|path| !path.starts_with('/')),
        "{capped:?}"
    );

    assert!(list_files(&service, root.path(), &[], 0)
        .unwrap()
        .is_empty());
    assert_eq!(
        list_files(&service, root.path(), &[], 1000).unwrap().len(),
        5
    );
}

#[test]
fn excludes_paths_on_segment_boundaries_and_drops_malformed_values() {
    let root = TempDir::new("exclude");
    write(&root.join("packages/app/a.txt"), "x");
    write(&root.join("packages/app2/b.txt"), "x");
    write(&root.join("packages/other/c.txt"), "x");
    let outside = TempDir::new("exclude-outside");
    write(&outside.join("outside.txt"), "x");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();

    let exclude_paths = vec![
        path_str(&root.join("packages/app")).to_string(),
        format!("{}/packages/other/../app", root.str()),
        format!("{}/packages/app/", root.str()),
        String::new(),
        "/".to_string(),
        root.str().to_string(),
        outside.str().to_string(),
        "packages/other".to_string(),
    ];
    let files = list_files(&service, root.path(), &exclude_paths, 1000).unwrap();

    assert!(
        !files.iter().any(|path| path == "packages/app/a.txt"),
        "{files:?}"
    );
    for kept in ["packages/app2/b.txt", "packages/other/c.txt"] {
        assert!(
            files.iter().any(|path| path == kept),
            "missing {kept}: {files:?}"
        );
    }
}

#[test]
fn escapes_glob_metacharacters_in_exclude_paths() {
    let root = TempDir::new("exclude-glob");
    write(&root.join("feature[1]/x.txt"), "x");
    write(&root.join("feature1/y.txt"), "x");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let exclude_paths = vec![path_str(&root.join("feature[1]")).to_string()];
    let files = list_files(&service, root.path(), &exclude_paths, 1000).unwrap();

    assert!(
        !files.iter().any(|path| path == "feature[1]/x.txt"),
        "{files:?}"
    );
    assert!(
        files.iter().any(|path| path == "feature1/y.txt"),
        "sibling whose name matches the unescaped glob must survive: {files:?}"
    );
}

#[cfg(unix)]
#[test]
fn lists_file_symlinks_but_not_directory_symlinks() {
    let root = TempDir::new("symlink");
    write(&root.join("real/inner.txt"), "x");
    std::os::unix::fs::symlink(root.join("real"), root.join("link-dir")).unwrap();
    std::os::unix::fs::symlink(root.join("real/inner.txt"), root.join("link-file.txt")).unwrap();

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let files = list_files(&service, root.path(), &[], 1000).unwrap();

    assert!(
        files.iter().any(|path| path == "real/inner.txt"),
        "{files:?}"
    );
    assert!(
        !files.iter().any(|path| path == "link-dir"),
        "symlinked directories must not be listed: {files:?}"
    );
    assert!(
        !files.iter().any(|path| path.starts_with("link-dir/")),
        "symlinked directories must not be traversed: {files:?}"
    );
    assert!(
        files.iter().any(|path| path == "link-file.txt"),
        "symlinked file entries are listed as paths: {files:?}"
    );
}

#[test]
fn cancelling_an_unknown_token_is_a_noop() {
    let root = TempDir::new("cancel-noop");
    write(&root.join("a.txt"), "x");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    service.cancel_list_files("ade-fs-list-files-test");

    let files = list_files(&service, root.path(), &[], 100).unwrap();
    assert_eq!(files, vec!["a.txt".to_string()]);
}

#[test]
fn rejects_unauthorized_roots() {
    let root = TempDir::new("unauthorized");
    write(&root.join("a.txt"), "x");

    let service = FsService::new();
    assert!(matches!(
        list_files(&service, root.path(), &[], 100).unwrap_err(),
        FsError::PathAccessDenied
    ));
    assert!(matches!(
        service.list_markdown_documents(root.str()).unwrap_err(),
        FsError::PathAccessDenied
    ));
}

#[test]
fn lists_markdown_documents_with_traversal_rules() {
    let root = TempDir::new("markdown");
    git_init(root.path());
    write(&root.join(".gitignore"), "ignored.md\n");
    write(&root.join("README.md"), "x");
    write(&root.join("docs/Guide.MDX"), "x");
    write(&root.join("docs/notes.markdown"), "x");
    write(&root.join("docs/plain.txt"), "x");
    write(&root.join(".hidden-docs/h.md"), "x");
    write(&root.join("node_modules/pkg/readme.md"), "x");
    write(&root.join("ignored.md"), "x");
    write(&root.join("assets/pic.png"), "x");

    let service = FsService::new();
    service.authorize_root(root.str()).unwrap();
    let documents = service.list_markdown_documents(root.str()).unwrap();

    let relative_paths: Vec<&str> = documents
        .iter()
        .map(|doc| doc.relative_path.as_str())
        .collect();
    let mut expected = vec![
        ".hidden-docs/h.md",
        "README.md",
        "docs/Guide.MDX",
        "docs/notes.markdown",
        "ignored.md",
    ];
    expected.sort_by(|a, b| compare_file_names(a, b));
    assert_eq!(relative_paths, expected);

    assert!(documents.windows(2).all(|pair| compare_file_names(
        &pair[0].relative_path,
        &pair[1].relative_path
    ) != std::cmp::Ordering::Greater));

    let guide = documents
        .iter()
        .find(|doc| doc.relative_path == "docs/Guide.MDX")
        .expect("guide document");
    assert_eq!(
        guide,
        &MarkdownDocument {
            file_path: format!("{}/docs/Guide.MDX", root.str()),
            relative_path: "docs/Guide.MDX".to_string(),
            basename: "Guide.MDX".to_string(),
            name: "Guide".to_string(),
        }
    );
}
