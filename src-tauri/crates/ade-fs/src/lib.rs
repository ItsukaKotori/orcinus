pub mod auth;
pub mod mutate;
pub mod read;
pub mod search;
pub mod walk;
pub mod watch;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use auth::PathAuthRegistry;
pub use read::{
    compare_file_names, sort_dir_entries, DirEntry, FileContent, FileStat, PathExistence,
    BINARY_PROBE_BYTES, MAX_PREVIEWABLE_BINARY_SIZE, MAX_TEXT_FILE_SIZE, PATH_EXISTENCE_BATCH_MAX,
    PREVIEWABLE_BINARY_MIME_TYPES,
};
pub use search::{
    clamp_max_results, split_search_glob_patterns, to_glob_pattern, SearchFileResult, SearchMatch,
    SearchOptions, SearchResult, MAX_FILE_SIZE, MAX_LINE_CONTENT_CHARS, MAX_RESULTS_CAP,
    MAX_RESULTS_DEFAULT, PER_FILE_MAX_MATCHES, SEARCH_TIMEOUT_MS,
};
pub use walk::{
    build_exclude_path_prefixes, is_markdown_document_path, should_exclude_quick_open_rel_path,
    should_include_quick_open_path, CancelRegistry, MarkdownDocument,
    FILE_LISTING_CANCELLED_MESSAGE, HIDDEN_DIR_BLOCKLIST,
};
pub use watch::{
    coalesce_events, FsChangeEvent, FsChangeKind, FsChangedPayload, FsWatcher, ManualWatchClock,
    RawEvent, SystemWatchClock, WatchClock, DIRECTORY_STAT_CONCURRENCY, MAX_BATCHED_WATCHER_EVENTS,
    WATCHER_IGNORE_DIRS, WATCHER_TEARDOWN_GRACE_MS, WATCH_BATCH_MAX_WAIT_MS,
    WATCH_BATCH_TRAILING_MS,
};

pub const PATH_ACCESS_DENIED_MESSAGE: &str =
    "Access denied: path resolves outside allowed directories. If this blocks a legitimate workflow, please file a GitHub issue.";

#[derive(Debug, Error)]
pub enum FsError {
    #[error("{}", PATH_ACCESS_DENIED_MESSAGE)]
    PathAccessDenied,
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("A file or folder named '{0}' already exists in this location")]
    AlreadyExists(String),
    #[error("File too large: {size_mb:.1}MB exceeds {limit_mb}MB limit")]
    FileTooLarge { size_mb: f64, limit_mb: u64 },
    #[error("Failed to move to trash: {0}")]
    Trash(String),
    #[error("{0}")]
    Cancelled(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Default)]
pub struct FsService {
    auth: PathAuthRegistry,
    cancel: CancelRegistry,
}

impl FsService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a repo/folder-workspace root; called when one is added.
    pub fn authorize_root(&self, path: &str) -> Result<(), FsError> {
        self.auth.authorize_root(path)
    }

    /// Drop a previously registered root; called when one is removed.
    pub fn revoke_root(&self, path: &str) -> Result<(), FsError> {
        self.auth.revoke_root(path)
    }

    /// Grant access to a path outside registered roots (`authorizeExternalPath`).
    pub fn authorize_external(&self, path: &str) -> Result<(), FsError> {
        self.auth.authorize_external(path)
    }

    /// Canonicalize and authorize a path, following symlinks.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, FsError> {
        self.auth.resolve(path)
    }

    /// Canonicalize the parent but keep the leaf so delete/rename/copy act on a
    /// symlink itself instead of its destination.
    pub fn resolve_preserving_symlink(&self, path: &str) -> Result<PathBuf, FsError> {
        self.auth.resolve_preserving_symlink(path)
    }

    pub fn is_authorized_root(&self, path: &Path) -> bool {
        self.auth.is_authorized_root(path)
    }
}
