//! Claude session deletion: pure path validation plus the executor that
//! re-checks the plan against the filesystem before trashing anything.
//!
//! Two-stage judgement, mirroring the reference (`session-delete-target.ts` /
//! `session-delete.ts`): [`validate_delete_target`] never touches disk, so it
//! cannot tell a transcript from a same-named directory or a symlink planted
//! inside the root — [`execute_delete`] MUST re-check every planned path's
//! kind and realpath containment before removing it.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

use crate::claude::{has_jsonl_extension, PRUNED_DIR_NAMES};
use crate::roots::claude_projects_root;
use crate::trash::trash_path;

/// Stage-one judgement. `Allowed` is a path-only judgement; see the module
/// docs for the caller contract.
pub enum DeleteValidation {
    Allowed {
        transcript: PathBuf,
        /// Companions first (the `<uuid>/` subagent session dir, then
        /// `session-env/<uuid>/`), transcript last: a failed companion leaves
        /// the row on screen to retry from.
        companions: Vec<PathBuf>,
    },
    Rejected {
        reason: &'static str,
    },
}

/// Pure path-only judgement: never touches the filesystem, and never panics —
/// a malformed or hostile `file_path` rejects like any other invalid input.
///
/// Rejection order (aligned with the reference): blank → `invalid-path`;
/// non-Claude agent → `unsupported-agent`; `#`-bearing synthetic identity →
/// `synthetic-path`; `.db` path (this slice's extra OpenCode guard) →
/// `unsupported-agent`; outside `<home>/.claude/projects` →
/// `path-outside-known-roots`; not a discoverable `.jsonl` transcript →
/// `undiscoverable-path`; degenerate session stem → `no-session-directory`.
pub fn validate_delete_target(home: &Path, agent: &str, file_path: &str) -> DeleteValidation {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return DeleteValidation::Rejected {
            reason: "invalid-path",
        };
    }
    if agent != "claude" {
        return DeleteValidation::Rejected {
            reason: "unsupported-agent",
        };
    }
    // A '#' marks an OpenCode SQLite row's synthetic `<dbPath>#<sessionId>`
    // identity — no real file to delete; '#' never appears in a transcript path.
    if trimmed.contains('#') {
        return DeleteValidation::Rejected {
            reason: "synthetic-path",
        };
    }
    // Extra guard for this slice: OpenCode SQLite rows carry the database path,
    // so a `.db` target must never be trashed even if it reaches us as Claude.
    if is_db_path(Path::new(trimmed)) {
        return DeleteValidation::Rejected {
            reason: "unsupported-agent",
        };
    }

    let projects_root = normalize_path(&claude_projects_root(home));
    let resolved = resolve_path(Path::new(trimmed));
    if !is_inside_or_equal(&projects_root, &resolved) {
        return DeleteValidation::Rejected {
            reason: "path-outside-known-roots",
        };
    }
    // The scanner's own accept rule, so a path it would never have surfaced as
    // a session row cannot become a delete target either.
    if !is_discoverable_transcript(&projects_root, &resolved) {
        return DeleteValidation::Rejected {
            reason: "undiscoverable-path",
        };
    }
    let Some(stem) = session_stem(&resolved) else {
        return DeleteValidation::Rejected {
            reason: "no-session-directory",
        };
    };
    let Some(project_dir) = resolved.parent() else {
        return DeleteValidation::Rejected {
            reason: "no-session-directory",
        };
    };

    // `<enc>/<uuid>/`, named after the transcript: taking it rather than just
    // the `subagents/` it holds is what stops an empty `<uuid>/` being left
    // behind. `session-env/<uuid>/` holds that session's generated shell
    // exports; its sibling `file-history/<uuid>/` is deliberately NOT planned —
    // that is the rewind buffer for the user's own files, never ours to trash.
    let companions = vec![
        project_dir.join(&stem),
        claude_session_env_root(home).join(&stem),
    ];
    DeleteValidation::Allowed {
        transcript: resolved,
        companions,
    }
}

/// Run the full two-stage judgement and trash the planned paths, companions
/// first. Never panics: a bad input and an fs error both resolve to the
/// discriminated JSON result the command layer renders.
pub fn execute_delete(home: &Path, agent: &str, file_path: &str) -> Value {
    let (transcript, companions) = match validate_delete_target(home, agent, file_path) {
        DeleteValidation::Rejected { reason } => {
            return json!({ "outcome": "rejected", "agent": agent, "reason": reason });
        }
        DeleteValidation::Allowed {
            transcript,
            companions,
        } => (transcript, companions),
    };

    let projects_root = normalize_path(&claude_projects_root(home));
    let session_env_root = claude_session_env_root(home);
    let mut removals: Vec<Removal> = companions
        .into_iter()
        .map(|path| {
            // Derived from the matched root, so a session-env companion can
            // only ever resolve inside the session-env root.
            let roots = if is_inside_or_equal(&session_env_root, &path) {
                vec![session_env_root.clone()]
            } else {
                vec![projects_root.clone()]
            };
            Removal {
                path,
                kind: RemovalKind::Directory,
                roots,
            }
        })
        .collect();
    removals.push(Removal {
        path: transcript,
        kind: RemovalKind::File,
        roots: vec![projects_root],
    });

    for removal in removals {
        match remove_one(&removal) {
            Ok(()) => {}
            Err(RemoveError::Rejected(reason)) => {
                return json!({ "outcome": "rejected", "agent": agent, "reason": reason });
            }
            Err(RemoveError::Failed(error)) => {
                return json!({ "outcome": "failed", "agent": agent, "error": error });
            }
        }
    }
    json!({ "outcome": "deleted" })
}

fn claude_session_env_root(home: &Path) -> PathBuf {
    normalize_path(&home.join(".claude").join("session-env"))
}

fn is_db_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("db"))
}

/// Whether a scan rooted at `projects_root` would surface this path: a `.jsonl`
/// file (case-insensitive, like the scanner) whose directories between root
/// and file are not pruned. Mirrors `claude.rs`'s walk so "discoverable" and
/// "deletable" cannot drift apart.
fn is_discoverable_transcript(projects_root: &Path, transcript: &Path) -> bool {
    if !has_jsonl_extension(transcript) {
        return false;
    }
    let Ok(relative) = transcript.strip_prefix(projects_root) else {
        return false;
    };
    let Some(parent) = relative.parent() else {
        return false;
    };
    parent.components().all(|component| match component {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            !PRUNED_DIR_NAMES
                .iter()
                .any(|pruned| name.as_ref() == *pruned)
        }
        _ => true,
    })
}

/// The session id, from the transcript's file stem. A degenerate stem (`.` from
/// `..jsonl`, `..`, or empty) would resolve the session dir to the project dir
/// and trash every session in it.
fn session_stem(transcript: &Path) -> Option<OsString> {
    let stem = transcript.file_stem()?;
    let text = stem.to_string_lossy();
    if text.is_empty() || text == "." || text == ".." {
        return None;
    }
    Some(stem.to_os_string())
}

/// Lexical normalization: collapse `.` and `..` without touching the
/// filesystem, mirroring the reference's `path.resolve`.
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

/// Absolute + normalized. Relative inputs resolve against the process cwd,
/// exactly like the reference's `resolve()`.
fn resolve_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return normalize_path(path);
    }
    match std::env::current_dir() {
        Ok(cwd) => normalize_path(&cwd.join(path)),
        Err(_) => normalize_path(path),
    }
}

fn is_inside_or_equal(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}

struct Removal {
    path: PathBuf,
    kind: RemovalKind,
    roots: Vec<PathBuf>,
}

enum RemovalKind {
    File,
    Directory,
}

enum RemoveError {
    Rejected(&'static str),
    Failed(String),
}

/// Trash one planned path, or return the rejection code that stops the plan.
/// A missing path is success everywhere: companions are optional (no subagents
/// dir if none ever ran) and an externally deleted transcript must stay
/// idempotent.
fn remove_one(removal: &Removal) -> Result<(), RemoveError> {
    let metadata = match fs::symlink_metadata(&removal.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(RemoveError::Failed(error.to_string())),
    };
    // symlink_metadata (not metadata) so a symlink is caught here rather than
    // dereferenced: a kind mismatch is a rejection, never a coerced delete.
    let kind_matches = match removal.kind {
        RemovalKind::File => metadata.file_type().is_file(),
        RemovalKind::Directory => metadata.file_type().is_dir(),
    };
    if !kind_matches {
        return Err(RemoveError::Rejected("unexpected-target-kind"));
    }

    let canonical = match fs::canonicalize(&removal.path) {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(RemoveError::Failed(error.to_string())),
    };
    // Catches a symlinked parent that escapes the roots, which the leaf's kind
    // check cannot see. Roots are only lexically normalized, so realpath them
    // too or a session under a symlinked root (~/.claude -> /Volumes/data/
    // .claude) is falsely rejected; a root absent on this host falls back to
    // its text form.
    if canonical != removal.path {
        let inside = removal.roots.iter().any(|root| {
            let real_root = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
            is_inside_or_equal(&real_root, &canonical)
        });
        if !inside {
            return Err(RemoveError::Rejected("path-outside-known-roots"));
        }
    }

    trash_path(&removal.path).map_err(RemoveError::Failed)
}

#[cfg(test)]
mod tests {
    use super::{execute_delete, validate_delete_target, DeleteValidation};
    use serde_json::json;
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn unique_stem(tag: &str) -> String {
        format!("{tag}-{}", std::process::id())
    }

    fn projects_root(home: &Path) -> PathBuf {
        crate::roots::claude_projects_root(home)
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn write_transcript(home: &Path, project: &str, stem: &str) -> PathBuf {
        let path = projects_root(home)
            .join(project)
            .join(format!("{stem}.jsonl"));
        write_file(&path, "{\"type\":\"user\"}\n");
        path
    }

    fn validation_of(home: &Path, file_path: &Path) -> DeleteValidation {
        validate_delete_target(home, "claude", file_path.to_str().unwrap())
    }

    fn rejected_reason(validation: &DeleteValidation) -> &'static str {
        match validation {
            DeleteValidation::Rejected { reason } => reason,
            DeleteValidation::Allowed { .. } => panic!("expected a rejection"),
        }
    }

    fn trash_entries() -> Option<HashSet<String>> {
        let home = std::env::var_os("HOME")?;
        let entries = fs::read_dir(PathBuf::from(home).join(".Trash")).ok()?;
        Some(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect(),
        )
    }

    #[test]
    fn rejects_a_blank_path() {
        let home = tempfile::tempdir().unwrap();
        for blank in ["", "   "] {
            let validation = validate_delete_target(home.path(), "claude", blank);
            assert_eq!(rejected_reason(&validation), "invalid-path");
        }
    }

    #[test]
    fn rejects_an_unsupported_agent() {
        let home = tempfile::tempdir().unwrap();
        let db = home.path().join(".local/share/opencode/opencode.db");
        let validation = validate_delete_target(home.path(), "opencode", db.to_str().unwrap());
        assert_eq!(rejected_reason(&validation), "unsupported-agent");
    }

    #[test]
    fn rejects_a_synthetic_hash_path() {
        let home = tempfile::tempdir().unwrap();
        let path = projects_root(home.path()).join("proj/opencode.db#session-1");
        assert_eq!(
            rejected_reason(&validation_of(home.path(), &path)),
            "synthetic-path"
        );
    }

    #[test]
    fn rejects_a_db_path_even_for_claude() {
        let home = tempfile::tempdir().unwrap();
        let path = projects_root(home.path()).join("proj/opencode.db");
        assert_eq!(
            rejected_reason(&validation_of(home.path(), &path)),
            "unsupported-agent"
        );
    }

    #[test]
    fn rejects_paths_outside_the_projects_root() {
        let home = tempfile::tempdir().unwrap();
        let outside = std::env::temp_dir().join("ade-ai-vault-outside.jsonl");
        assert_eq!(
            rejected_reason(&validation_of(home.path(), &outside)),
            "path-outside-known-roots"
        );
        let escape = projects_root(home.path()).join("../escape.jsonl");
        assert_eq!(
            rejected_reason(&validation_of(home.path(), &escape)),
            "path-outside-known-roots"
        );
    }

    #[test]
    fn rejects_a_file_the_scanner_would_not_discover() {
        let home = tempfile::tempdir().unwrap();
        let path = projects_root(home.path()).join("proj/notes.txt");
        assert_eq!(
            rejected_reason(&validation_of(home.path(), &path)),
            "undiscoverable-path"
        );
    }

    #[test]
    fn rejects_transcripts_under_pruned_directories() {
        let home = tempfile::tempdir().unwrap();
        for relative in [
            "proj/sess-1/subagents/agent-abc.jsonl",
            "proj/session-env/sess-1/env.jsonl",
            "proj/file-history/sess-1/old.jsonl",
        ] {
            let path = projects_root(home.path()).join(relative);
            assert_eq!(
                rejected_reason(&validation_of(home.path(), &path)),
                "undiscoverable-path",
                "{relative}"
            );
        }
    }

    #[test]
    fn rejects_a_degenerate_session_stem() {
        let home = tempfile::tempdir().unwrap();
        for name in ["..jsonl", "...jsonl"] {
            let path = projects_root(home.path()).join("proj").join(name);
            assert_eq!(
                rejected_reason(&validation_of(home.path(), &path)),
                "no-session-directory",
                "{name}"
            );
        }
    }

    #[test]
    fn allows_a_transcript_and_plans_companions_before_it() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("plan");
        let transcript = write_transcript(home.path(), "proj", &stem);
        let session_dir = projects_root(home.path()).join("proj").join(&stem);
        let session_env_dir = home.path().join(".claude/session-env").join(&stem);
        fs::create_dir_all(session_dir.join("subagents")).unwrap();
        fs::create_dir_all(&session_env_dir).unwrap();

        match validation_of(home.path(), &transcript) {
            DeleteValidation::Allowed {
                transcript: planned,
                companions,
            } => {
                assert_eq!(planned, transcript);
                assert_eq!(companions, vec![session_dir, session_env_dir]);
            }
            DeleteValidation::Rejected { reason } => panic!("expected allowed, got {reason}"),
        }
    }

    #[test]
    fn plans_companions_even_when_they_do_not_exist_yet() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("absent");
        let transcript = write_transcript(home.path(), "proj", &stem);

        match validation_of(home.path(), &transcript) {
            DeleteValidation::Allowed { companions, .. } => assert_eq!(
                companions,
                vec![
                    projects_root(home.path()).join("proj").join(&stem),
                    home.path().join(".claude/session-env").join(&stem),
                ]
            ),
            DeleteValidation::Rejected { reason } => panic!("expected allowed, got {reason}"),
        }
    }

    #[test]
    fn never_plans_file_history() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("fh");
        let transcript = write_transcript(home.path(), "proj", &stem);
        fs::create_dir_all(home.path().join(".claude/file-history").join(&stem)).unwrap();

        match validation_of(home.path(), &transcript) {
            DeleteValidation::Allowed { companions, .. } => assert!(companions
                .iter()
                .all(|path| !path.to_string_lossy().contains("file-history"))),
            DeleteValidation::Rejected { reason } => panic!("expected allowed, got {reason}"),
        }
    }

    #[test]
    fn accepts_an_uppercase_jsonl_extension() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("upper");
        let path = projects_root(home.path())
            .join("proj")
            .join(format!("{stem}.JSONL"));
        write_file(&path, "{}\n");

        assert!(matches!(
            validation_of(home.path(), &path),
            DeleteValidation::Allowed { .. }
        ));
    }

    #[test]
    fn deletes_the_session_paths_and_leaves_file_history_alone() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("exec");
        let transcript = write_transcript(home.path(), "proj", &stem);
        let session_dir = projects_root(home.path()).join("proj").join(&stem);
        write_file(&session_dir.join("subagents/agent-abc.jsonl"), "{}\n");
        let session_env_dir = home.path().join(".claude/session-env").join(&stem);
        write_file(&session_env_dir.join("env.sh"), "export A=1\n");
        let file_history_file = home
            .path()
            .join(".claude/file-history")
            .join(&stem)
            .join("version.jsonl");
        write_file(&file_history_file, "keep me\n");

        let before_trash = trash_entries();
        let result = execute_delete(home.path(), "claude", transcript.to_str().unwrap());

        assert_eq!(result, json!({ "outcome": "deleted" }));
        assert!(!transcript.exists());
        assert!(!session_dir.exists());
        assert!(!session_env_dir.exists());
        assert_eq!(fs::read_to_string(&file_history_file).unwrap(), "keep me\n");

        match (before_trash, trash_entries()) {
            (Some(before), Some(after)) => {
                let expected = format!("{stem}.jsonl");
                assert!(
                    after.difference(&before).any(|name| name == &expected),
                    "expected {expected} among new ~/.Trash entries: {:?}",
                    after.difference(&before).collect::<Vec<_>>()
                );
            }
            _ => eprintln!("~/.Trash unreadable; keeping the primary assertions only"),
        }
    }

    #[test]
    fn treats_a_repeat_delete_as_deleted() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("idem");
        let transcript = write_transcript(home.path(), "proj", &stem);
        write_file(
            &projects_root(home.path())
                .join("proj")
                .join(&stem)
                .join("subagents/agent-x.jsonl"),
            "{}\n",
        );
        write_file(
            &home
                .path()
                .join(".claude/session-env")
                .join(&stem)
                .join("env.sh"),
            "x\n",
        );

        let first = execute_delete(home.path(), "claude", transcript.to_str().unwrap());
        assert_eq!(first, json!({ "outcome": "deleted" }));
        let second = execute_delete(home.path(), "claude", transcript.to_str().unwrap());
        assert_eq!(second, json!({ "outcome": "deleted" }));
    }

    #[test]
    fn rejects_a_directory_where_the_transcript_file_is_expected() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("dirkind");
        let path = projects_root(home.path())
            .join("proj")
            .join(format!("{stem}.jsonl"));
        fs::create_dir_all(path.join("inner")).unwrap();

        let result = execute_delete(home.path(), "claude", path.to_str().unwrap());

        assert_eq!(
            result,
            json!({ "outcome": "rejected", "agent": "claude", "reason": "unexpected-target-kind" })
        );
        assert!(path.is_dir());
    }

    #[test]
    fn rejects_a_symlink_where_the_transcript_file_is_expected() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("symlink");
        let target = home.path().join("target.jsonl");
        write_file(&target, "real\n");
        let path = projects_root(home.path())
            .join("proj")
            .join(format!("{stem}.jsonl"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let result = execute_delete(home.path(), "claude", path.to_str().unwrap());

        assert_eq!(
            result,
            json!({ "outcome": "rejected", "agent": "claude", "reason": "unexpected-target-kind" })
        );
        assert!(target.exists());
        assert!(path.symlink_metadata().unwrap().file_type().is_symlink());
    }

    #[test]
    fn stops_before_the_transcript_when_a_companion_has_the_wrong_kind() {
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("companion");
        let transcript = write_transcript(home.path(), "proj", &stem);
        let session_dir = projects_root(home.path()).join("proj").join(&stem);
        write_file(&session_dir, "a file where a directory is planned");

        let result = execute_delete(home.path(), "claude", transcript.to_str().unwrap());

        assert_eq!(
            result,
            json!({ "outcome": "rejected", "agent": "claude", "reason": "unexpected-target-kind" })
        );
        assert!(transcript.exists());
        assert!(session_dir.is_file());
    }

    #[test]
    fn rejects_when_the_real_path_escapes_the_known_roots() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let stem = unique_stem("escape");
        let real = outside.path().join(format!("{stem}.jsonl"));
        write_file(&real, "outside\n");
        let projects = projects_root(home.path());
        fs::create_dir_all(&projects).unwrap();
        let project_dir = projects.join("proj");
        std::os::unix::fs::symlink(outside.path(), &project_dir).unwrap();
        let path = project_dir.join(format!("{stem}.jsonl"));
        assert!(path.exists());

        let result = execute_delete(home.path(), "claude", path.to_str().unwrap());

        assert_eq!(
            result,
            json!({ "outcome": "rejected", "agent": "claude", "reason": "path-outside-known-roots" })
        );
        assert!(real.exists());
    }

    #[test]
    fn accepts_a_session_under_a_symlinked_root() {
        // ~/.claude/projects -> /real/.../projects; the transcript's realpath
        // lands under the real target, which the text-only root would not
        // match. Realpath-ing the root too keeps this legit delete from a
        // false rejection.
        let home = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        let stem = unique_stem("symroot");
        let real_projects = real.path().join("projects");
        let transcript = real_projects.join("proj").join(format!("{stem}.jsonl"));
        write_file(&transcript, "{}\n");
        let lexical_claude = home.path().join(".claude");
        fs::create_dir_all(&lexical_claude).unwrap();
        std::os::unix::fs::symlink(&real_projects, lexical_claude.join("projects")).unwrap();
        let lexical_transcript = lexical_claude
            .join("projects")
            .join("proj")
            .join(format!("{stem}.jsonl"));

        let result = execute_delete(home.path(), "claude", lexical_transcript.to_str().unwrap());

        assert_eq!(result, json!({ "outcome": "deleted" }));
        assert!(!transcript.exists());
        assert!(!lexical_transcript.exists());
    }

    #[test]
    fn reports_failure_when_the_trash_api_fails() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let stem = unique_stem("denied");
        let transcript = write_transcript(home.path(), "proj", &stem);
        let project_dir = transcript.parent().unwrap().to_path_buf();
        fs::set_permissions(&project_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let result = execute_delete(home.path(), "claude", transcript.to_str().unwrap());

        fs::set_permissions(&project_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(result["outcome"], json!("failed"));
        assert_eq!(result["agent"], json!("claude"));
        assert!(result["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()));
        assert!(transcript.exists());
    }

    #[test]
    fn a_rejected_path_is_never_trashed() {
        let home = tempfile::tempdir().unwrap();
        let path = projects_root(home.path()).join("proj/notes.txt");
        write_file(&path, "keep\n");

        let result = execute_delete(home.path(), "claude", path.to_str().unwrap());

        assert_eq!(
            result,
            json!({ "outcome": "rejected", "agent": "claude", "reason": "undiscoverable-path" })
        );
        assert!(path.exists());
    }
}
