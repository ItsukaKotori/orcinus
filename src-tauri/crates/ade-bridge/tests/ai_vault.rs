//! Integration coverage for the aiVault command surface: service assembly over
//! an injected `VaultConfig`, argument mapping, cache/cancel wiring, and the
//! five `*_impl` wrappers against a temp home with Claude + OpenCode fixtures.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use ade_ai_vault::VaultConfig;
use ade_bridge::commands::ai_vault::{
    cancel_list_sessions_impl, delete_session_impl, list_sessions_impl,
    prepare_session_resume_impl, resolve_titles_impl, AiVaultDeleteSessionArgs, AiVaultListArgs,
    AiVaultPrepareSessionResumeArgs, AiVaultService, AiVaultSessionTitlesArgs,
};
use serde_json::{json, Value};

const BASE_MS: u64 = 1_700_000_000_000;

fn write_file(root: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = root.join(relative);
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
    session_id: &str,
    title: &str,
    cwd: &str,
    modified_ms: u64,
) -> PathBuf {
    let body = format!(
        "{}\n{}\n",
        json!({ "type": "custom-title", "customTitle": title }),
        json!({
            "type": "user",
            "sessionId": session_id,
            "timestamp": "2026-05-01T10:00:00.000Z",
            "cwd": cwd,
            "message": { "role": "user", "content": "hello" }
        })
    );
    let path = write_file(
        home,
        &format!(".claude/projects/-repo-app/{session_id}.jsonl"),
        &body,
    );
    set_mtime(&path, modified_ms);
    path
}

fn write_opencode_legacy(
    home: &Path,
    session_id: &str,
    title: &str,
    directory: &str,
    modified_ms: u64,
) -> PathBuf {
    let record = json!({
        "id": session_id,
        "title": title,
        "directory": directory,
        "time": { "created": 1_777_634_000_000i64, "updated": 1_777_634_001_000i64 }
    });
    let path = write_file(
        home,
        &format!(".local/share/opencode/storage/session/bridge/{session_id}.json"),
        &record.to_string(),
    );
    set_mtime(&path, modified_ms);
    path
}

/// A file where the OpenCode SQLite database would be: the scanner reports it
/// as a scope issue while every other source keeps flowing.
fn write_broken_opencode_db(home: &Path) -> PathBuf {
    write_file(home, ".local/share/opencode/opencode.db", "not sqlite")
}

fn service(home: &Path) -> AiVaultService {
    AiVaultService::new(VaultConfig::new(home))
}

fn list_args(value: Value) -> AiVaultListArgs {
    serde_json::from_value(value).unwrap()
}

fn delete_args(value: Value) -> AiVaultDeleteSessionArgs {
    serde_json::from_value(value).unwrap()
}

fn value_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}

fn session_pairs(result: &Value) -> Vec<(String, String)> {
    result["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["agent"].as_str().unwrap().to_string(),
                row["sessionId"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn lists_both_sources_and_passes_through_issues() {
    let home = tempfile::tempdir().unwrap();
    write_claude_session(
        home.path(),
        "session-a",
        "Bridge Claude",
        "/repo/app",
        BASE_MS + 2_000,
    );
    write_opencode_legacy(
        home.path(),
        "ses_bridge",
        "OpenCode bridge",
        "/repo/opencode",
        BASE_MS + 1_000,
    );
    write_broken_opencode_db(home.path());
    let service = service(home.path());

    let result = list_sessions_impl(&service, &list_args(json!({}))).unwrap();
    let value = value_of(&result);

    assert_eq!(
        session_pairs(&value),
        vec![
            ("claude".to_string(), "session-a".to_string()),
            ("opencode".to_string(), "ses_bridge".to_string()),
        ]
    );
    let issues = value["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["agent"], json!("opencode"));
    assert_eq!(issues[0]["kind"], json!("scope"));
    assert!(issues[0]["path"]
        .as_str()
        .is_some_and(|path| path.ends_with("opencode.db")));
    assert!(value["scannedAt"]
        .as_str()
        .is_some_and(|stamp| !stamp.is_empty()));
    assert!(value.get("cancelled").is_none());
}

#[test]
fn unlimited_is_accepted_and_ignored() {
    let home = tempfile::tempdir().unwrap();
    write_claude_session(
        home.path(),
        "session-a",
        "Bridge Claude",
        "/repo/app",
        BASE_MS + 2_000,
    );
    write_opencode_legacy(
        home.path(),
        "ses_bridge",
        "OpenCode bridge",
        "/repo/opencode",
        BASE_MS + 1_000,
    );
    let service = service(home.path());

    // `unlimited: true` must not lift the limit (spec §5: ignored, still 1000).
    let with_unlimited = list_sessions_impl(
        &service,
        &list_args(json!({ "limit": 1, "unlimited": true })),
    )
    .unwrap();
    let without_unlimited =
        list_sessions_impl(&service, &list_args(json!({ "limit": 1 }))).unwrap();

    assert_eq!(with_unlimited.sessions.len(), 1);
    assert_eq!(value_of(&with_unlimited), value_of(&without_unlimited));
}

#[test]
fn resolves_claude_titles_and_ignores_the_host_scope_field() {
    let home = tempfile::tempdir().unwrap();
    write_claude_session(
        home.path(),
        "session-a",
        "Bridge Claude",
        "/repo/app",
        BASE_MS,
    );
    let service = service(home.path());
    let args: AiVaultSessionTitlesArgs = serde_json::from_value(json!({
        "executionHostScope": "local",
        "requests": [{ "agent": "claude", "sessionId": "session-a" }]
    }))
    .unwrap();

    let result = resolve_titles_impl(&service, &args).unwrap();

    assert_eq!(
        value_of(&result),
        json!({
            "titles": [{
                "agent": "claude",
                "sessionId": "session-a",
                "title": "Bridge Claude"
            }]
        })
    );
}

#[test]
fn rejects_unsupported_or_outside_session_targets() {
    let home = tempfile::tempdir().unwrap();
    let service = service(home.path());
    let projects = home.path().join(".claude/projects/-repo-app");
    let transcript = write_claude_session(
        home.path(),
        "session-a",
        "Bridge Claude",
        "/repo/app",
        BASE_MS,
    );
    let db = write_broken_opencode_db(home.path());
    let notes = write_file(
        home.path(),
        ".claude/projects/-repo-app/notes.txt",
        "keep\n",
    );
    let pruned = write_file(
        home.path(),
        ".claude/projects/-repo-app/session-a/subagents/agent-x.jsonl",
        "{}\n",
    );

    let cases: Vec<(Value, &str)> = vec![
        (
            json!({ "agent": "claude", "filePath": "   " }),
            "invalid-path",
        ),
        (
            json!({ "agent": "opencode", "filePath": db.to_string_lossy() }),
            "unsupported-agent",
        ),
        (
            json!({
                "agent": "claude",
                "filePath": home.path().join("outside.jsonl").to_string_lossy()
            }),
            "path-outside-known-roots",
        ),
        (
            json!({
                "agent": "claude",
                "filePath": format!("{}#session-a", db.to_string_lossy())
            }),
            "synthetic-path",
        ),
        (
            json!({
                "agent": "claude",
                "filePath": projects.join("opencode.db").to_string_lossy()
            }),
            "unsupported-agent",
        ),
        (
            json!({ "agent": "claude", "filePath": notes.to_string_lossy() }),
            "undiscoverable-path",
        ),
        (
            json!({ "agent": "claude", "filePath": pruned.to_string_lossy() }),
            "undiscoverable-path",
        ),
    ];

    for (args, reason) in cases {
        let args = delete_args(args);
        let result = delete_session_impl(&service, &args).unwrap();
        assert_eq!(
            value_of(&result),
            json!({ "outcome": "rejected", "agent": args.agent, "reason": reason }),
            "{reason}"
        );
    }
    // Nothing was trashed on any rejected plan.
    assert!(transcript.exists());
    assert!(db.exists());
    assert!(notes.exists());
    assert!(pruned.exists());
}

#[test]
fn deletes_a_claude_session_to_the_trash_and_invalidates_the_cache() {
    let home = tempfile::tempdir().unwrap();
    let service = service(home.path());
    let stem = format!("bridge-delete-{}", std::process::id());
    let transcript = write_claude_session(home.path(), &stem, "Delete me", "/repo/app", BASE_MS);
    let companion = write_file(
        home.path(),
        &format!(".claude/projects/-repo-app/{stem}/subagents/agent-1.jsonl"),
        "{}\n",
    );
    let session_env = write_file(
        home.path(),
        &format!(".claude/session-env/{stem}/env.sh"),
        "export A=1\n",
    );

    let before = list_sessions_impl(&service, &list_args(json!({}))).unwrap();
    assert_eq!(before.sessions.len(), 1);

    let result = delete_session_impl(
        &service,
        &delete_args(json!({ "agent": "claude", "filePath": transcript.to_string_lossy() })),
    )
    .unwrap();

    assert_eq!(value_of(&result), json!({ "outcome": "deleted" }));
    assert!(!transcript.exists());
    assert!(!companion.exists());
    assert!(!session_env.exists());

    // The successful delete must invalidate the 60s list cache: a plain repeat
    // list (no `force`) sees the row gone.
    let after = list_sessions_impl(&service, &list_args(json!({}))).unwrap();
    assert!(after.sessions.is_empty());
    assert!(after.issues.is_empty());
}

#[test]
fn cancelling_an_unknown_token_is_a_noop() {
    let home = tempfile::tempdir().unwrap();
    write_claude_session(
        home.path(),
        "session-a",
        "Bridge Claude",
        "/repo/app",
        BASE_MS,
    );
    let service = service(home.path());

    assert!(!cancel_list_sessions_impl(&service, "missing-token"));

    let result =
        list_sessions_impl(&service, &list_args(json!({ "requestToken": "fresh" }))).unwrap();
    assert_eq!(result.sessions.len(), 1);
    // The registry dropped the finished scan, so a late cancel is a no-op too.
    assert!(!cancel_list_sessions_impl(&service, "fresh"));
}

#[test]
fn prepare_resume_reports_the_real_codex_home() {
    let args: AiVaultPrepareSessionResumeArgs = serde_json::from_value(json!({
        "agent": "claude",
        "filePath": "/home/u/.claude/projects/-repo-app/session-a.jsonl",
        "codexHome": "/managed/codex-accounts/acct/home",
        "sessionId": "session-a",
        "executionHostId": "local"
    }))
    .unwrap();

    let result = prepare_session_resume_impl(&args).unwrap();

    assert_eq!(value_of(&result), json!({ "useRealCodexHome": true }));
}
