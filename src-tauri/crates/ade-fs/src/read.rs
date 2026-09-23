use std::cmp::Ordering;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::Serialize;

use crate::{FsError, FsService};

// Mirrors oracle `filesystem-file-content-inspection.ts`: 50MB text block and
// 50MB previewable-binary block, with a NUL-byte probe over the first 8KB.
pub const MAX_TEXT_FILE_SIZE: u64 = 50 * 1024 * 1024;
pub const BINARY_PROBE_BYTES: usize = 8192;
pub const MAX_PREVIEWABLE_BINARY_SIZE: u64 = 50 * 1024 * 1024;
pub const PATH_EXISTENCE_BATCH_MAX: usize = 128;

pub const PREVIEWABLE_BINARY_MIME_TYPES: &[(&str, &str)] = &[
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
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub is_directory: bool,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub content: String,
    pub is_binary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_image: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStat {
    pub size: u64,
    pub is_directory: bool,
    pub mtime: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PathExistence {
    Exists { exists: bool },
    Error { error: String },
}

impl FsService {
    pub fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let target = self.resolve(path)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&target)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let is_symlink = file_type.is_symlink();
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: file_type.is_dir() && !is_symlink,
                is_symlink,
            });
        }
        sort_dir_entries(&mut entries);
        Ok(entries)
    }

    pub fn read_file(&self, path: &str) -> Result<FileContent, FsError> {
        let target = self.resolve(path)?;
        let stats = fs::metadata(&target)?;
        let mime_type = previewable_mime_type(&target);
        let size_limit = if mime_type.is_some() {
            MAX_PREVIEWABLE_BINARY_SIZE
        } else {
            MAX_TEXT_FILE_SIZE
        };
        if stats.len() > size_limit {
            return Err(FsError::FileTooLarge {
                size_mb: stats.len() as f64 / 1024.0 / 1024.0,
                limit_mb: size_limit / 1024 / 1024,
            });
        }

        if let Some(mime_type) = mime_type {
            let buffer = fs::read(&target)?;
            return Ok(FileContent {
                content: BASE64_STANDARD.encode(&buffer),
                is_binary: true,
                is_image: Some(true),
                mime_type: Some(mime_type.to_string()),
                file_identity: None,
            });
        }

        if stats.len() > BINARY_PROBE_BYTES as u64 && is_binary_file_prefix(&target)? {
            return Ok(FileContent {
                content: String::new(),
                is_binary: true,
                ..FileContent::default()
            });
        }

        let buffer = fs::read(&target)?;
        if is_binary_buffer(&buffer) {
            return Ok(FileContent {
                content: String::new(),
                is_binary: true,
                ..FileContent::default()
            });
        }
        Ok(FileContent {
            content: String::from_utf8_lossy(&buffer).into_owned(),
            is_binary: false,
            ..FileContent::default()
        })
    }

    pub fn stat(&self, path: &str) -> Result<FileStat, FsError> {
        let target = self.resolve(path)?;
        let metadata = fs::metadata(&target)?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Ok(FileStat {
            size: metadata.len(),
            is_directory: metadata.is_dir(),
            mtime,
        })
    }

    pub fn path_exists(&self, path: &str) -> Result<bool, FsError> {
        let target = self.resolve(path)?;
        match fs::metadata(&target) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(FsError::Io(error)),
        }
    }

    pub fn paths_exist(&self, paths: &[String]) -> Result<Vec<PathExistence>, FsError> {
        if paths.len() > PATH_EXISTENCE_BATCH_MAX {
            return Err(FsError::InvalidInput(
                "Invalid path existence batch".to_string(),
            ));
        }
        Ok(paths
            .iter()
            .map(|path| match self.path_exists(path) {
                Ok(exists) => PathExistence::Exists { exists },
                Err(error) => PathExistence::Error {
                    error: error.to_string(),
                },
            })
            .collect())
    }
}

pub fn is_binary_buffer(buffer: &[u8]) -> bool {
    buffer
        .iter()
        .take(BINARY_PROBE_BYTES)
        .any(|byte| *byte == 0)
}

fn is_binary_file_prefix(path: &Path) -> std::io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut probe = vec![0u8; BINARY_PROBE_BYTES];
    let mut filled = 0;
    while filled < probe.len() {
        let read = file.read(&mut probe[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(is_binary_buffer(&probe[..filled]))
}

fn previewable_mime_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    let suffix = format!(".{}", extension.to_lowercase());
    PREVIEWABLE_BINARY_MIME_TYPES
        .iter()
        .find(|(candidate, _)| *candidate == suffix)
        .map(|(_, mime_type)| *mime_type)
}

/// Natural-order comparison equivalent to
/// `Intl.Collator('en', {numeric: true})` followed by the code-unit fallback in
/// `src/shared/file-name-sort.ts`.
///
/// Non-digit runs compare case-insensitively (ICU primary strength), digit runs
/// compare numerically, and any remaining tie falls back to code units so the
/// order is total and stable.
pub fn compare_file_names(a: &str, b: &str) -> Ordering {
    let a_runs = name_runs(a);
    let b_runs = name_runs(b);
    let shared = a_runs.len().min(b_runs.len());
    for index in 0..shared {
        let (a_run, a_digits) = a_runs[index];
        let (b_run, b_digits) = b_runs[index];
        if a_digits != b_digits {
            break;
        }
        let ordering = if a_digits {
            compare_numeric_runs(a_run, b_run)
        } else {
            compare_primary_text(a_run, b_run)
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    if a_runs.len() != b_runs.len() && (a.starts_with(b) || b.starts_with(a)) {
        return a_runs.len().cmp(&b_runs.len());
    }
    a.cmp(b)
}

/// Directories first, then natural name order — the File Explorer listing contract.
pub fn sort_dir_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => compare_file_names(&a.name, &b.name),
    });
}

fn name_runs(name: &str) -> Vec<(&str, bool)> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (index, character) in name.char_indices() {
        let is_digit = character.is_ascii_digit();
        match current {
            Some(kind) if kind == is_digit => {}
            Some(_) => {
                runs.push((&name[start..index], current == Some(true)));
                start = index;
                current = Some(is_digit);
            }
            None => current = Some(is_digit),
        }
    }
    if let Some(kind) = current {
        runs.push((&name[start..], kind));
    }
    runs
}

fn compare_numeric_runs(a: &str, b: &str) -> Ordering {
    let a_digits = a.trim_start_matches('0');
    let b_digits = b.trim_start_matches('0');
    a_digits
        .len()
        .cmp(&b_digits.len())
        .then_with(|| a_digits.cmp(b_digits))
}

fn compare_primary_text(a: &str, b: &str) -> Ordering {
    let a_lower: String = a.chars().flat_map(char::to_lowercase).collect();
    let b_lower: String = b.chars().flat_map(char::to_lowercase).collect();
    a_lower.cmp(&b_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(a: &str, b: &str) -> i32 {
        match compare_file_names(a, b) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    fn sorted(mut names: Vec<&str>) -> Vec<&str> {
        names.sort_by(|a, b| compare_file_names(a, b));
        names
    }

    #[test]
    fn sorts_numeric_segments_naturally() {
        let names = vec![
            "1 - item.txt",
            "100 - item.txt",
            "2 - item.txt",
            "200 - item.txt",
            "409 - item.txt",
            "41 - item.txt",
            "410 - item.txt",
            "9 - item.txt",
            "99 - item.txt",
        ];
        assert_eq!(
            sorted(names),
            vec![
                "1 - item.txt",
                "2 - item.txt",
                "9 - item.txt",
                "41 - item.txt",
                "99 - item.txt",
                "100 - item.txt",
                "200 - item.txt",
                "409 - item.txt",
                "410 - item.txt",
            ]
        );
    }

    #[test]
    fn sorts_embedded_numbers_naturally() {
        assert_eq!(
            sorted(vec!["b1.md", "a10.md", "a2.md", "a1.md"]),
            vec!["a1.md", "a2.md", "a10.md", "b1.md"]
        );
    }

    #[test]
    fn breaks_numeric_ties_by_code_units() {
        assert_eq!(sorted(vec!["2.txt", "02.txt"]), vec!["02.txt", "2.txt"]);
        assert_eq!(sorted(vec!["02.txt", "2.txt"]), vec!["02.txt", "2.txt"]);
        assert_eq!(compare("a.txt", "a.txt"), 0);
    }

    #[test]
    fn compares_text_case_insensitively_with_code_unit_fallback() {
        assert_eq!(compare("a.txt", "B.txt"), -1);
        assert_eq!(compare("B.txt", "a.txt"), 1);
        assert_eq!(compare("A.md", "a.md"), -1);
        assert_eq!(compare("a", "a1"), -1);
        assert_eq!(compare("a1", "a"), 1);
        assert_eq!(compare("1a", "a1"), -1);
    }

    #[test]
    fn keeps_directories_first_in_natural_order() {
        let mut entries = vec![
            DirEntry {
                name: "10 - notes".to_string(),
                is_directory: false,
                is_symlink: false,
            },
            DirEntry {
                name: "2 - src".to_string(),
                is_directory: true,
                is_symlink: false,
            },
            DirEntry {
                name: "9 - docs.txt".to_string(),
                is_directory: false,
                is_symlink: false,
            },
            DirEntry {
                name: "10 - assets".to_string(),
                is_directory: true,
                is_symlink: false,
            },
        ];
        sort_dir_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["2 - src", "10 - assets", "9 - docs.txt", "10 - notes"]
        );
    }
}
