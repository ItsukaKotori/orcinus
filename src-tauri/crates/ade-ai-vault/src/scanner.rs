//! Unified scanner for local agent session history.
//!
//! The two source parsers (`claude`, `opencode`) each return recency-capped
//! candidate lists. This module concatenates them, resolves OpenCode
//! legacy-file/SQLite duplicate rows, applies the final recency cap plus the
//! scope guarantee, and reports cancellation as a flag instead of an error.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::claude::{self, system_time_to_iso};
use crate::opencode;
use crate::roots::{claude_projects_root, opencode_data_dir};

/// Result-page size when the caller does not pin one (mirrors the shared TS
/// `DEFAULT_AI_VAULT_SCAN_LIMIT`).
pub const DEFAULT_SCAN_LIMIT: usize = 1000;
/// Candidate rows read per source before the final limit is applied.
const CANDIDATE_LIMIT_FACTOR: usize = 2;
/// Renderer contract: scope paths past this count are ignored.
const SCOPE_PATHS_MAX_COUNT: usize = 64;

/// Roots and environment overrides the scan resolves its sources from.
#[derive(Debug, Clone)]
pub struct VaultConfig {
    pub home: PathBuf,
    pub xdg_data_home: Option<String>,
    pub opencode_db: Option<String>,
}

impl VaultConfig {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            xdg_data_home: None,
            opencode_db: None,
        }
    }

    /// Resolve the overridable roots from the process environment.
    pub fn from_env(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            xdg_data_home: std::env::var("XDG_DATA_HOME").ok(),
            opencode_db: std::env::var("OPENCODE_DB").ok(),
        }
    }
}

/// One `ai_vault_list_sessions` request. `force` and `request_token` are
/// consumed by the cache/cancel layers, not by the scan itself.
#[derive(Debug, Clone, Default)]
pub struct ScanRequest {
    pub scope_paths: Vec<String>,
    pub limit: usize,
    pub force: bool,
    pub request_token: Option<String>,
}

#[derive(Clone, Default)]
pub struct CancelFlag {
    flag: Arc<AtomicBool>,
}

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// In-flight list-scan cancellations keyed by the renderer's request token.
/// Re-registering the same token cancels the superseded run first, so a stale
/// scan can never publish a result after its caller gave up. Mirrors
/// `ade-bridge`'s `GitCancelRegistry`.
pub struct CancelRegistry {
    tokens: Mutex<HashMap<String, CancelFlag>>,
}

impl CancelRegistry {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Register one scan and return its live cancellation flag.
    pub fn register(&self, token: &str) -> CancelFlag {
        let mut tokens = lock(&self.tokens);
        if let Some(previous) = tokens.get(token) {
            previous.cancel();
        }
        let fresh = CancelFlag::new();
        tokens.insert(token.to_string(), fresh.clone());
        fresh
    }

    /// Drop one registration once its scan returned (success or failure).
    pub fn finish(&self, token: &str) {
        lock(&self.tokens).remove(token);
    }

    /// Set and drop one registration; `false` when the scan already finished.
    pub fn cancel(&self, token: &str) -> bool {
        match lock(&self.tokens).remove(token) {
            Some(flag) => {
                flag.cancel();
                true
            }
            None => false,
        }
    }
}

impl Default for CancelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Scan Claude + OpenCode history into an `AiVaultListResult` JSON body
/// (`{sessions, issues, scannedAt, cancelled?}`). Cancellation returns the
/// flag instead of throwing.
pub fn scan_sessions(config: &VaultConfig, request: &ScanRequest, cancel: &CancelFlag) -> Value {
    let scanned_at = system_time_to_iso(SystemTime::now());
    let limit = effective_limit(request.limit);
    // Candidates are read per source at twice the final page so the merge can
    // still fill it after dedupe; the source parsers never see the raw limit.
    let candidate_limit = limit.saturating_mul(CANDIDATE_LIMIT_FACTOR).max(limit);

    if cancel.is_cancelled() {
        return cancelled_result(scanned_at);
    }

    let claude_root = claude_projects_root(&config.home);
    let (mut rows, mut issues, claude_cancelled) =
        claude::scan_claude_sessions_cancellable(&claude_root, candidate_limit, Some(cancel));
    if claude_cancelled {
        return cancelled_result(scanned_at);
    }

    let opencode_root = opencode_data_dir(&config.home, config.xdg_data_home.as_deref());
    let (opencode_rows, opencode_issues, opencode_cancelled) =
        opencode::scan_opencode_sessions_cancellable(
            &opencode_root,
            candidate_limit,
            config.opencode_db.as_deref(),
            Some(cancel),
        );
    if opencode_cancelled || cancel.is_cancelled() {
        return cancelled_result(scanned_at);
    }
    rows.extend(opencode_rows);
    issues.extend(opencode_issues);

    let scope_paths = normalize_scope_paths(&request.scope_paths);
    json!({
        "sessions": finalize_rows(rows, limit, &scope_paths),
        "issues": issues,
        "scannedAt": scanned_at,
    })
}

pub(crate) fn effective_limit(limit: usize) -> usize {
    if limit > 0 {
        limit
    } else {
        DEFAULT_SCAN_LIMIT
    }
}

pub(crate) fn normalize_scope_paths(raw: &[String]) -> Vec<String> {
    let mut scopes: Vec<String> = raw
        .iter()
        .map(|scope| scope.trim().to_string())
        .filter(|scope| !scope.is_empty())
        .collect();
    scopes.truncate(SCOPE_PATHS_MAX_COUNT);
    scopes
}

fn cancelled_result(scanned_at: String) -> Value {
    json!({
        "sessions": [],
        "issues": [],
        "scannedAt": scanned_at,
        "cancelled": true,
    })
}

/// Dedupe (an OpenCode SQLite row supersedes its same-id legacy file), sort by
/// `modifiedAt` desc, then cap at `limit` while keeping Claude rows whose cwd is
/// inside a scope path past the cap (scope guarantee).
fn finalize_rows(rows: Vec<Value>, limit: usize, scope_paths: &[String]) -> Vec<Value> {
    let mut rows = dedupe_rows(rows);
    rows.sort_by(|left, right| modified_at(right).cmp(modified_at(left)));
    rows.into_iter()
        .enumerate()
        .filter(|(index, row)| *index < limit || is_scoped_claude_row(row, scope_paths))
        .map(|(_, row)| row)
        .collect()
}

fn dedupe_rows(rows: Vec<Value>) -> Vec<Value> {
    let mut groups: Vec<Vec<Value>> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for row in rows {
        let key = (
            row["agent"].as_str().unwrap_or_default().to_string(),
            row["sessionId"].as_str().unwrap_or_default().to_string(),
        );
        match index.get(&key) {
            Some(position) => groups[*position].push(row),
            None => {
                index.insert(key, groups.len());
                groups.push(vec![row]);
            }
        }
    }
    let mut output = Vec::new();
    for group in groups {
        let supersedes = group.iter().any(is_sqlite_row);
        output.extend(
            group
                .into_iter()
                .filter(|row| !supersedes || is_sqlite_row(row)),
        );
    }
    output
}

fn is_sqlite_row(row: &Value) -> bool {
    row["filePath"]
        .as_str()
        .is_some_and(|path| path.ends_with(".db"))
}

fn modified_at(row: &Value) -> &str {
    row["modifiedAt"].as_str().unwrap_or("")
}

fn is_scoped_claude_row(row: &Value, scope_paths: &[String]) -> bool {
    if row["agent"].as_str() != Some("claude") {
        return false;
    }
    let Some(cwd) = row["cwd"].as_str() else {
        return false;
    };
    scope_paths
        .iter()
        .any(|scope| is_path_inside_or_equal(scope, cwd))
}

/// POSIX containment with a path-separator boundary, so `/repo/app` does not
/// match `/repo/apple`.
fn is_path_inside_or_equal(scope: &str, candidate: &str) -> bool {
    let scope = scope.trim().trim_end_matches('/');
    if scope.is_empty() {
        return candidate.starts_with('/');
    }
    candidate == scope
        || candidate
            .strip_prefix(scope)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, UNIX_EPOCH};

    const BASE_MS: u64 = 1_700_000_000_000;

    fn iso(millis: u64) -> String {
        system_time_to_iso(UNIX_EPOCH + Duration::from_millis(millis))
    }

    fn write_file(root: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    fn set_mtime(path: &Path, millis: u64) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_millis(millis))
            .unwrap();
    }

    fn write_claude_session(
        home: &Path,
        project: &str,
        session_id: &str,
        cwd: &str,
        modified_ms: u64,
    ) -> PathBuf {
        let record = json!({
            "type": "user",
            "sessionId": session_id,
            "timestamp": "2026-05-01T10:00:00.000Z",
            "cwd": cwd,
            "message": { "role": "user", "content": "hello" }
        });
        let path = write_file(
            home,
            &format!(".claude/projects/{project}/{session_id}.jsonl"),
            &record.to_string(),
        );
        set_mtime(&path, modified_ms);
        path
    }

    fn write_opencode_legacy(home: &Path, session_id: &str, modified_ms: u64) -> PathBuf {
        let record = json!({ "id": session_id, "title": format!("Legacy {session_id}") });
        let path = write_file(
            home,
            &format!(".local/share/opencode/storage/session/project/{session_id}.json"),
            &record.to_string(),
        );
        set_mtime(&path, modified_ms);
        path
    }

    fn write_opencode_db(home: &Path, sessions: &[(&str, i64, i64)]) -> PathBuf {
        let path = home.join(".local/share/opencode/opencode.db");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            );",
        )
        .unwrap();
        for (id, created, updated) in sessions {
            conn.execute(
                "INSERT INTO session VALUES (?1, ?2, ?3)",
                rusqlite::params![id, created, updated],
            )
            .unwrap();
        }
        drop(conn);
        path
    }

    fn request(limit: usize, scope_paths: &[&str]) -> ScanRequest {
        ScanRequest {
            scope_paths: scope_paths.iter().map(|scope| scope.to_string()).collect(),
            limit,
            force: false,
            request_token: None,
        }
    }

    fn scan(home: &Path, request: &ScanRequest) -> Value {
        scan_sessions(&VaultConfig::new(home), request, &CancelFlag::new())
    }

    fn sessions(result: &Value) -> &[Value] {
        result["sessions"].as_array().unwrap()
    }

    fn session_ids(result: &Value) -> Vec<String> {
        sessions(result)
            .iter()
            .map(|row| row["sessionId"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn merges_dedupes_and_sorts_rows_by_modified_at_desc() {
        let home = tempfile::tempdir().unwrap();
        write_claude_session(
            home.path(),
            "-repo-app",
            "claude-recent",
            "/repo/app",
            BASE_MS + 3_000,
        );
        write_opencode_db(
            home.path(),
            &[("ses_dual", 1_777_634_000_000, (BASE_MS + 4_000) as i64)],
        );
        write_opencode_legacy(home.path(), "ses_dual", BASE_MS + 5_000);
        write_opencode_legacy(home.path(), "ses_legacy_only", BASE_MS + 2_000);
        write_claude_session(
            home.path(),
            "-repo-other",
            "claude-recent",
            "/repo/other",
            BASE_MS + 1_500,
        );
        write_claude_session(
            home.path(),
            "-repo-third",
            "claude-recent",
            "/repo/third",
            BASE_MS + 1_000,
        );

        let result = scan(home.path(), &request(100, &[]));

        assert!(result.get("cancelled").is_none());
        let dual: Vec<&Value> = sessions(&result)
            .iter()
            .filter(|row| row["sessionId"] == json!("ses_dual"))
            .collect();
        assert_eq!(dual.len(), 1);
        assert!(dual[0]["filePath"]
            .as_str()
            .unwrap()
            .ends_with("opencode.db"));
        assert_eq!(
            sessions(&result)
                .iter()
                .filter(|row| row["sessionId"] == json!("claude-recent"))
                .count(),
            3
        );

        let timeline: Vec<(String, String)> = sessions(&result)
            .iter()
            .map(|row| {
                (
                    row["sessionId"].as_str().unwrap().to_string(),
                    row["modifiedAt"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(
            timeline,
            vec![
                ("ses_dual".to_string(), iso(BASE_MS + 4_000)),
                ("claude-recent".to_string(), iso(BASE_MS + 3_000)),
                ("ses_legacy_only".to_string(), iso(BASE_MS + 2_000)),
                ("claude-recent".to_string(), iso(BASE_MS + 1_500)),
                ("claude-recent".to_string(), iso(BASE_MS + 1_000)),
            ]
        );
    }

    #[test]
    fn scope_paths_keep_in_scope_claude_rows_past_the_limit() {
        let home = tempfile::tempdir().unwrap();
        write_claude_session(
            home.path(),
            "-other-0",
            "recent-out-0",
            "/other/0",
            BASE_MS + 3_000,
        );
        write_claude_session(
            home.path(),
            "-other-1",
            "recent-out-1",
            "/other/1",
            BASE_MS + 2_500,
        );
        write_claude_session(
            home.path(),
            "-repo-app-packages-ui",
            "old-in-scope",
            "/repo/app/packages/ui",
            BASE_MS + 1_000,
        );
        write_claude_session(
            home.path(),
            "-repo-apple",
            "old-out-sibling",
            "/repo/apple",
            BASE_MS + 500,
        );

        let unscoped = scan(home.path(), &request(2, &[]));
        assert_eq!(session_ids(&unscoped), vec!["recent-out-0", "recent-out-1"]);

        let scoped = scan(home.path(), &request(2, &["/repo/app"]));
        assert_eq!(
            session_ids(&scoped),
            vec!["recent-out-0", "recent-out-1", "old-in-scope"]
        );
    }

    #[test]
    fn a_cancelled_scan_returns_cancelled_without_failing() {
        let home = tempfile::tempdir().unwrap();
        write_claude_session(home.path(), "-repo-app", "any", "/repo/app", BASE_MS);

        let cancel = CancelFlag::new();
        cancel.cancel();
        let result = scan_sessions(&VaultConfig::new(home.path()), &request(10, &[]), &cancel);

        assert_eq!(result["cancelled"], json!(true));
        assert_eq!(result["sessions"], json!([]));
        assert_eq!(result["issues"], json!([]));
        assert!(result["scannedAt"]
            .as_str()
            .is_some_and(|stamp| !stamp.is_empty()));
    }

    #[test]
    fn cancellation_lands_mid_scan_and_discards_partial_work() {
        let home = tempfile::tempdir().unwrap();
        write_opencode_db(
            home.path(),
            &[("ses_locked", 1_777_634_000_000, 1_777_634_001_000)],
        );
        let db_path = home.path().join(".local/share/opencode/opencode.db");
        let writer = Connection::open(&db_path).unwrap();
        writer.execute_batch("BEGIN EXCLUSIVE;").unwrap();

        let cancel = CancelFlag::new();
        let scan_cancel = cancel.clone();
        let scan_home = home.path().to_path_buf();
        let handle = thread::spawn(move || {
            let request = request(10, &[]);
            scan_sessions(&VaultConfig::new(scan_home), &request, &scan_cancel)
        });

        thread::sleep(Duration::from_millis(50));
        cancel.cancel();
        writer.execute_batch("ROLLBACK;").unwrap();
        drop(writer);

        let result = handle.join().unwrap();
        assert_eq!(result["cancelled"], json!(true));
        assert_eq!(result["sessions"], json!([]));
    }

    #[test]
    fn issues_from_both_sources_are_passed_through() {
        let home = tempfile::tempdir().unwrap();
        write_file(home.path(), ".claude/projects", "not a directory");
        write_file(
            home.path(),
            ".local/share/opencode/opencode.db",
            "definitely not sqlite",
        );

        let result = scan(home.path(), &request(10, &[]));

        let issues = result["issues"].as_array().unwrap();
        assert_eq!(issues.len(), 2);
        let agents: Vec<&str> = issues
            .iter()
            .map(|issue| issue["agent"].as_str().unwrap())
            .collect();
        assert!(agents.contains(&"claude"));
        assert!(agents.contains(&"opencode"));
        assert!(issues.iter().all(|issue| issue["kind"] == json!("scope")));
    }

    #[test]
    fn cancel_registry_tracks_tokens_like_the_git_registry() {
        let registry = CancelRegistry::new();
        let first = registry.register("token-1");
        assert!(!first.is_cancelled());
        assert!(!registry.cancel("unknown"));
        assert!(registry.cancel("token-1"));
        assert!(first.is_cancelled());

        let superseded = registry.register("token-2");
        let current = registry.register("token-2");
        assert!(superseded.is_cancelled());
        assert!(!current.is_cancelled());
        registry.finish("token-2");
        assert!(!current.is_cancelled());
        assert!(!registry.cancel("token-2"));
    }
}
