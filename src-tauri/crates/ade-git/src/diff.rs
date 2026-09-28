//! Blob-semantics diff engine.
//!
//! Mirrors `orca:src/main/git/source-control/file-diff.ts`,
//! `git-blob-read.ts`, `diff-result.ts`, `previewable-binary-mime-types.ts`
//! and `orca:src/shared/{binary-buffer,large-diff-render-limit}.ts`: both
//! sides are read as whole blobs, a NUL byte in the first 8 KiB marks a side
//! binary, oversized blobs become binary instead of errors, and render-level
//! truncation turns huge text into metadata.

use std::path::Path;
use std::time::Duration;

use ade_core::errors::CoreError;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::Serialize;

use crate::runner::run_git_in;

pub const MAX_GIT_SHOW_BYTES: u64 = 10 * 1024 * 1024;
const DIFF_TIMEOUT: Duration = Duration::from_secs(120);
/// A NUL byte in the first chunk is Git's own heuristic for "this is binary".
const BINARY_SNIFF_BYTES: usize = 8192;
const MAX_RENDERED_DIFF_LINES_PER_SIDE: u64 = 120_000;
const MAX_RENDERED_DIFF_COMBINED_CHARACTERS: u64 = 6_000_000;

/// The binary formats the renderer can preview, mirroring
/// `previewable-binary-mime-types.ts`.
const PREVIEWABLE_BINARY_MIME_TYPES: &[(&str, &str)] = &[
    (".png", "image/png"),
    (".jpg", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".gif", "image/gif"),
    (".svg", "image/svg+xml"),
    (".webp", "image/webp"),
    (".bmp", "image/bmp"),
    (".ico", "image/x-icon"),
    (".pdf", "application/pdf"),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DiffLineCounts {
    pub original: u64,
    pub modified: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DiffLineCountMinimums {
    pub original: bool,
    pub modified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LargeDiffRenderLimitLimits {
    pub max_lines_per_side: u64,
    pub max_combined_characters: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum LargeDiffRenderLimitReason {
    LineCount,
    CharacterCount,
}

/// Mirrors the `LargeDiffRenderLimit` union in
/// `src/shared/large-diff-render-limit.ts`. `Unlimited` exists for contract
/// fidelity; a result that fits the caps omits the field entirely (the oracle
/// computes the unlimited shape and then drops it in `buildDiffResult`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(untagged)]
pub enum LargeDiffRenderLimit {
    #[serde(rename_all = "camelCase")]
    Unlimited {
        limited: bool,
        line_counts: DiffLineCounts,
        character_count: u64,
    },
    #[serde(rename_all = "camelCase")]
    Limited {
        limited: bool,
        reason: LargeDiffRenderLimitReason,
        line_counts: Option<DiffLineCounts>,
        #[serde(skip_serializing_if = "Option::is_none")]
        line_counts_are_minimum: Option<DiffLineCountMinimums>,
        character_count: u64,
        limits: LargeDiffRenderLimitLimits,
    },
}

/// Mirrors `GitDiffResult` in `src/shared/git-diff-compare-types.ts`: the
/// `kind` tag discriminates the two shapes on the wire.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GitDiffResult {
    #[serde(rename_all = "camelCase")]
    Text {
        original_content: String,
        modified_content: String,
        original_is_binary: bool,
        modified_is_binary: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        large_diff_render_limit: Option<LargeDiffRenderLimit>,
    },
    #[serde(rename_all = "camelCase")]
    Binary {
        original_content: String,
        modified_content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_image: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        modified_deleted: Option<bool>,
        original_is_binary: bool,
        modified_is_binary: bool,
    },
}

/// One side of a ref-to-ref diff, so branch/commit compare can reuse the blob
/// reader. `Rev` reads `rev:path` (an empty `rev` is the index: `:path`),
/// `Worktree` reads a file under the worktree, `Empty` is the absent side of a
/// root commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSide {
    Rev { rev: String, path: String },
    Worktree { path: String },
    Empty,
}

/// Original and modified content for one file, mirroring `getDiff` in
/// `file-diff.ts`. `staged` compares HEAD to the index; otherwise the left
/// side is the index (falling back to HEAD when the index has no entry) and
/// the right side is the working tree. `compare_against_head` pins the left
/// side at HEAD even when unstaged.
pub fn diff(
    worktree_path: &str,
    file_path: &str,
    staged: bool,
    compare_against_head: bool,
    timeout: Duration,
) -> Result<GitDiffResult, CoreError> {
    let (original, modified, modified_deleted) = if staged {
        let original = read_git_blob(worktree_path, "HEAD", file_path, timeout)?;
        let modified = read_git_blob(worktree_path, "", file_path, timeout)?;
        let modified_deleted = modified.is_none();
        (
            original.unwrap_or_default(),
            modified.unwrap_or_default(),
            modified_deleted,
        )
    } else {
        let original = if compare_against_head {
            read_git_blob(worktree_path, "HEAD", file_path, timeout)?.unwrap_or_default()
        } else {
            match read_git_blob(worktree_path, "", file_path, timeout)? {
                Some(blob) => blob,
                // Why: the index chain falls back to HEAD when the index has
                // no entry (e.g. a staged deletion recreated in the worktree).
                None => {
                    read_git_blob(worktree_path, "HEAD", file_path, timeout)?.unwrap_or_default()
                }
            }
        };
        let modified = read_worktree_file(worktree_path, file_path)?;
        let modified_deleted = !modified.exists;
        (original, modified, modified_deleted)
    };

    Ok(build_diff_result(
        original,
        modified,
        file_path,
        modified_deleted,
    ))
}

/// Reads both sides from explicit refs, mirroring `branch-diff.ts` /
/// `commit-diff.ts`. `old_path`, when provided, overrides the left side's
/// path so a rename reads its preimage. `file_path` names the file for MIME
/// detection. Proven deletions are not part of this contract; a missing right
/// blob reads as empty content, exactly like the oracle's branch/commit diff.
pub fn diff_refs(
    worktree_path: &str,
    left_ref: &DiffSide,
    right_ref: &DiffSide,
    file_path: &str,
    old_path: Option<&str>,
) -> Result<GitDiffResult, CoreError> {
    let original = read_side(worktree_path, left_ref, old_path)?;
    let modified = read_side(worktree_path, right_ref, None)?;
    Ok(build_diff_result(original, modified, file_path, false))
}

#[derive(Debug, Clone, Default)]
struct Blob {
    content: String,
    is_binary: bool,
    exists: bool,
}

/// Mirrors `readGitBlobAtOidPath` / `readGitBlobAtIndexPath`. Git exits 128
/// for a path absent from the tree, the index, or an unborn HEAD; that is
/// `Ok(None)`. Any other failure is an error — the oracle tracks a separate
/// `failed` flag only to poison its read cache, which this crate does not
/// have.
fn read_git_blob(
    worktree_path: &str,
    rev: &str,
    file_path: &str,
    timeout: Duration,
) -> Result<Option<Blob>, CoreError> {
    // Git's `rev:path` syntax expects forward slashes even on Windows.
    let git_path = file_path.replace('\\', "/");
    let spec = format!("{rev}:{git_path}");
    let mut args: Vec<&str> = vec!["show"];
    if !rev.is_empty() {
        // Why: a ref reaches the host as untrusted RPC input, so stop git
        // option parsing before the revision (mirrors git-blob-read.ts). The
        // index form starts with `:`, which cannot be an option.
        args.push("--end-of-options");
    }
    args.push(&spec);

    let output = run_git_in(worktree_path, &args, timeout, None)?;
    if output.status.success() {
        return Ok(Some(blob_from_bytes(&output.stdout, file_path)));
    }
    if output.status.code() == Some(128) {
        return Ok(None);
    }
    Err(CoreError::GitCommandFailed {
        command: args.join(" "),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        exit_code: output.status.code(),
    })
}

/// Mirrors `readWorkingTreeFile`: `ENOENT` is a proven deletion, a
/// non-regular file is absent, an oversized file is binary without being
/// read, and any other read failure is an error so it can never be mistaken
/// for a deletion.
fn read_worktree_file(worktree_path: &str, file_path: &str) -> Result<Blob, CoreError> {
    let relative = file_path.trim_start_matches(['/', '\\']);
    let target = Path::new(worktree_path).join(relative);
    match std::fs::metadata(&target) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Ok(Blob::default());
            }
            if metadata.len() > MAX_GIT_SHOW_BYTES {
                return Ok(Blob {
                    content: String::new(),
                    is_binary: true,
                    exists: true,
                });
            }
            Ok(blob_from_bytes(&std::fs::read(&target)?, file_path))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Blob::default()),
        Err(error) => Err(error.into()),
    }
}

/// Mirrors `bufferToBlob` plus the runner's max-buffer overflow path: a blob
/// over the cap is binary with no content, a previewable binary is base64, and
/// every other binary carries an empty string.
fn blob_from_bytes(bytes: &[u8], file_path: &str) -> Blob {
    if bytes.len() as u64 > MAX_GIT_SHOW_BYTES {
        return Blob {
            content: String::new(),
            is_binary: true,
            exists: true,
        };
    }
    let is_binary = is_binary_bytes(bytes);
    let content = if is_binary {
        if previewable_mime_type(file_path).is_some() {
            BASE64_STANDARD.encode(bytes)
        } else {
            String::new()
        }
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    Blob {
        content,
        is_binary,
        exists: true,
    }
}

fn is_binary_bytes(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0)
}

/// Mirrors `path.extname(filePath).toLowerCase()`: dotfiles have no extension.
fn previewable_mime_type(file_path: &str) -> Option<&'static str> {
    let extension = Path::new(file_path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    PREVIEWABLE_BINARY_MIME_TYPES
        .iter()
        .find(|(candidate, _)| candidate.trim_start_matches('.') == extension)
        .map(|(_, mime_type)| *mime_type)
}

fn read_side(
    worktree_path: &str,
    side: &DiffSide,
    path_override: Option<&str>,
) -> Result<Blob, CoreError> {
    match side {
        DiffSide::Rev { rev, path } => {
            let path = path_override.unwrap_or(path);
            Ok(read_git_blob(worktree_path, rev, path, DIFF_TIMEOUT)?.unwrap_or_default())
        }
        DiffSide::Worktree { path } => {
            let path = path_override.unwrap_or(path);
            read_worktree_file(worktree_path, path)
        }
        DiffSide::Empty => Ok(Blob::default()),
    }
}

/// Mirrors `buildDiffResult`: binary wins over text, a previewable extension
/// sets `mimeType`/`isImage` (PDFs included), oversized text turns into an
/// empty text result carrying `largeDiffRenderLimit`, and a proven deletion is
/// only representable on the binary contract.
fn build_diff_result(
    original: Blob,
    modified: Blob,
    file_path: &str,
    modified_deleted: bool,
) -> GitDiffResult {
    if original.is_binary || modified.is_binary {
        let mime_type = previewable_mime_type(file_path).map(str::to_string);
        return GitDiffResult::Binary {
            original_content: original.content,
            modified_content: modified.content,
            is_image: mime_type.as_ref().map(|_| true),
            mime_type,
            modified_deleted: modified_deleted.then_some(true),
            original_is_binary: original.is_binary,
            modified_is_binary: modified.is_binary,
        };
    }

    match large_diff_render_limit(&original.content, &modified.content) {
        Some(limit) => GitDiffResult::Text {
            original_content: String::new(),
            modified_content: String::new(),
            original_is_binary: false,
            modified_is_binary: false,
            large_diff_render_limit: Some(limit),
        },
        None => GitDiffResult::Text {
            original_content: original.content,
            modified_content: modified.content,
            original_is_binary: false,
            modified_is_binary: false,
            large_diff_render_limit: None,
        },
    }
}

/// Mirrors `getLargeDiffRenderLimit`, including its order of checks: the
/// combined character cap short-circuits before the bounded line counts. The
/// oracle measures JS string length — UTF-16 code units — not bytes.
fn large_diff_render_limit(original: &str, modified: &str) -> Option<LargeDiffRenderLimit> {
    let character_count = js_length(original) + js_length(modified);
    let limits = LargeDiffRenderLimitLimits {
        max_lines_per_side: MAX_RENDERED_DIFF_LINES_PER_SIDE,
        max_combined_characters: MAX_RENDERED_DIFF_COMBINED_CHARACTERS,
    };

    if character_count > MAX_RENDERED_DIFF_COMBINED_CHARACTERS {
        return Some(LargeDiffRenderLimit::Limited {
            limited: true,
            reason: LargeDiffRenderLimitReason::CharacterCount,
            line_counts: None,
            line_counts_are_minimum: None,
            character_count,
            limits,
        });
    }

    let original_lines = count_lines_up_to(original, MAX_RENDERED_DIFF_LINES_PER_SIDE);
    let modified_lines = count_lines_up_to(modified, MAX_RENDERED_DIFF_LINES_PER_SIDE);
    if original_lines.exceeded || modified_lines.exceeded {
        return Some(LargeDiffRenderLimit::Limited {
            limited: true,
            reason: LargeDiffRenderLimitReason::LineCount,
            line_counts: Some(DiffLineCounts {
                original: original_lines.count,
                modified: modified_lines.count,
            }),
            line_counts_are_minimum: Some(DiffLineCountMinimums {
                original: original_lines.exceeded,
                modified: modified_lines.exceeded,
            }),
            character_count,
            limits,
        });
    }

    None
}

fn js_length(content: &str) -> u64 {
    content.encode_utf16().count() as u64
}

struct LineCount {
    count: u64,
    exceeded: bool,
}

/// Mirrors `countLinesEmptyAsZeroUpToLimit`: empty content is zero lines,
/// anything else is one plus its newlines, and counting stops one line past
/// the cap.
fn count_lines_up_to(content: &str, max_lines: u64) -> LineCount {
    if content.is_empty() {
        return LineCount {
            count: 0,
            exceeded: false,
        };
    }
    let mut count = 1;
    for byte in content.bytes() {
        if byte != b'\n' {
            continue;
        }
        count += 1;
        if count > max_lines {
            return LineCount {
                count,
                exceeded: true,
            };
        }
    }
    LineCount {
        count,
        exceeded: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_lines_up_to_stops_one_past_the_cap() {
        let at_cap = "x\n".repeat(119_999);
        let counted = count_lines_up_to(&at_cap, 120_000);
        assert_eq!(counted.count, 120_000);
        assert!(!counted.exceeded);

        let over_cap = "x\n".repeat(120_000);
        let counted = count_lines_up_to(&over_cap, 120_000);
        assert_eq!(counted.count, 120_001);
        assert!(counted.exceeded);

        let empty = count_lines_up_to("", 120_000);
        assert_eq!(empty.count, 0);
        assert!(!empty.exceeded);
    }

    #[test]
    fn large_diff_render_limit_is_none_at_the_caps() {
        let at_line_cap = "\n".repeat(119_999);
        assert!(large_diff_render_limit(&at_line_cap, "").is_none());

        let at_character_cap = "a".repeat(6_000_000);
        assert!(large_diff_render_limit(&at_character_cap, "").is_none());
    }

    #[test]
    fn large_diff_render_limit_prefers_the_character_cap() {
        let content = "a\n".repeat(3_000_001);
        match large_diff_render_limit(&content, "") {
            Some(LargeDiffRenderLimit::Limited {
                reason,
                line_counts,
                ..
            }) => {
                assert_eq!(reason, LargeDiffRenderLimitReason::CharacterCount);
                assert!(line_counts.is_none());
            }
            other => panic!("expected a limited render result, got {other:?}"),
        }
    }

    #[test]
    fn js_length_counts_utf16_code_units() {
        assert_eq!(js_length("aé😀"), 4);
    }

    #[test]
    fn previewable_mime_type_matches_extensions_case_insensitively() {
        assert_eq!(previewable_mime_type("a/b/Image.PNG"), Some("image/png"));
        assert_eq!(previewable_mime_type("a/b/README.md"), None);
        assert_eq!(previewable_mime_type(".png"), None);
        assert_eq!(
            previewable_mime_type("a/b/archive.PDF"),
            Some("application/pdf")
        );
    }
}
