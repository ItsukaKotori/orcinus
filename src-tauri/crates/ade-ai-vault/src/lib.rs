//! AI vault core: local agent session history roots, transcript parsers, and
//! the scanner that unifies them for the Agent Session History panel.

pub mod cache;
pub mod claude;
pub mod delete;
pub mod opencode;
pub mod roots;
pub mod scanner;
pub mod trash;

use serde::Serialize;

pub use cache::ListCache;
pub use claude::scan_claude_sessions;
pub use delete::{execute_delete, validate_delete_target, DeleteValidation};
pub use opencode::{opencode_db_paths, scan_opencode_sessions};
pub use roots::{claude_projects_root, opencode_data_dir};
pub use scanner::{
    scan_sessions, CancelFlag, CancelRegistry, ScanRequest, VaultConfig, DEFAULT_SCAN_LIMIT,
};
pub use trash::trash_path;

/// A non-fatal scan problem. Rows keep flowing; the panel renders issues as
/// skipped/notice rows instead of failing the whole scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanIssue {
    pub agent: String,
    pub kind: String,
    pub path: String,
    pub message: String,
}
