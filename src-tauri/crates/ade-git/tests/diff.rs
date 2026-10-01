use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ade_git::diff::{
    diff, diff_refs, DiffSide, GitDiffResult, LargeDiffRenderLimit, LargeDiffRenderLimitLimits,
    LargeDiffRenderLimitReason, MAX_GIT_SHOW_BYTES,
};

/// A minimal 1x1 transparent PNG (68 bytes).
const PNG_V1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x00, 0x02, 0x00,
    0x00, 0x05, 0x00, 0x01, 0x7a, 0x5e, 0xab, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x82,
];
/// Same PNG with the IEND CRC flipped, so the bytes differ from v1.
const PNG_V2: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x00, 0x02, 0x00,
    0x00, 0x05, 0x00, 0x01, 0x7a, 0x5e, 0xab, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x83,
];
const PNG_V1_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=";
const PNG_V2_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYIM=";

const TIMEOUT: Duration = Duration::from_secs(30);

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ade-git-{name}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// git with the user's environment stripped, so host config cannot leak into fixtures.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ade Test")
        .env("GIT_AUTHOR_EMAIL", "ade-test@example.com")
        .env("GIT_COMMITTER_NAME", "Ade Test")
        .env("GIT_COMMITTER_EMAIL", "ade-test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let output = git_command(dir).args(args).output().expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            message,
        ],
    );
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    String::from_utf8(git(dir, &["rev-parse", rev]).stdout)
        .expect("oid is UTF-8")
        .trim()
        .to_string()
}

/// `main` branch with one commit containing `README.md` (`a\n`).
fn init_repo_with_commit(dir: &TempDir) -> PathBuf {
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(repo.join("README.md"), "a\n").expect("write README");
    commit_all(&repo, "init");
    repo
}

fn repo_str(repo: &Path) -> &str {
    repo.to_str().expect("repo path is UTF-8")
}

#[test]
fn unstaged_diff_returns_index_and_worktree_contents() {
    let dir = TempDir::new("diff-unstaged");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();

    let result = diff(repo_str(&repo), "README.md", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            original_is_binary,
            modified_is_binary,
            large_diff_render_limit,
        } => {
            assert_eq!(original_content, "a\n");
            assert_eq!(modified_content, "b\n");
            assert!(!original_is_binary);
            assert!(!modified_is_binary);
            assert!(large_diff_render_limit.is_none());
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn staged_diff_returns_head_and_index_contents() {
    let dir = TempDir::new("diff-staged");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    git(&repo, &["add", "README.md"]);

    let result = diff(repo_str(&repo), "README.md", true, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => {
            assert_eq!(original_content, "a\n");
            assert_eq!(modified_content, "b\n");
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn compare_against_head_uses_head_as_original_for_unstaged() {
    let dir = TempDir::new("diff-against-head");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("README.md"), "b\n").unwrap();
    git(&repo, &["add", "README.md"]);
    std::fs::write(repo.join("README.md"), "c\n").unwrap();

    let result = diff(repo_str(&repo), "README.md", false, true, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => {
            assert_eq!(
                original_content, "a\n",
                "compareAgainstHead pins the original at HEAD"
            );
            assert_eq!(
                modified_content, "c\n",
                "the working tree is always the modified side"
            );
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn binary_file_returns_binary_kind_with_empty_contents() {
    let dir = TempDir::new("diff-binary");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
    commit_all(&repo, "add bin");
    std::fs::write(repo.join("bin.dat"), [0u8, 9, 9, 9]).unwrap();

    let result = diff(repo_str(&repo), "bin.dat", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Binary {
            original_content,
            modified_content,
            is_image,
            mime_type,
            modified_deleted,
            original_is_binary,
            modified_is_binary,
        } => {
            assert!(original_is_binary || modified_is_binary);
            assert!(original_is_binary && modified_is_binary);
            assert_eq!(
                original_content, "",
                "non-previewable binary carries no content"
            );
            assert_eq!(modified_content, "");
            assert!(is_image.is_none());
            assert!(mime_type.is_none());
            assert!(modified_deleted.is_none());
        }
        other => panic!("expected binary diff, got {other:?}"),
    }
}

#[test]
fn png_within_cap_returns_base64_and_mime_type() {
    let dir = TempDir::new("diff-png");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("image.png"), PNG_V1).unwrap();
    commit_all(&repo, "add png v1");
    std::fs::write(repo.join("image.png"), PNG_V2).unwrap();

    let result = diff(repo_str(&repo), "image.png", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Binary {
            original_content,
            modified_content,
            is_image,
            mime_type,
            original_is_binary,
            modified_is_binary,
            ..
        } => {
            assert!(original_is_binary && modified_is_binary);
            assert_eq!(mime_type.as_deref(), Some("image/png"));
            assert_eq!(
                is_image,
                Some(true),
                "the legacy isImage flag is set for previewable binaries"
            );
            assert_eq!(original_content, PNG_V1_BASE64);
            assert_eq!(modified_content, PNG_V2_BASE64);
        }
        other => panic!("expected binary diff, got {other:?}"),
    }
}

#[test]
fn oversized_blob_is_treated_as_binary_not_error() {
    let dir = TempDir::new("diff-oversized");
    let repo = init_repo_with_commit(&dir);
    let big = vec![b'a'; (MAX_GIT_SHOW_BYTES + 1) as usize];
    std::fs::write(repo.join("big.txt"), &big).unwrap();
    git(&repo, &["add", "big.txt"]);

    let result = diff(repo_str(&repo), "big.txt", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Binary {
            original_content,
            modified_content,
            original_is_binary,
            modified_is_binary,
            ..
        } => {
            assert!(original_is_binary && modified_is_binary);
            assert_eq!(original_content, "");
            assert_eq!(modified_content, "");
        }
        other => panic!("expected binary diff for oversized blob, got {other:?}"),
    }
}

#[test]
fn untracked_text_file_returns_empty_original_content() {
    let dir = TempDir::new("diff-untracked");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("new.txt"), "hello\n").unwrap();

    let result = diff(repo_str(&repo), "new.txt", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => {
            assert_eq!(original_content, "");
            assert_eq!(modified_content, "hello\n");
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn deleted_text_file_returns_empty_modified_content() {
    let dir = TempDir::new("diff-delete-text");
    let repo = init_repo_with_commit(&dir);
    std::fs::remove_file(repo.join("README.md")).unwrap();

    let result = diff(repo_str(&repo), "README.md", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => {
            assert_eq!(original_content, "a\n");
            assert_eq!(modified_content, "");
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn deleted_binary_file_marks_modified_deleted() {
    let dir = TempDir::new("diff-delete-binary");
    let repo = init_repo_with_commit(&dir);
    std::fs::write(repo.join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
    commit_all(&repo, "add bin");
    std::fs::remove_file(repo.join("bin.dat")).unwrap();

    let result = diff(repo_str(&repo), "bin.dat", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Binary {
            original_is_binary,
            modified_content,
            modified_deleted,
            ..
        } => {
            assert!(original_is_binary);
            assert_eq!(modified_content, "");
            assert_eq!(
                modified_deleted,
                Some(true),
                "a proven deletion is marked so previewers do not read a read failure as one"
            );
        }
        other => panic!("expected binary diff, got {other:?}"),
    }
}

#[test]
fn diff_refs_reads_both_sides_at_revisions() {
    let dir = TempDir::new("diff-refs");
    let repo = init_repo_with_commit(&dir);
    let first = rev_parse(&repo, "HEAD");
    std::fs::write(repo.join("README.md"), "B\n").unwrap();
    commit_all(&repo, "second");
    let second = rev_parse(&repo, "HEAD");

    let left = DiffSide::Rev {
        rev: first,
        path: "README.md".to_string(),
    };
    let right = DiffSide::Rev {
        rev: second,
        path: "README.md".to_string(),
    };
    let result = diff_refs(repo_str(&repo), &left, &right, "README.md", None).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => {
            assert_eq!(original_content, "a\n");
            assert_eq!(modified_content, "B\n");
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn diff_refs_uses_old_path_for_left_side() {
    let dir = TempDir::new("diff-refs-rename");
    let repo = init_repo_with_commit(&dir);
    git(&repo, &["mv", "README.md", "renamed.md"]);
    std::fs::write(repo.join("renamed.md"), "renamed content\n").unwrap();
    commit_all(&repo, "rename");

    let parent = rev_parse(&repo, "HEAD~1");
    let head = rev_parse(&repo, "HEAD");
    let left = DiffSide::Rev {
        rev: parent,
        path: "renamed.md".to_string(),
    };
    let right = DiffSide::Rev {
        rev: head,
        path: "renamed.md".to_string(),
    };
    let result = diff_refs(
        repo_str(&repo),
        &left,
        &right,
        "renamed.md",
        Some("README.md"),
    )
    .unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => {
            assert_eq!(
                original_content, "a\n",
                "old_path overrides the left read path"
            );
            assert_eq!(modified_content, "renamed content\n");
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn diff_refs_empty_left_side_reads_as_empty() {
    let dir = TempDir::new("diff-refs-empty");
    let repo = init_repo_with_commit(&dir);
    let head = rev_parse(&repo, "HEAD");
    let right = DiffSide::Rev {
        rev: head,
        path: "README.md".to_string(),
    };

    let result = diff_refs(repo_str(&repo), &DiffSide::Empty, &right, "README.md", None).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            ..
        } => {
            assert_eq!(original_content, "");
            assert_eq!(modified_content, "a\n");
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn large_line_count_sets_line_count_render_limit() {
    let dir = TempDir::new("diff-lines");
    let repo = init_repo_with_commit(&dir);
    let lines = "x\n".repeat(120_001);
    std::fs::write(repo.join("huge.txt"), &lines).unwrap();

    let result = diff(repo_str(&repo), "huge.txt", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            large_diff_render_limit,
            ..
        } => {
            assert_eq!(original_content, "");
            assert_eq!(modified_content, "");
            match large_diff_render_limit.expect("render limit") {
                LargeDiffRenderLimit::Limited {
                    limited,
                    reason,
                    line_counts,
                    line_counts_are_minimum,
                    character_count,
                    limits,
                } => {
                    assert!(limited);
                    assert_eq!(reason, LargeDiffRenderLimitReason::LineCount);
                    let counts =
                        line_counts.expect("line counts are known below the character cap");
                    assert_eq!(counts.original, 0);
                    assert_eq!(counts.modified, 120_001);
                    let minimums = line_counts_are_minimum.expect("exceeded sides are minimums");
                    assert!(!minimums.original);
                    assert!(minimums.modified);
                    assert_eq!(character_count, 2 * 120_001);
                    assert_eq!(limits.max_lines_per_side, 120_000);
                    assert_eq!(limits.max_combined_characters, 6_000_000);
                }
                other => panic!("expected a limited render result, got {other:?}"),
            }
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn large_character_count_sets_character_count_render_limit() {
    let dir = TempDir::new("diff-chars");
    let repo = init_repo_with_commit(&dir);
    let chars = "a".repeat(6_000_001);
    std::fs::write(repo.join("huge.txt"), &chars).unwrap();

    let result = diff(repo_str(&repo), "huge.txt", false, false, TIMEOUT).unwrap();

    match result {
        GitDiffResult::Text {
            original_content,
            modified_content,
            large_diff_render_limit,
            ..
        } => {
            assert_eq!(original_content, "");
            assert_eq!(modified_content, "");
            match large_diff_render_limit.expect("render limit") {
                LargeDiffRenderLimit::Limited {
                    reason,
                    line_counts,
                    line_counts_are_minimum,
                    character_count,
                    ..
                } => {
                    assert_eq!(reason, LargeDiffRenderLimitReason::CharacterCount);
                    assert!(
                        line_counts.is_none(),
                        "the character cap short-circuits before line counting"
                    );
                    assert!(line_counts_are_minimum.is_none());
                    assert_eq!(character_count, 6_000_001);
                }
                other => panic!("expected a limited render result, got {other:?}"),
            }
        }
        other => panic!("expected text diff, got {other:?}"),
    }
}

#[test]
fn text_result_serializes_with_kind_and_camel_case() {
    let result = GitDiffResult::Text {
        original_content: "a\n".to_string(),
        modified_content: "b\n".to_string(),
        original_is_binary: false,
        modified_is_binary: false,
        large_diff_render_limit: None,
    };

    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(json["kind"], "text");
    assert_eq!(json["originalContent"], "a\n");
    assert_eq!(json["modifiedContent"], "b\n");
    assert_eq!(json["originalIsBinary"], false);
    assert_eq!(json["modifiedIsBinary"], false);
    assert!(json.get("largeDiffRenderLimit").is_none());
}

#[test]
fn render_limit_serializes_with_camel_case_and_kebab_reason() {
    let result = GitDiffResult::Text {
        original_content: String::new(),
        modified_content: String::new(),
        original_is_binary: false,
        modified_is_binary: false,
        large_diff_render_limit: Some(LargeDiffRenderLimit::Limited {
            limited: true,
            reason: LargeDiffRenderLimitReason::CharacterCount,
            line_counts: None,
            line_counts_are_minimum: None,
            character_count: 6_000_001,
            limits: LargeDiffRenderLimitLimits {
                max_lines_per_side: 120_000,
                max_combined_characters: 6_000_000,
            },
        }),
    };

    let json = serde_json::to_value(&result).unwrap();
    let limit = &json["largeDiffRenderLimit"];

    assert_eq!(json["kind"], "text");
    assert_eq!(limit["limited"], true);
    assert_eq!(limit["reason"], "character-count");
    assert!(limit["lineCounts"].is_null());
    assert!(limit.get("lineCountsAreMinimum").is_none());
    assert_eq!(limit["characterCount"], 6_000_001);
    assert_eq!(limit["limits"]["maxLinesPerSide"], 120_000);
    assert_eq!(limit["limits"]["maxCombinedCharacters"], 6_000_000);
}

#[test]
fn binary_result_serializes_with_kind_and_camel_case() {
    let result = GitDiffResult::Binary {
        original_content: "b64".to_string(),
        modified_content: "b64".to_string(),
        is_image: Some(true),
        mime_type: Some("image/png".to_string()),
        modified_deleted: Some(true),
        original_is_binary: true,
        modified_is_binary: false,
    };

    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(json["kind"], "binary");
    assert_eq!(json["originalContent"], "b64");
    assert_eq!(json["modifiedContent"], "b64");
    assert_eq!(json["isImage"], true);
    assert_eq!(json["mimeType"], "image/png");
    assert_eq!(json["modifiedDeleted"], true);
    assert_eq!(json["originalIsBinary"], true);
    assert_eq!(json["modifiedIsBinary"], false);
}
