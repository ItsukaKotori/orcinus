//! Append-only audit trail for host-API mutations performed on a plugin's
//! behalf.
//!
//! Ported from `orca:src/main/plugins/plugin-audit-log.ts`: one JSONL document
//! per plugin at `<data_dir>/<qualifiedKey>/audit.log` (spec §3.3), rotating
//! to `audit.log.1` once the current segment would exceed 10 MiB. Entries
//! carry `{ts, actor: "plugin:<key>", method, summary, outcome}` where
//! `outcome` is `attempt`/`ok`/`error`; `summary` is bounded and never full
//! params. Reads are tail-oriented: the last `limit` well-formed lines across
//! the rotated segment and the current one.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::discovery::is_qualified_plugin_key;
use crate::types::{PluginError, INVALID_PARAMS, IO_ERROR};

pub const AUDIT_LOG_FILE_NAME: &str = "audit.log";
pub const AUDIT_LOG_ROTATED_FILE_NAME: &str = "audit.log.1";
pub const AUDIT_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const AUDIT_ACTOR_PREFIX: &str = "plugin:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOutcome {
    Attempt,
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub ts: i64,
    pub actor: String,
    pub method: String,
    pub summary: String,
    pub outcome: AuditOutcome,
}

/// A parsed audit line as read back from disk.
pub type LogLine = AuditRecord;

fn plugin_key_from_actor(actor: &str) -> Result<&str, PluginError> {
    let plugin_key = actor.strip_prefix(AUDIT_ACTOR_PREFIX).unwrap_or_default();
    if plugin_key.is_empty() || !is_qualified_plugin_key(plugin_key) {
        return Err(PluginError::new(
            INVALID_PARAMS,
            format!("invalid audit actor: {actor}"),
        ));
    }
    Ok(plugin_key)
}

/// Per-scope audit log rooted at the `plugin-data` directory; each record is
/// routed to its plugin's own file through the actor key.
#[derive(Debug)]
pub struct AuditLog {
    data_dir: PathBuf,
    max_bytes: u64,
    write_lock: Mutex<()>,
}

impl AuditLog {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self::with_max_bytes(data_dir, AUDIT_LOG_MAX_BYTES)
    }

    /// `max_bytes` exists for tests and mirrors the oracle's `options.maxBytes`.
    pub fn with_max_bytes(data_dir: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Self {
            data_dir: data_dir.into(),
            max_bytes,
            write_lock: Mutex::new(()),
        }
    }

    /// Appends one entry, rotating the segment first when it would overflow.
    pub fn record(&self, record: AuditRecord) -> Result<(), PluginError> {
        let plugin_key = plugin_key_from_actor(&record.actor)?;
        let line = serde_json::to_string(&record).map_err(|error| {
            PluginError::new(IO_ERROR, format!("failed to encode audit entry: {error}"))
        })?;
        let line = format!("{line}\n");
        let directory = self.data_dir.join(plugin_key);
        fs::create_dir_all(&directory).map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to create the audit log directory: {error}"),
            )
        })?;
        let path = directory.join(AUDIT_LOG_FILE_NAME);
        let rotated = directory.join(AUDIT_LOG_ROTATED_FILE_NAME);

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let existing = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if existing > 0 && existing + line.len() as u64 > self.max_bytes {
            let _ = fs::remove_file(&rotated);
            match fs::rename(&path, &rotated) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(PluginError::new(
                        IO_ERROR,
                        format!("failed to rotate the plugin audit log: {error}"),
                    ))
                }
            }
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                PluginError::new(
                    IO_ERROR,
                    format!("failed to open the plugin audit log: {error}"),
                )
            })?;
        file.write_all(line.as_bytes()).map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to append to the plugin audit log: {error}"),
            )
        })
    }

    /// The last `limit` well-formed entries for `plugin_key`, oldest first.
    /// Missing logs and unparseable lines are skipped, never fatal.
    pub fn read_tail(&self, plugin_key: &str, limit: usize) -> Vec<LogLine> {
        if limit == 0 || !is_qualified_plugin_key(plugin_key) {
            return Vec::new();
        }
        let directory = self.data_dir.join(plugin_key);
        let mut text =
            fs::read_to_string(directory.join(AUDIT_LOG_ROTATED_FILE_NAME)).unwrap_or_default();
        text.push_str(&fs::read_to_string(directory.join(AUDIT_LOG_FILE_NAME)).unwrap_or_default());
        let mut lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
        if lines.len() > limit {
            lines.drain(..lines.len() - limit);
        }
        lines
            .into_iter()
            .filter_map(|line| serde_json::from_str::<LogLine>(line).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const KEY: &str = "orca-samples.demo";
    const OTHER: &str = "acme.other";

    fn record(ts: i64) -> AuditRecord {
        AuditRecord {
            ts,
            actor: format!("{AUDIT_ACTOR_PREFIX}{KEY}"),
            method: "storage.set".to_string(),
            summary: format!("key={ts}"),
            outcome: AuditOutcome::Ok,
        }
    }

    #[test]
    fn appends_jsonl_under_the_plugin_data_directory() {
        let dir = tempdir().unwrap();
        let log = AuditLog::new(dir.path());
        log.record(AuditRecord {
            ts: 1,
            actor: format!("{AUDIT_ACTOR_PREFIX}{KEY}"),
            method: "storage.set".to_string(),
            summary: "key=theme".to_string(),
            outcome: AuditOutcome::Attempt,
        })
        .unwrap();
        log.record(record(2)).unwrap();

        let raw = fs::read_to_string(dir.path().join(KEY).join(AUDIT_LOG_FILE_NAME)).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(
            first,
            serde_json::json!({
                "ts": 1,
                "actor": format!("{AUDIT_ACTOR_PREFIX}{KEY}"),
                "method": "storage.set",
                "summary": "key=theme",
                "outcome": "attempt"
            })
        );

        let parsed = log.read_tail(KEY, 10);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].outcome, AuditOutcome::Attempt);
        assert_eq!(parsed[1], record(2));
    }

    #[test]
    fn rotates_at_the_byte_cap_and_reads_across_the_boundary() {
        let dir = tempdir().unwrap();
        let log = AuditLog::with_max_bytes(dir.path(), 240);
        for ts in 0..8 {
            log.record(record(ts)).unwrap();
        }

        let directory = dir.path().join(KEY);
        assert!(fs::metadata(directory.join(AUDIT_LOG_FILE_NAME))
            .unwrap()
            .is_file());
        assert!(fs::metadata(directory.join(AUDIT_LOG_ROTATED_FILE_NAME))
            .unwrap()
            .is_file());
        let recent = log.read_tail(KEY, 3);
        assert_eq!(
            recent.iter().map(|line| line.ts).collect::<Vec<_>>(),
            vec![5, 6, 7]
        );
    }

    #[test]
    fn read_tail_isolates_plugins_and_skips_corrupt_lines() {
        let dir = tempdir().unwrap();
        let log = AuditLog::new(dir.path());
        log.record(record(1)).unwrap();
        log.record(AuditRecord {
            ts: 9,
            actor: format!("{AUDIT_ACTOR_PREFIX}{OTHER}"),
            method: "secrets.set".to_string(),
            summary: "key=token".to_string(),
            outcome: AuditOutcome::Error,
        })
        .unwrap();

        let path = dir.path().join(KEY).join(AUDIT_LOG_FILE_NAME);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{not json").unwrap();

        let own = log.read_tail(KEY, 10);
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].actor, format!("{AUDIT_ACTOR_PREFIX}{KEY}"));
        let other = log.read_tail(OTHER, 10);
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].method, "secrets.set");
    }

    #[test]
    fn read_tail_limit_zero_or_missing_log_is_empty() {
        let dir = tempdir().unwrap();
        let log = AuditLog::new(dir.path());
        assert!(log.read_tail(KEY, 0).is_empty());
        assert!(log.read_tail(KEY, 5).is_empty());
        assert!(log.read_tail("demo", 5).is_empty());
    }

    #[test]
    fn rejects_malformed_audit_actors_before_touching_disk() {
        let dir = tempdir().unwrap();
        let log = AuditLog::new(dir.path());
        for actor in [
            "not-a-plugin",
            "plugin:",
            "plugin:demo",
            "plugin:bad key.demo",
        ] {
            let error = log
                .record(AuditRecord {
                    ts: 1,
                    actor: actor.to_string(),
                    method: "storage.set".to_string(),
                    summary: "k".to_string(),
                    outcome: AuditOutcome::Ok,
                })
                .unwrap_err();
            assert_eq!(error.code(), INVALID_PARAMS);
        }
        assert!(!dir.path().join("not-a-plugin").exists());
        assert!(!dir.path().join("plugin:").exists());
    }
}
