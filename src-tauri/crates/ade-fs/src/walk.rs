use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ignore::overrides::{Override, OverrideBuilder};
use ignore::WalkBuilder;
use serde::Serialize;

use crate::auth::lexical_absolute;
use crate::read::compare_file_names;
use crate::{FsError, FsService};

/// Tool-generated cache/state directories that Quick Open prunes during
/// traversal. Mirrors the oracle `HIDDEN_DIR_BLOCKLIST` plus the separate
/// `.local/share` path and `node_modules` prune in
/// `src/shared/quick-open-filter.ts`; the Rust matcher keeps both forms in one
/// list and treats entries containing `/` as path-shaped.
pub const HIDDEN_DIR_BLOCKLIST: &[&str] = &[
    ".git",
    ".next",
    ".nuxt",
    ".cache",
    ".stably",
    ".vscode",
    ".idea",
    ".yarn",
    ".pnpm-store",
    ".terraform",
    ".docker",
    ".husky",
    ".npm",
    ".npm-global",
    ".gvfs",
    ".local/share",
    "node_modules",
];

pub const FILE_LISTING_CANCELLED_MESSAGE: &str = "File listing cancelled";

/// How often the traversal samples the cancellation flag. Cancels are latched,
/// so a scan that runs past the next sample still stops promptly.
const CANCEL_CHECK_INTERVAL: usize = 256;

const MARKDOWN_EXTENSIONS: &[&str] = &["md", "mdx", "markdown"];

/// rg/git glob metacharacters; escaping keeps a directory literally named
/// `feature[1]` from excluding `feature1`.
const GLOB_METACHARACTERS: &[char] = &['*', '?', '[', ']', '{', '}', '\\'];

/// Session-scoped cancellation tokens for [`FsService::list_files`]. Each run
/// registers a fresh flag under its request token; `cancel` latches the flag
/// the traversal samples. Starting a run with an already-registered token
/// aborts the previous run first so superseded scans cannot keep walking.
#[derive(Debug, Default, Clone)]
pub struct CancelRegistry {
    tokens: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl CancelRegistry {
    /// Abort the run registered under `token`; unknown tokens are a no-op.
    pub fn cancel(&self, token: &str) {
        if let Some(flag) = lock(&self.tokens).get(token) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    fn begin(&self, token: &str) -> Arc<AtomicBool> {
        let mut tokens = lock(&self.tokens);
        if let Some(previous) = tokens.get(token) {
            previous.store(true, Ordering::SeqCst);
        }
        let flag = Arc::new(AtomicBool::new(false));
        tokens.insert(token.to_string(), Arc::clone(&flag));
        flag
    }

    /// Drop the token only while this run still owns it; a newer run with the
    /// same token must keep its cancellation reachable.
    fn finish(&self, token: &str, flag: &Arc<AtomicBool>) {
        let mut tokens = lock(&self.tokens);
        if tokens
            .get(token)
            .is_some_and(|current| Arc::ptr_eq(current, flag))
        {
            tokens.remove(token);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Registers a run for the duration of a scan and unregisters it on drop.
/// Shared with later cancellable traversals (Task 6 search).
pub(crate) struct CancelGuard {
    registry: CancelRegistry,
    token: String,
    flag: Arc<AtomicBool>,
}

impl CancelGuard {
    pub(crate) fn new(registry: CancelRegistry, token: &str) -> Self {
        let flag = registry.begin(token);
        Self {
            registry,
            token: token.to_string(),
            flag,
        }
    }

    pub(crate) fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        self.registry.finish(&self.token, &self.flag);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct MarkdownDocument {
    pub file_path: String,
    pub relative_path: String,
    pub basename: String,
    pub name: String,
}

impl FsService {
    /// Abort the quick-open scan registered under `token`.
    pub fn cancel_list_files(&self, token: &str) {
        self.cancel.cancel(token);
    }

    /// Shared handle to the service registry so callers can pass the same
    /// cancellation state into [`FsService::list_files`].
    pub fn cancel_registry(&self) -> CancelRegistry {
        self.cancel.clone()
    }

    /// List files under `root` for Quick Open: root-relative, `/`-separated.
    ///
    /// Two passes per the oracle: the primary pass honors gitignore rules and
    /// the second (`no_ignore_vcs`) pass appends ignored files after it, with
    /// duplicates dropped. Hidden files are included; the hidden-dir blocklist
    /// is pruned with directory-form override globs plus a segment backstop.
    /// `exclude_paths` are absolute nested-worktree paths reduced to
    /// root-relative prefixes and matched on segment boundaries; malformed or
    /// outside-root values are silently dropped. `max_results` bounds the
    /// result (the caller reads a full page as truncated).
    ///
    /// `cancel` must share state with this service's registry (pass
    /// [`FsService::cancel_registry`]); only then does
    /// [`FsService::cancel_list_files`] reach the running scan. A foreign
    /// registry is still cancellable through its own [`CancelRegistry::cancel`].
    pub fn list_files(
        &self,
        root: &str,
        exclude_paths: &[String],
        max_results: usize,
        token: &str,
        cancel: CancelRegistry,
    ) -> Result<Vec<String>, FsError> {
        let resolved = self.resolve(root)?;
        let exclude_prefixes = build_exclude_path_prefixes(root, exclude_paths);
        let guard = CancelGuard::new(cancel, token);
        collect_quick_open_paths(
            &resolved,
            &exclude_prefixes,
            Some(max_results),
            Some(guard.flag()),
            &|_| true,
        )
    }

    /// Collect markdown documents (`.md`/`.mdx`/`.markdown`, case-insensitive)
    /// with the same traversal rules as [`FsService::list_files`], sorted by
    /// relative path. `file_path` echoes the caller's root form so it round
    /// trips through the same IPC paths the renderer stores.
    pub fn list_markdown_documents(&self, root: &str) -> Result<Vec<MarkdownDocument>, FsError> {
        let resolved = self.resolve(root)?;
        let relative_paths =
            collect_quick_open_paths(&resolved, &[], None, None, &is_markdown_document_path)?;
        let root_prefix = root.replace('\\', "/").trim_end_matches('/').to_string();
        let mut documents: Vec<MarkdownDocument> = relative_paths
            .iter()
            .map(|relative_path| markdown_document(&root_prefix, relative_path))
            .collect();
        documents.sort_by(|a, b| compare_file_names(&a.relative_path, &b.relative_path));
        Ok(documents)
    }
}

/// Normalize `exclude_paths` into root-relative, `/`-separated prefixes.
/// Values that are empty, relative, equal to the root, or outside the root are
/// silently dropped (a stale nested-worktree path must not fail the request).
pub fn build_exclude_path_prefixes(root: &str, exclude_paths: &[String]) -> Vec<String> {
    let mut root_forms: Vec<PathBuf> = Vec::new();
    if let Ok(lexical) = lexical_absolute(root) {
        root_forms.push(lexical);
    }
    if let Ok(canonical) = Path::new(root).canonicalize() {
        if !root_forms.contains(&canonical) {
            root_forms.push(canonical);
        }
    }
    if root_forms.is_empty() {
        return Vec::new();
    }

    let mut prefixes: Vec<String> = Vec::new();
    for raw in exclude_paths {
        let candidate = Path::new(raw);
        if raw.is_empty() || !candidate.is_absolute() {
            continue;
        }
        let Ok(absolute) = lexical_absolute(raw) else {
            continue;
        };
        let Some(relative) = root_forms
            .iter()
            .find_map(|root_form| absolute.strip_prefix(root_form).ok())
        else {
            continue;
        };
        let rel = normalize_relative_path(relative)
            .trim_end_matches('/')
            .to_string();
        if rel.is_empty() || rel == ".." || rel.starts_with("../") || rel.starts_with('/') {
            continue;
        }
        if !prefixes.contains(&rel) {
            prefixes.push(rel);
        }
    }
    prefixes
}

/// Segment-boundary exclusion: a raw `starts_with` would exclude
/// `packages/app2` when `packages/app` is the excluded nested worktree.
pub fn should_exclude_quick_open_rel_path(relative_path: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| {
        relative_path == prefix
            || (relative_path.starts_with(prefix.as_str())
                && relative_path.as_bytes().get(prefix.len()) == Some(&b'/'))
    })
}

/// Correctness backstop after the pruning globs: true when `path`
/// (`/`-separated, root-relative) traverses no blocklisted segment or path.
pub fn should_include_quick_open_path(path: &str) -> bool {
    for blocked in HIDDEN_DIR_BLOCKLIST {
        if blocked.contains('/') && contains_blocked_rel_path(path, blocked) {
            return false;
        }
    }
    path.split('/')
        .all(|segment| !HIDDEN_DIR_BLOCKLIST.contains(&segment))
}

pub fn is_markdown_document_path(path: &str) -> bool {
    let Some(extension) = Path::new(path).extension() else {
        return false;
    };
    MARKDOWN_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn markdown_document(root_prefix: &str, relative_path: &str) -> MarkdownDocument {
    let basename = relative_path
        .rsplit('/')
        .next()
        .unwrap_or(relative_path)
        .to_string();
    let name = Path::new(&basename)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| basename.clone());
    MarkdownDocument {
        file_path: format!("{root_prefix}/{relative_path}"),
        relative_path: relative_path.to_string(),
        basename,
        name,
    }
}

fn contains_blocked_rel_path(path: &str, blocked_path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut search_from = 0;
    while let Some(offset) = path[search_from..].find(blocked_path) {
        let start = search_from + offset;
        let end = start + blocked_path.len();
        let boundary_before = start == 0 || bytes[start - 1] == b'/';
        let boundary_after = end == path.len() || bytes[end] == b'/';
        if boundary_before && boundary_after {
            return true;
        }
        search_from = end;
    }
    false
}

fn collection_error() -> FsError {
    FsError::Cancelled(FILE_LISTING_CANCELLED_MESSAGE.to_string())
}

/// Shared two-pass traversal behind `list_files` and
/// `list_markdown_documents`. `keep` filters leaf paths; directories are
/// pruned by the blocklist/exclude globs plus the segment backstop.
fn collect_quick_open_paths(
    root: &Path,
    exclude_prefixes: &[String],
    max_results: Option<usize>,
    cancelled: Option<&AtomicBool>,
    keep: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, FsError> {
    if max_results == Some(0) {
        return Ok(Vec::new());
    }
    if is_cancelled(cancelled) {
        return Err(collection_error());
    }

    let overrides = build_walk_overrides(root, exclude_prefixes)?;
    let backstop_root = root.to_path_buf();
    let backstop_prefixes = exclude_prefixes.to_vec();
    let mut files: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut inspected = 0usize;

    for no_ignore_vcs in [false, true] {
        let mut builder = WalkBuilder::new(root);
        builder
            // rg `--hidden`: the crate's toggle is inverted (`hidden(true)`
            // ignores hidden entries); the blocklist globs still prune caches.
            .hidden(false)
            .parents(false)
            .follow_links(false)
            .git_ignore(!no_ignore_vcs)
            .git_global(!no_ignore_vcs)
            .git_exclude(!no_ignore_vcs)
            .overrides(overrides.clone())
            .filter_entry({
                let backstop_root = backstop_root.clone();
                let backstop_prefixes = backstop_prefixes.clone();
                move |entry| should_walk_quick_open_entry(entry, &backstop_root, &backstop_prefixes)
            });

        for entry in builder.build() {
            inspected += 1;
            if inspected.is_multiple_of(CANCEL_CHECK_INTERVAL) && is_cancelled(cancelled) {
                return Err(collection_error());
            }
            let Ok(entry) = entry else {
                continue;
            };
            let file_type = entry.file_type();
            if entry.depth() == 0 || file_type.is_some_and(|file_type| file_type.is_dir()) {
                continue;
            }
            // Symlinked directories are neither traversed nor listed: a Quick
            // Open entry must be openable as a file, and a directory symlink
            // would resolve to a directory. Symlinked files stay listed.
            if file_type.is_some_and(|file_type| file_type.is_symlink()) && entry.path().is_dir() {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(root) else {
                continue;
            };
            let relative = normalize_relative_path(relative);
            if relative.is_empty() || !keep(&relative) || !seen.insert(relative.clone()) {
                continue;
            }
            files.push(relative);
            if max_results.is_some_and(|limit| files.len() >= limit) {
                return Ok(files);
            }
        }
    }
    Ok(files)
}

fn is_cancelled(cancelled: Option<&AtomicBool>) -> bool {
    cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst))
}

fn should_walk_quick_open_entry(
    entry: &ignore::DirEntry,
    root: &Path,
    exclude_prefixes: &[String],
) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let Ok(relative) = entry.path().strip_prefix(root) else {
        return true;
    };
    let relative = normalize_relative_path(relative);
    should_include_quick_open_path(&relative)
        && !should_exclude_quick_open_rel_path(&relative, exclude_prefixes)
}

fn build_walk_overrides(root: &Path, exclude_prefixes: &[String]) -> Result<Override, FsError> {
    let mut builder = OverrideBuilder::new(root);
    for blocked in HIDDEN_DIR_BLOCKLIST {
        builder
            .add(&format!("!**/{}", escape_glob_path(blocked)))
            .map_err(override_error)?;
    }
    for prefix in exclude_prefixes {
        let escaped = escape_glob_path(prefix);
        builder
            .add(&format!("!{escaped}"))
            .map_err(override_error)?;
        builder
            .add(&format!("!{escaped}/**"))
            .map_err(override_error)?;
    }
    builder.build().map_err(override_error)
}

fn override_error(error: ignore::Error) -> FsError {
    FsError::InvalidInput(format!("Invalid traversal glob: {error}"))
}

fn escape_glob_path(relative_path: &str) -> String {
    relative_path
        .split('/')
        .map(escape_glob_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_glob_segment(segment: &str) -> String {
    let mut escaped = String::with_capacity(segment.len());
    for character in segment.chars() {
        if GLOB_METACHARACTERS.contains(&character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("ade-fs-walk-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create unit test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn blocklist_backstop_checks_segments_and_path_entries() {
        for blocked in [
            ".git/config",
            "sub/.git/config",
            "node_modules",
            "a/node_modules/b",
            ".cache/x",
            ".local/share/app",
            "deep/.local/share/app",
        ] {
            assert!(
                !should_include_quick_open_path(blocked),
                "{blocked} should be blocked"
            );
        }
        for allowed in [
            ".hidden.md",
            ".github/workflows/ci.yml",
            ".gitignore",
            ".local/notes.md",
            "src/node_modules_helper.rs",
            "a/node_modulesx/b",
            "",
        ] {
            assert!(
                should_include_quick_open_path(allowed),
                "{allowed} should be allowed"
            );
        }
    }

    #[test]
    fn exclude_prefixes_normalize_and_drop_malformed_values() {
        let root = TestDir::new("exclude-prefixes");
        let root_str = root.path().to_str().unwrap();
        let nested = root.path().join("packages").join("app");
        std::fs::create_dir_all(&nested).unwrap();
        let outside = TestDir::new("exclude-prefixes-outside");

        let values = vec![
            format!("{root_str}/packages/app"),
            format!("{root_str}/packages/app/"),
            format!("{root_str}/packages/other/../app"),
            format!("{root_str}/packages/app2"),
            String::new(),
            "/".to_string(),
            root_str.to_string(),
            outside.path().to_str().unwrap().to_string(),
            "packages/app".to_string(),
        ];
        let prefixes = build_exclude_path_prefixes(root_str, &values);
        assert_eq!(
            prefixes,
            vec!["packages/app".to_string(), "packages/app2".to_string()]
        );
    }

    #[test]
    fn exclude_boundary_requires_a_path_segment() {
        let prefixes = vec!["packages/app".to_string(), "feature[1]".to_string()];
        assert!(should_exclude_quick_open_rel_path(
            "packages/app",
            &prefixes
        ));
        assert!(should_exclude_quick_open_rel_path(
            "packages/app/src/main.rs",
            &prefixes
        ));
        assert!(should_exclude_quick_open_rel_path(
            "feature[1]/x.txt",
            &prefixes
        ));
        assert!(!should_exclude_quick_open_rel_path(
            "packages/app2/b.txt",
            &prefixes
        ));
        assert!(!should_exclude_quick_open_rel_path("packages", &prefixes));
        assert!(!should_exclude_quick_open_rel_path("", &prefixes));
    }

    #[test]
    fn escapes_glob_metacharacters_per_segment() {
        assert_eq!(escape_glob_path("feature[1]"), "feature\\[1\\]");
        assert_eq!(escape_glob_path("a/{b}/c*d?e"), "a/\\{b\\}/c\\*d\\?e");
        assert_eq!(escape_glob_path("plain/path"), "plain/path");
    }

    #[test]
    fn markdown_path_detection_matches_oracle_extensions() {
        for path in ["README.md", "docs/Guide.MDX", "notes.markdown", "a.b.md"] {
            assert!(is_markdown_document_path(path), "{path}");
        }
        for path in [
            "plain.txt",
            "docs/readme.mdx.bak",
            "noextension",
            ".md",
            ".mdx",
        ] {
            assert!(!is_markdown_document_path(path), "{path}");
        }
    }

    #[test]
    fn repeated_token_supersedes_the_previous_run() {
        let registry = CancelRegistry::default();
        let first = registry.begin("token");
        let second = registry.begin("token");
        assert!(first.load(Ordering::SeqCst), "previous run must be aborted");
        assert!(!second.load(Ordering::SeqCst), "new run must start fresh");

        registry.finish("token", &first);
        registry.cancel("token");
        assert!(
            second.load(Ordering::SeqCst),
            "stale finish must not drop the current run's flag"
        );

        registry.finish("token", &second);
        registry.cancel("token");
        assert!(
            registry.tokens.lock().unwrap().is_empty(),
            "finished runs must unregister"
        );
    }

    #[test]
    fn cancel_list_files_latches_a_run_started_from_the_service_registry() {
        let service = FsService::new();
        let guard = CancelGuard::new(service.cancel_registry(), "request-token");
        assert!(!guard.flag().load(Ordering::SeqCst));

        service.cancel_list_files("request-token");
        assert!(
            guard.flag().load(Ordering::SeqCst),
            "cancel_list_files must latch the flag list_files is scanning"
        );

        // Isolation contract: a registry not sourced from the service is only
        // cancellable through itself, never through cancel_list_files.
        let foreign_guard = CancelGuard::new(CancelRegistry::default(), "request-token");
        service.cancel_list_files("request-token");
        assert!(!foreign_guard.flag().load(Ordering::SeqCst));
    }

    #[test]
    fn collect_returns_cancelled_before_walking() {
        let root = TestDir::new("cancel");
        std::fs::write(root.path().join("a.txt"), "x").unwrap();
        let cancelled = AtomicBool::new(true);
        let error =
            collect_quick_open_paths(root.path(), &[], Some(100), Some(&cancelled), &|_| true)
                .unwrap_err();
        assert!(matches!(error, FsError::Cancelled(_)));
        assert_eq!(error.to_string(), FILE_LISTING_CANCELLED_MESSAGE);
    }
}
