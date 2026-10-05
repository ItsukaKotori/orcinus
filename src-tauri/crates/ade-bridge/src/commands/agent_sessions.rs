//! providerSession capture via transcript-directory scan (spec §3.3).
//!
//! 2A supports the two agents the PATH probe knows (`claude`, `codex`); any
//! other `agentKind` returns null. A miss degrades to a plain shell restore —
//! never an error.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::errors::BridgeError;
use crate::state::AppState;

const SCAN_TIME_BUDGET: Duration = Duration::from_millis(500);
const CODEX_WALK_MAX_DEPTH: u8 = 4;
const CODEX_HEADER_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderSessionKey {
    SessionId,
    ConversationId,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderSessionMetadata {
    pub key: AgentProviderSessionKey,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionsResolveCaptureArgs {
    pub cwd: String,
    pub agent_kind: String,
    pub window_from_ms: i64,
    pub window_to_ms: i64,
}

pub(crate) type CaptureWindow = (i64, i64);

/// Claude Code stores transcripts in `~/.claude/projects/<munged-cwd>/`, the
/// munge replacing every path separator with `-` (`/Users/a/b` → `-Users-a-b`).
pub(crate) fn munge_claude_project_dir_name(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect()
}

fn modified_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis() as i64,
    )
}

fn in_window(mtime: i64, window: CaptureWindow) -> bool {
    mtime >= window.0 && mtime <= window.1
}

fn latest_jsonl_in_window(
    dir: &Path,
    window: CaptureWindow,
    started: &Instant,
) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(i64, PathBuf)> = None;
    for entry in entries.flatten() {
        if started.elapsed() >= SCAN_TIME_BUDGET {
            break;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(mtime) = modified_ms(&path) else { continue };
        if !in_window(mtime, window) {
            continue;
        }
        if best.as_ref().is_none_or(|(best_mtime, _)| mtime >= *best_mtime) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, path)| path)
}

fn codex_rollout_mentions_cwd(path: &Path, cwd: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut head = vec![0u8; CODEX_HEADER_BYTES];
    let Ok(read) = file.read(&mut head) else {
        return false;
    };
    head.truncate(read);
    String::from_utf8_lossy(&head).contains(cwd)
}

fn latest_codex_rollout_in_window(
    root: &Path,
    cwd: &str,
    window: CaptureWindow,
    started: &Instant,
) -> Option<PathBuf> {
    let mut best: Option<(i64, PathBuf)> = None;
    let mut stack = vec![(root.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if started.elapsed() >= SCAN_TIME_BUDGET {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if started.elapsed() >= SCAN_TIME_BUDGET {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                if depth < CODEX_WALK_MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let is_rollout = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("rollout-") && name.ends_with(".jsonl")
                });
            if !is_rollout {
                continue;
            }
            let Some(mtime) = modified_ms(&path) else { continue };
            if !in_window(mtime, window) || !codex_rollout_mentions_cwd(&path, cwd) {
                continue;
            }
            if best.as_ref().is_none_or(|(best_mtime, _)| mtime >= *best_mtime) {
                best = Some((mtime, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// Pure capture core (testable without a home directory): newest transcript in
/// the window whose location matches `agentKind`'s on-disk convention.
pub(crate) fn resolve_capture(
    home: &Path,
    cwd: &str,
    agent_kind: &str,
    window: CaptureWindow,
) -> Option<AgentProviderSessionMetadata> {
    if window.1 < window.0 || cwd.trim().is_empty() {
        return None;
    }
    let started = Instant::now();
    match agent_kind {
        "claude" => {
            let dir = home
                .join(".claude")
                .join("projects")
                .join(munge_claude_project_dir_name(cwd));
            let transcript = latest_jsonl_in_window(&dir, window, &started)?;
            let id = transcript.file_stem()?.to_str()?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            Some(AgentProviderSessionMetadata {
                key: AgentProviderSessionKey::SessionId,
                id,
                transcript_path: Some(transcript.to_string_lossy().into_owned()),
            })
        }
        "codex" => {
            // Observed layout: ~/.codex/sessions/YYYY/MM/DD/rollout-<local-ts>-<uuid>.jsonl.
            // The CLI resume id is the rollout filename's dashed UUID — exactly
            // the trailing 5 hyphen groups (loose by design — spec §8.3; a miss
            // degrades to shell-only restore).
            let sessions = home.join(".codex").join("sessions");
            let transcript = latest_codex_rollout_in_window(&sessions, cwd, window, &started)?;
            let stem = transcript.file_stem()?.to_str()?;
            let groups: Vec<&str> = stem.split('-').collect();
            let id = if groups.len() >= 5 {
                groups[groups.len() - 5..].join("-")
            } else {
                stem.to_string()
            };
            let id = id.trim().to_string();
            if id.is_empty() {
                return None;
            }
            Some(AgentProviderSessionMetadata {
                key: AgentProviderSessionKey::SessionId,
                id,
                transcript_path: Some(transcript.to_string_lossy().into_owned()),
            })
        }
        _ => None,
    }
}

/// Transcript scan runs off the async runtime (directory walks can stall on
/// network volumes); the 500ms budget bounds the worst case.
#[tauri::command]
#[specta::specta]
pub async fn agent_sessions_resolve_capture(
    state: State<'_, AppState>,
    args: AgentSessionsResolveCaptureArgs,
) -> Result<Option<AgentProviderSessionMetadata>, BridgeError> {
    let home = state.home.clone();
    crate::commands::run_blocking(move || {
        Ok(resolve_capture(
            Path::new(&home),
            &args.cwd,
            &args.agent_kind,
            (args.window_from_ms, args.window_to_ms),
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    fn write_with_mtime(path: &Path, contents: &str, mtime_unix: i64) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
        // `mtime_unix` is milliseconds since the epoch, matching the ms window
        // the scan compares against; `from_unix_time` takes (seconds, nanos).
        let time = filetime::FileTime::from_unix_time(
            mtime_unix.div_euclid(1000),
            (mtime_unix.rem_euclid(1000) * 1_000_000) as u32,
        );
        filetime::set_file_mtime(path, time).unwrap();
    }

    const WINDOW: CaptureWindow = (1_000, 2_000);

    #[test]
    fn munge_replaces_separators_with_dashes() {
        assert_eq!(munge_claude_project_dir_name("/Users/a/b"), "-Users-a-b");
        // Only separators munge; the Windows drive colon passes through.
        assert_eq!(munge_claude_project_dir_name("C:\\Users\\x"), "C:-Users-x");
    }

    #[test]
    fn claude_scan_picks_newest_transcript_in_window() {
        let home = tempfile::tempdir().unwrap();
        let dir = home
            .path()
            .join(".claude/projects/-Users-a-b");
        write_with_mtime(&dir.join("old.jsonl"), "{}", 1_100);
        write_with_mtime(&dir.join("new.jsonl"), "{}", 1_500);
        write_with_mtime(&dir.join("too-new.jsonl"), "{}", 9_999);
        write_with_mtime(&dir.join("too-old.jsonl"), "{}", 5);

        let found = resolve_capture(home.path(), "/Users/a/b", "claude", WINDOW).unwrap();
        assert_eq!(found.key, AgentProviderSessionKey::SessionId);
        assert_eq!(found.id, "new");
        assert!(found.transcript_path.unwrap().ends_with("new.jsonl"));
    }

    #[test]
    fn claude_scan_returns_none_when_dir_or_window_misses() {
        let home = tempfile::tempdir().unwrap();
        assert!(resolve_capture(home.path(), "/Users/other", "claude", WINDOW).is_none());
        let dir = home.path().join(".claude/projects/-Users-a-b");
        write_with_mtime(&dir.join("a.jsonl"), "{}", 9_999);
        assert!(resolve_capture(home.path(), "/Users/a/b", "claude", WINDOW).is_none());
    }

    #[test]
    fn codex_scan_walks_date_dirs_and_requires_cwd_match() {
        let home = tempfile::tempdir().unwrap();
        let base = home.path().join(".codex/sessions/2026/10/01");
        write_with_mtime(
            &base.join("rollout-2026-10-01T10-00-00-aaaaaaaa-1111-2222-3333-444444444444.jsonl"),
            r#"{"cwd":"/repo/one"}"#,
            1_200,
        );
        // Newer rollout for a different cwd must not win.
        write_with_mtime(
            &base.join("rollout-2026-10-01T11-00-00-bbbbbbbb-1111-2222-3333-444444444444.jsonl"),
            r#"{"cwd":"/repo/two"}"#,
            1_800,
        );

        let found = resolve_capture(home.path(), "/repo/one", "codex", WINDOW).unwrap();
        // The CLI resume id is the whole dashed UUID, not a fragment.
        assert_eq!(found.id, "aaaaaaaa-1111-2222-3333-444444444444");
        assert!(found.transcript_path.unwrap().contains("aaaaaaaa"));
    }

    #[test]
    fn unknown_agent_kind_and_degenerate_windows_return_none() {
        let home = tempfile::tempdir().unwrap();
        assert!(resolve_capture(home.path(), "/repo", "gemini", WINDOW).is_none());
        assert!(resolve_capture(home.path(), "/repo", "claude", (2_000, 1_000)).is_none());
        assert!(resolve_capture(home.path(), "  ", "claude", WINDOW).is_none());
    }

    #[test]
    fn scan_budget_bounds_the_walk() {
        let home = tempfile::tempdir().unwrap();
        // A fifo-style unreadable path would hang a naive walk; here we only
        // assert a normal empty scan returns quickly and misses.
        let started = Instant::now();
        assert!(resolve_capture(home.path(), "/repo", "codex", WINDOW).is_none());
        assert!(started.elapsed() < StdDuration::from_secs(2));
    }
}
