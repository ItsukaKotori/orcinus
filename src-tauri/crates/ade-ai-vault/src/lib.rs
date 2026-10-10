//! AI vault core: local agent session history roots, transcript parsers, and
//! the scanner that unifies them for the Agent Session History panel.

pub mod claude;
pub mod opencode;
pub mod roots;

use serde::Serialize;

pub use claude::scan_claude_sessions;
pub use opencode::{opencode_db_paths, scan_opencode_sessions};
pub use roots::{claude_projects_root, opencode_data_dir};

/// A non-fatal scan problem. Rows keep flowing; the panel renders issues as
/// skipped/notice rows instead of failing the whole scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanIssue {
    pub agent: String,
    pub kind: String,
    pub path: String,
    pub message: String,
}
