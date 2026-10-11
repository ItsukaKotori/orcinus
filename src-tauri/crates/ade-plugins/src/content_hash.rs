//! Deterministic hash of a plugin's file tree.
//!
//! The hash names the immutable install directory, so two installs of
//! identical content share a name and a mutated install is detectable. The
//! algorithm is a verbatim port of
//! `orca:src/main/plugins/plugin-content-hash.ts`: sha256 of
//! `orca-plugin-tree-v1\0` followed by length-framed relative paths and file
//! bytes in sorted order; symlinks are refused outright; the root `.git`
//! directory is skipped; 2000 entries / 50 MiB limits apply.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::types::{
    plugin_path_segment_error, PluginError, CONTENT_CHANGED, CONTENT_TOO_LARGE, IO_ERROR,
    SYMLINK_NOT_ALLOWED, TOO_MANY_FILES, UNSAFE_PATH, UNSUPPORTED_ENTRY,
};

pub const PLUGIN_TREE_HASH_PREFIX: &[u8] = b"orca-plugin-tree-v1\0";
pub const MAX_PLUGIN_FILES: usize = 2_000;
pub const MAX_PLUGIN_TOTAL_BYTES: u64 = 50 * 1024 * 1024;

struct PluginFile {
    path: PathBuf,
    size: u64,
}

#[derive(Default)]
struct Counters {
    entries: usize,
    bytes: u64,
}

pub fn hash_plugin_tree(root: &Path) -> Result<String, PluginError> {
    let mut files: Vec<PluginFile> = Vec::new();
    let mut counters = Counters::default();
    collect_files(root, root, &mut files, &mut counters)?;

    let mut hasher = Sha256::new();
    hasher.update(PLUGIN_TREE_HASH_PREFIX);
    let mut total_bytes: u64 = 0;
    for file in &files {
        total_bytes += file.size;
        if total_bytes > MAX_PLUGIN_TOTAL_BYTES {
            return Err(content_too_large());
        }
        let relative = relative_path(root, &file.path).replace('\\', "/");
        let relative_bytes = relative.as_bytes();
        hasher.update((relative_bytes.len() as u64).to_be_bytes());
        hasher.update(relative_bytes);
        hasher.update(file.size.to_be_bytes());
        hash_file_bounded(&mut hasher, file)?;
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PluginFile>,
    counters: &mut Counters,
) -> Result<(), PluginError> {
    let read_dir = fs::read_dir(dir).map_err(|error| io_error(dir, error))?;

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|error| io_error(dir, error))?;
        entries.push((
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
        ));
    }
    entries.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));

    for (name, full) in entries {
        if dir == root && name == ".git" {
            continue;
        }
        if let Some(detail) = plugin_path_segment_error(&name) {
            return Err(PluginError::new(
                UNSAFE_PATH,
                format!("unsafe plugin path segment \"{name}\": {detail}"),
            ));
        }
        let metadata = fs::symlink_metadata(&full).map_err(|error| io_error(&full, error))?;
        counters.entries += 1;
        if counters.entries > MAX_PLUGIN_FILES {
            return Err(PluginError::new(
                TOO_MANY_FILES,
                format!("plugin exceeds the {MAX_PLUGIN_FILES}-entry limit"),
            ));
        }
        if metadata.file_type().is_symlink() {
            return Err(PluginError::new(
                SYMLINK_NOT_ALLOWED,
                format!(
                    "symlink not allowed in plugin content: {}",
                    relative_path(root, &full)
                ),
            ));
        }
        if metadata.is_dir() {
            collect_files(root, &full, files, counters)?;
        } else if metadata.is_file() {
            counters.bytes += metadata.len();
            if counters.bytes > MAX_PLUGIN_TOTAL_BYTES {
                return Err(content_too_large());
            }
            files.push(PluginFile {
                path: full,
                size: metadata.len(),
            });
        } else {
            return Err(PluginError::new(
                UNSUPPORTED_ENTRY,
                format!(
                    "unsupported plugin entry type: {}",
                    relative_path(root, &full)
                ),
            ));
        }
    }
    Ok(())
}

fn hash_file_bounded(hasher: &mut Sha256, file: &PluginFile) -> Result<(), PluginError> {
    let mut handle = fs::File::open(&file.path).map_err(|error| io_error(&file.path, error))?;
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes_read: u64 = 0;
    loop {
        let read = handle
            .read(&mut buffer)
            .map_err(|error| io_error(&file.path, error))?;
        if read == 0 {
            break;
        }
        bytes_read += read as u64;
        if bytes_read > file.size || bytes_read > MAX_PLUGIN_TOTAL_BYTES {
            return Err(content_changed(&file.path));
        }
        hasher.update(&buffer[..read]);
    }
    if bytes_read != file.size {
        return Err(content_changed(&file.path));
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn io_error(path: &Path, error: std::io::Error) -> PluginError {
    PluginError::new(IO_ERROR, format!("{}: {error}", path.display()))
}

fn content_too_large() -> PluginError {
    PluginError::new(
        CONTENT_TOO_LARGE,
        format!("plugin exceeds the {MAX_PLUGIN_TOTAL_BYTES}-byte limit"),
    )
}

fn content_changed(path: &Path) -> PluginError {
    PluginError::new(
        CONTENT_CHANGED,
        format!("plugin file changed while hashing: {}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CONTENT_TOO_LARGE, IO_ERROR, SYMLINK_NOT_ALLOWED, TOO_MANY_FILES, UNSAFE_PATH,
        UNSUPPORTED_ENTRY,
    };
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    const BASIC_HASH: &str = "fb62eb68d2eb67d4a3a0be81cee1719a359020ce2684b39112159421840f667d";
    const PREFIX_ONLY_HASH: &str =
        "f89304b8b588480f174f63bb5b29e394af1f8b3d8d8bb3ec38e280cc7efeae13";
    const NESTED_GIT_HASH: &str =
        "f7c0aec87c0a90e41028dfbdd176ff5c077227284925a29e7143ec365956c8a4";
    const SORT_ORDER_HASH: &str =
        "1cedba87eb69b89f181b02fb85d2037a1a8936bc2b474be70f10d025c819b496";
    const UNICODE_HASH: &str = "12eaef482f81c3a8b1c9c72d3ae3448ff7960c4b43969fd7d57bfb325bc64097";
    const TWO_THOUSAND_FILES_HASH: &str =
        "aff5d483fabfae5852b4e67a3a0a75729208e7fa6433ef526fc1a5a0aa933696";
    const ENTRY_BOUNDARY_HASH: &str =
        "dfd12645f7ef685207b0dddaf56d75e76cad355426c96d834d80713a5ef5b4ee";
    const BACKSLASH_HASH: &str = "cf88e18ddc236cf97650c1531e632d23485479d9e9c29727498229dcfeec5e00";
    const MIXED_HASH: &str = "2026f0c2d7d0377b6a5ec28ca118acf091b482747b3cb31066c65a53c2f8d080";

    fn write_file(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn hashes_fixed_tree_like_oracle() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "hello.txt", b"hello world\n");
        write_file(root, "empty.txt", b"");
        write_file(root, "sub/nested.json", b"{\"a\":1}");
        write_file(root, ".git/config", b"[core]\n");

        assert_eq!(hash_plugin_tree(root).unwrap(), BASIC_HASH);
    }

    #[test]
    fn root_git_skip_is_content_independent() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "hello.txt", b"hello world\n");
        write_file(root, "empty.txt", b"");
        write_file(root, "sub/nested.json", b"{\"a\":1}");
        assert_eq!(hash_plugin_tree(root).unwrap(), BASIC_HASH);

        write_file(root, ".git/objects", &[b'x'; 1000]);
        assert_eq!(hash_plugin_tree(root).unwrap(), BASIC_HASH);
    }

    #[test]
    fn nested_git_directory_is_hashed() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "a.txt", b"root\n");
        write_file(root, "sub/.git/HEAD", b"ref: refs/heads/main\n");
        assert_eq!(hash_plugin_tree(root).unwrap(), NESTED_GIT_HASH);

        write_file(root, ".git/HEAD", b"skip me\n");
        assert_eq!(hash_plugin_tree(root).unwrap(), NESTED_GIT_HASH);
    }

    #[test]
    fn sorts_entries_depth_first_in_js_order() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "a.txt", b"file-a\n");
        write_file(root, "a/b.txt", b"file-b\n");
        write_file(root, "B.txt", b"file-B\n");

        assert_eq!(hash_plugin_tree(root).unwrap(), SORT_ORDER_HASH);
    }

    #[test]
    fn hashes_unicode_and_space_names_like_oracle() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "héllo wörld.txt", "unicode\n".as_bytes());
        write_file(root, "dir with space/x.txt", b"x");

        assert_eq!(hash_plugin_tree(root).unwrap(), UNICODE_HASH);
    }

    #[test]
    fn normalizes_backslash_names_in_hash_records() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "a\\b.txt", b"slash\n");

        assert_eq!(hash_plugin_tree(root).unwrap(), BACKSLASH_HASH);
    }

    #[test]
    fn hashes_mixed_tree_like_oracle() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "README.md", b"# Demo\n");
        write_file(root, "main.mjs", b"export function activate() {}\n");
        write_file(root, "empty", b"");
        write_file(root, "a b.txt", b"spaces");
        write_file(root, "z-y.txt", b"dash");
        write_file(root, "UPPER.txt", b"upper");
        write_file(root, ".hidden", b"hidden");
        write_file(root, ".gitignore", b"node_modules\n");
        write_file(root, "assets/logo.svg", b"<svg></svg>");
        write_file(
            root,
            "assets/deep/nested/data.bin",
            "0123456789".repeat(100).as_bytes(),
        );
        write_file(root, "locales/en-US.json", b"{\"hello\":\"Hello\"}");
        write_file(
            root,
            "locales/zh-Hant-TW.json",
            "{\"hello\":\"你好\"}".as_bytes(),
        );
        write_file(root, "panels/dashboard.html", b"<html>dashboard</html>");
        write_file(root, "panels/console.html", b"<html>console</html>");
        write_file(root, ".git", b"gitdir: elsewhere\n");

        assert_eq!(hash_plugin_tree(root).unwrap(), MIXED_HASH);

        write_file(root, "panels/console.html", b"<html>console!</html>");
        assert_ne!(hash_plugin_tree(root).unwrap(), MIXED_HASH);
        assert_eq!(hash_plugin_tree(root).unwrap().len(), 64);
    }

    #[test]
    fn rejects_symlinks() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "real.txt", b"real");
        symlink("real.txt", root.join("link.txt")).unwrap();

        let error = hash_plugin_tree(root).unwrap_err();
        assert_eq!(error.code(), SYMLINK_NOT_ALLOWED);
        assert_eq!(
            error.message(),
            "symlink not allowed in plugin content: link.txt"
        );

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        write_file(root, "sub/real.txt", b"real");
        symlink("real.txt", root.join("sub/link.txt")).unwrap();

        let error = hash_plugin_tree(root).unwrap_err();
        assert_eq!(error.code(), SYMLINK_NOT_ALLOWED);
        assert_eq!(
            error.message(),
            "symlink not allowed in plugin content: sub/link.txt"
        );
    }

    #[test]
    fn rejects_unsafe_path_segments() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "a:b.txt", b"x");

        let error = hash_plugin_tree(root).unwrap_err();
        assert_eq!(error.code(), UNSAFE_PATH);
        assert_eq!(
            error.message(),
            "unsafe plugin path segment \"a:b.txt\": path segment contains a Windows-forbidden character or alternate-data-stream colon"
        );

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "con.txt", b"x");

        let error = hash_plugin_tree(root).unwrap_err();
        assert_eq!(error.code(), UNSAFE_PATH);
        assert_eq!(
            error.message(),
            "unsafe plugin path segment \"con.txt\": path segment is a Windows reserved device name"
        );
    }

    #[test]
    fn enforces_entry_limit_over_files_and_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        for index in 0..MAX_PLUGIN_FILES + 1 {
            write_file(root, &format!("f{index:04}.txt"), b"x");
        }
        let error = hash_plugin_tree(root).unwrap_err();
        assert_eq!(error.code(), TOO_MANY_FILES);
        assert_eq!(error.message(), "plugin exceeds the 2000-entry limit");

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        for index in 0..MAX_PLUGIN_FILES {
            write_file(root, &format!("f{index:04}.txt"), b"x");
        }
        assert_eq!(hash_plugin_tree(root).unwrap(), TWO_THOUSAND_FILES_HASH);

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        for index in 0..MAX_PLUGIN_FILES - 1 {
            fs::create_dir(root.join(format!("d{index:04}"))).unwrap();
        }
        write_file(root, "z.txt", b"z");
        assert_eq!(hash_plugin_tree(root).unwrap(), ENTRY_BOUNDARY_HASH);

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        for index in 0..MAX_PLUGIN_FILES - 1 {
            fs::create_dir(root.join(format!("d{index:04}"))).unwrap();
        }
        write_file(root, "y.txt", b"y");
        write_file(root, "z.txt", b"z");
        let error = hash_plugin_tree(root).unwrap_err();
        assert_eq!(error.code(), TOO_MANY_FILES);
    }

    #[test]
    fn empty_tree_matches_prefix_digest() {
        let dir = TempDir::new().unwrap();
        assert_eq!(hash_plugin_tree(dir.path()).unwrap(), PREFIX_ONLY_HASH);

        let dir = TempDir::new().unwrap();
        for index in 0..MAX_PLUGIN_FILES - 1 {
            fs::create_dir(dir.path().join(format!("d{index:04}"))).unwrap();
        }
        assert_eq!(hash_plugin_tree(dir.path()).unwrap(), PREFIX_ONLY_HASH);
    }

    #[test]
    fn enforces_total_bytes_limit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let file = fs::File::create(root.join("big.bin")).unwrap();
        file.set_len(MAX_PLUGIN_TOTAL_BYTES + 1).unwrap();
        drop(file);

        let error = hash_plugin_tree(root).unwrap_err();
        assert_eq!(error.code(), CONTENT_TOO_LARGE);
        assert_eq!(error.message(), "plugin exceeds the 52428800-byte limit");
    }

    #[test]
    fn rejects_unsupported_entry_types() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pipe");
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let result = unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) };
        assert_eq!(result, 0);

        let error = hash_plugin_tree(dir.path()).unwrap_err();
        assert_eq!(error.code(), UNSUPPORTED_ENTRY);
        assert_eq!(error.message(), "unsupported plugin entry type: pipe");
    }

    #[test]
    fn missing_root_is_io_error() {
        let dir = TempDir::new().unwrap();
        let error = hash_plugin_tree(&dir.path().join("nope")).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
    }

    #[test]
    fn repeated_hash_is_stable() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "hello.txt", b"hello world\n");
        let first = hash_plugin_tree(root).unwrap();
        let second = hash_plugin_tree(root).unwrap();
        assert_eq!(first, second);
    }
}
