//! Claude session title resolution for the Agent Session History panel.
//!
//! Mirrors the reference's claude-only slice (`session-title-resolver.ts` /
//! `session-title-file-reader.ts`) without its worker and parse-cache layers:
//! requests are capped, a transcript path under the projects root is honored
//! when it validates, otherwise the transcript is located by session id under
//! the projects root, and a request that cannot produce a title is silently
//! dropped — never an error.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::claude::{has_jsonl_extension, read_claude_title};
use crate::roots::claude_projects_root;

/// Per-call request cap, mirroring the shared TS
/// `AI_VAULT_SESSION_TITLE_REQUEST_MAX_COUNT`.
pub const AI_VAULT_SESSION_TITLE_REQUEST_MAX_COUNT: usize = 64;
/// Reference request-normalization bounds (`session-title-resolver.ts`).
const SESSION_ID_MAX_LENGTH: usize = 512;
const TRANSCRIPT_PATH_MAX_LENGTH: usize = 32_768;

/// One `ai_vault_resolve_session_titles` request, camelCase on the wire to
/// match `src/shared/ai-vault-session-title.ts`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleRequest {
    pub agent: String,
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
}

/// Resolve up to 64 request titles into `{"titles": [{agent, sessionId,
/// title}]}`. Only `claude` is supported: requests for other agents
/// contribute nothing.
pub fn resolve_titles(home: &Path, requests: &[TitleRequest]) -> Value {
    let projects_root = claude_projects_root(home);
    let titles: Vec<Value> = requests
        .iter()
        .take(AI_VAULT_SESSION_TITLE_REQUEST_MAX_COUNT)
        .filter_map(|request| resolve_one(&projects_root, request))
        .collect();
    json!({ "titles": titles })
}

fn resolve_one(projects_root: &Path, request: &TitleRequest) -> Option<Value> {
    if request.agent != "claude" {
        return None;
    }
    let session_id = request.session_id.trim();
    if !is_safe_session_id(session_id) {
        return None;
    }
    let transcript = locate_transcript(
        projects_root,
        request.transcript_path.as_deref(),
        session_id,
    )?;
    let title = read_claude_title(&transcript)?;
    Some(json!({
        "agent": request.agent,
        "sessionId": session_id,
        "title": title,
    }))
}

/// The session id is joined into a file name, so anything that could traverse
/// or leave the projects root — separators, `.`/`..`, absolute paths, control
/// characters — is rejected. Mirrors the reference's request normalization.
fn is_safe_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= SESSION_ID_MAX_LENGTH
        && !session_id.contains('/')
        && !session_id.contains('\\')
        && session_id != "."
        && session_id != ".."
        && !session_id
            .chars()
            .any(|code| code <= '\u{1f}' || code == '\u{7f}')
}

/// A request-provided transcript path is used when it is a `.jsonl` file under
/// the projects root; otherwise the transcript is located by session id.
fn locate_transcript(
    projects_root: &Path,
    transcript_path: Option<&str>,
    session_id: &str,
) -> Option<PathBuf> {
    let root = normalize_path(projects_root);
    let explicit = transcript_path
        .map(str::trim)
        .filter(|path| !path.is_empty() && path.len() <= TRANSCRIPT_PATH_MAX_LENGTH)
        .map(Path::new)
        .map(normalize_path)
        .filter(|path| has_jsonl_extension(path) && is_inside_or_equal(&root, path));
    explicit.or_else(|| find_session_transcript(projects_root, session_id))
}

/// `<projects_root>/<any project dir>/<session_id>.jsonl`, deterministic by
/// sorted project directory. Direct children only, like the reference layout:
/// subagent transcripts live deeper and never resolve a session title.
fn find_session_transcript(projects_root: &Path, session_id: &str) -> Option<PathBuf> {
    let file_name = format!("{session_id}.jsonl");
    let mut project_dirs: Vec<PathBuf> = fs::read_dir(projects_root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    project_dirs.sort();
    project_dirs
        .into_iter()
        .map(|dir| dir.join(&file_name))
        .find(|candidate| candidate.is_file())
}

fn is_inside_or_equal(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}

/// Lexical normalization: collapse `.` and `..` without touching the
/// filesystem, mirroring the reference's `path.resolve` (same helper as
/// `delete.rs`).
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(name) => normalized.push(name),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn titled_records(title: &str) -> Vec<Value> {
        vec![json!({ "type": "custom-title", "customTitle": title })]
    }

    fn write_transcript(home: &Path, project: &str, file_name: &str, records: &[Value]) -> PathBuf {
        let path = claude_projects_root(home).join(project).join(file_name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, body).unwrap();
        path
    }

    fn request(agent: &str, session_id: &str) -> TitleRequest {
        TitleRequest {
            agent: agent.to_string(),
            session_id: session_id.to_string(),
            transcript_path: None,
        }
    }

    fn titles(result: &Value) -> &[Value] {
        result["titles"].as_array().unwrap()
    }

    #[test]
    fn resolves_a_claude_session_title_by_session_id() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "-repo-app",
            "session-a.jsonl",
            &titled_records("Named session"),
        );

        let result = resolve_titles(home.path(), &[request("claude", "session-a")]);

        assert_eq!(
            result,
            json!({
                "titles": [{
                    "agent": "claude",
                    "sessionId": "session-a",
                    "title": "Named session"
                }]
            })
        );
    }

    #[test]
    fn prefers_a_valid_request_transcript_path_over_the_session_id_lookup() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "project-a",
            "session-a.jsonl",
            &titled_records("Lookup title"),
        );
        let explicit = write_transcript(
            home.path(),
            "project-b",
            "renamed.jsonl",
            &titled_records("Explicit title"),
        );
        let mut explicit_request = request("claude", "session-a");
        explicit_request.transcript_path = Some(explicit.to_string_lossy().into_owned());

        let result = resolve_titles(home.path(), &[explicit_request]);

        assert_eq!(titles(&result)[0]["title"], json!("Explicit title"));
    }

    #[test]
    fn ignores_request_paths_outside_the_projects_root() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "project-a",
            "session-a.jsonl",
            &titled_records("Inside title"),
        );
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("session-a.jsonl");
        fs::write(
            &outside_file,
            titled_records("Outside title")[0].to_string(),
        )
        .unwrap();

        let mut outside_request = request("claude", "session-a");
        outside_request.transcript_path = Some(outside_file.to_string_lossy().into_owned());
        let mut unknown_request = request("claude", "unknown");
        unknown_request.transcript_path = Some(outside_file.to_string_lossy().into_owned());

        let result = resolve_titles(home.path(), &[outside_request, unknown_request]);

        let titles = titles(&result);
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0]["title"], json!("Inside title"));
    }

    #[test]
    fn ignores_non_transcript_request_paths() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "project-a",
            "session-a.jsonl",
            &titled_records("Lookup title"),
        );
        let notes = claude_projects_root(home.path())
            .join("project-a")
            .join("notes.txt");
        fs::write(&notes, titled_records("Notes title")[0].to_string()).unwrap();
        let mut notes_request = request("claude", "session-a");
        notes_request.transcript_path = Some(notes.to_string_lossy().into_owned());

        let result = resolve_titles(home.path(), &[notes_request]);

        assert_eq!(titles(&result)[0]["title"], json!("Lookup title"));
    }

    #[test]
    fn omits_unknown_sessions_without_failing() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "project-a",
            "known.jsonl",
            &titled_records("Known title"),
        );

        let result = resolve_titles(
            home.path(),
            &[request("claude", "missing"), request("claude", "known")],
        );

        let titles = titles(&result);
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0]["sessionId"], json!("known"));
        assert_eq!(titles[0]["title"], json!("Known title"));
    }

    #[test]
    fn omits_a_transcript_that_names_no_title() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "project-a",
            "bare.jsonl",
            &[json!({
                "type": "assistant",
                "sessionId": "bare",
                "message": { "model": "claude-sonnet-4-5", "content": "no title here" }
            })],
        );

        let result = resolve_titles(home.path(), &[request("claude", "bare")]);

        assert!(titles(&result).is_empty());
    }

    #[test]
    fn only_claude_requests_produce_titles() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "project-a",
            "shared.jsonl",
            &titled_records("Claude title"),
        );

        let result = resolve_titles(
            home.path(),
            &[
                request("codex", "shared"),
                request("opencode", "shared"),
                request("claude", "shared"),
            ],
        );

        let titles = titles(&result);
        assert_eq!(titles.len(), 1);
        assert_eq!(
            titles[0],
            json!({
                "agent": "claude",
                "sessionId": "shared",
                "title": "Claude title"
            })
        );
    }

    #[test]
    fn caps_requests_at_the_max_count() {
        let home = tempfile::tempdir().unwrap();
        for index in 0..65 {
            write_transcript(
                home.path(),
                "project-a",
                &format!("cap-{index:02}.jsonl"),
                &titled_records(&format!("Title {index}")),
            );
        }
        let requests: Vec<TitleRequest> = (0..65)
            .map(|index| request("claude", &format!("cap-{index:02}")))
            .collect();

        let result = resolve_titles(home.path(), &requests);

        let titles = titles(&result);
        assert_eq!(titles.len(), 64);
        assert_eq!(titles[0]["sessionId"], json!("cap-00"));
        assert_eq!(titles[63]["sessionId"], json!("cap-63"));
    }

    #[test]
    fn rejects_unsafe_session_ids() {
        let home = tempfile::tempdir().unwrap();
        write_transcript(
            home.path(),
            "project-a",
            "escape.jsonl",
            &titled_records("Escape title"),
        );

        for session_id in ["", "   ", "..", "../escape", "a/b", "a\\b", "bad\u{7f}id"] {
            let result = resolve_titles(home.path(), &[request("claude", session_id)]);
            assert!(
                titles(&result).is_empty(),
                "session id {session_id:?} must not resolve"
            );
        }
    }

    #[test]
    fn deserializes_camel_case_requests() {
        let parsed: TitleRequest = serde_json::from_value(json!({
            "agent": "claude",
            "sessionId": "session-a",
            "transcriptPath": "/tmp/a.jsonl"
        }))
        .unwrap();
        assert_eq!(parsed.agent, "claude");
        assert_eq!(parsed.session_id, "session-a");
        assert_eq!(parsed.transcript_path.as_deref(), Some("/tmp/a.jsonl"));

        let minimal: TitleRequest =
            serde_json::from_value(json!({ "agent": "claude", "sessionId": "session-a" })).unwrap();
        assert_eq!(minimal.transcript_path, None);
    }
}
