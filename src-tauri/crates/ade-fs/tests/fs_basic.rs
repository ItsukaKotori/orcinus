use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ade_fs::read::{FileContent, PathExistence};
use ade_fs::{FsError, FsService, MAX_TEXT_FILE_SIZE, PATH_ACCESS_DENIED_MESSAGE};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ade-fs-{name}-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn str(&self) -> &str {
        self.path.to_str().expect("temp dir path is UTF-8")
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn canonical(&self) -> PathBuf {
        self.path.canonicalize().expect("canonicalize temp dir")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("path is UTF-8")
}

#[test]
fn authorizes_registered_root_and_descendants() {
    let root = TempDir::new("root");
    let file = root.join("a.txt");
    fs::write(&file, "hello").unwrap();

    let service = FsService::new();
    service.authorize_root(root.str());

    assert_eq!(service.resolve(root.str()).unwrap(), root.canonical());
    assert_eq!(
        service.resolve(&format!("{}/", root.str())).unwrap(),
        root.canonical()
    );
    assert!(service.resolve(path_str(&file)).is_ok());
    assert_eq!(service.read_file(path_str(&file)).unwrap().content, "hello");
}

#[test]
fn denies_sibling_with_shared_prefix() {
    let parent = TempDir::new("prefix");
    let root = parent.join("root");
    let sibling = parent.join("root-other");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&sibling).unwrap();
    fs::write(sibling.join("f.txt"), "nope").unwrap();

    let service = FsService::new();
    service.authorize_root(path_str(&root));

    let error = service.resolve(path_str(&sibling)).unwrap_err();
    assert!(matches!(&error, FsError::PathAccessDenied));
    assert_eq!(error.to_string(), PATH_ACCESS_DENIED_MESSAGE);

    let error = service
        .resolve(path_str(&sibling.join("f.txt")))
        .unwrap_err();
    assert!(matches!(&error, FsError::PathAccessDenied));
    assert_eq!(error.to_string(), PATH_ACCESS_DENIED_MESSAGE);
}

#[test]
fn denies_lexical_parent_escape() {
    let parent = TempDir::new("lexical");
    let root = parent.join("root");
    fs::create_dir_all(&root).unwrap();
    let outside = parent.join("outside.txt");
    fs::write(&outside, "outside").unwrap();

    let service = FsService::new();
    service.authorize_root(path_str(&root));

    let escape = root.join("..").join("outside.txt");
    let error = service.resolve(path_str(&escape)).unwrap_err();
    assert!(matches!(&error, FsError::PathAccessDenied));

    let inside = root.join("sub").join("..").join("ok.txt");
    assert!(service.resolve(path_str(&inside)).is_ok());
}

#[cfg(unix)]
#[test]
fn denies_symlink_escape() {
    let root = TempDir::new("escape-root");
    let outside = TempDir::new("escape-outside");
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();

    let service = FsService::new();
    service.authorize_root(root.str());

    let escaped = root.join("escape").join("secret.txt");
    let error = service.resolve(path_str(&escaped)).unwrap_err();
    assert!(matches!(&error, FsError::PathAccessDenied));
    assert_eq!(error.to_string(), PATH_ACCESS_DENIED_MESSAGE);
    assert!(service.read_file(path_str(&escaped)).is_err());
}

#[test]
fn authorizes_explicit_external_paths() {
    let outside = TempDir::new("external");
    let granted_dir = outside.join("granted");
    fs::create_dir_all(&granted_dir).unwrap();
    fs::write(granted_dir.join("note.txt"), "external").unwrap();
    let sibling = outside.join("sibling.txt");
    fs::write(&sibling, "sibling").unwrap();

    let service = FsService::new();
    assert!(service.resolve(path_str(&granted_dir)).is_err());

    service.authorize_external(path_str(&granted_dir)).unwrap();
    let note = granted_dir.join("note.txt");
    assert!(service.resolve(path_str(&note)).is_ok());
    assert_eq!(
        service.read_file(path_str(&note)).unwrap().content,
        "external"
    );
    assert!(service.resolve(path_str(&sibling)).is_err());

    let file_only = FsService::new();
    file_only.authorize_external(path_str(&sibling)).unwrap();
    assert!(file_only.resolve(path_str(&sibling)).is_ok());
    assert!(file_only
        .resolve(path_str(&outside.join("missing.txt")))
        .is_err());
}

#[test]
fn revoking_a_root_denies_access() {
    let root = TempDir::new("revoke");
    fs::write(root.join("a.txt"), "a").unwrap();

    let service = FsService::new();
    service.authorize_root(root.str());
    assert!(service.resolve(root.str()).is_ok());

    service.revoke_root(root.str());
    assert!(matches!(
        service.resolve(root.str()).unwrap_err(),
        FsError::PathAccessDenied
    ));
}

#[test]
fn read_dir_sorts_directories_first_and_naturally() {
    let root = TempDir::new("read-dir");
    fs::create_dir_all(root.join("2 - src")).unwrap();
    fs::create_dir_all(root.join("10 - assets")).unwrap();
    for name in [
        "99 - a.txt",
        "100 - b.txt",
        "2.txt",
        "02.txt",
        "a.txt",
        "B.txt",
    ] {
        fs::write(root.join(name), name).unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("a.txt"), root.join("link.txt")).unwrap();

    let service = FsService::new();
    service.authorize_root(root.str());
    let entries = service.read_dir(root.str()).unwrap();
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();

    #[cfg(unix)]
    let expected = vec![
        "2 - src",
        "10 - assets",
        "02.txt",
        "2.txt",
        "99 - a.txt",
        "100 - b.txt",
        "a.txt",
        "B.txt",
        "link.txt",
    ];
    #[cfg(not(unix))]
    let expected = vec![
        "2 - src",
        "10 - assets",
        "02.txt",
        "2.txt",
        "99 - a.txt",
        "100 - b.txt",
        "a.txt",
        "B.txt",
    ];
    assert_eq!(names, expected);

    assert!(entries[0].is_directory);
    assert!(!entries[0].is_symlink);
    #[cfg(unix)]
    {
        let link = entries
            .iter()
            .find(|entry| entry.name == "link.txt")
            .expect("symlink entry");
        assert!(!link.is_directory);
        assert!(link.is_symlink);
    }
}

#[test]
fn read_file_reports_text_binary_and_image_content() {
    let root = TempDir::new("read-file");
    let service = FsService::new();
    service.authorize_root(root.str());

    let text = root.join("note.txt");
    fs::write(&text, "hello").unwrap();
    assert_eq!(
        service.read_file(path_str(&text)).unwrap(),
        FileContent {
            content: "hello".to_string(),
            is_binary: false,
            is_image: None,
            mime_type: None,
            file_identity: None,
        }
    );

    let binary = root.join("bin.dat");
    fs::write(&binary, [0u8, 1, 2, 3]).unwrap();
    let result = service.read_file(path_str(&binary)).unwrap();
    assert!(result.is_binary);
    assert!(result.content.is_empty());

    let large_binary = root.join("large.bin");
    let mut payload = vec![b'a'; 9000];
    payload[0] = 0;
    fs::write(&large_binary, payload).unwrap();
    let result = service.read_file(path_str(&large_binary)).unwrap();
    assert!(result.is_binary);
    assert!(result.content.is_empty());

    let image = root.join("pic.png");
    let image_bytes: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0, 1, 2];
    fs::write(&image, &image_bytes).unwrap();
    let result = service.read_file(path_str(&image)).unwrap();
    assert!(result.is_binary);
    assert_eq!(result.is_image, Some(true));
    assert_eq!(result.mime_type.as_deref(), Some("image/png"));
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use base64::Engine as _;
    assert_eq!(
        BASE64_STANDARD.decode(&result.content).unwrap(),
        image_bytes
    );
}

#[test]
fn read_file_rejects_files_over_the_text_limit() {
    let root = TempDir::new("too-large");
    let service = FsService::new();
    service.authorize_root(root.str());

    let large = root.join("large.txt");
    let file = fs::File::create(&large).unwrap();
    file.set_len(MAX_TEXT_FILE_SIZE + 1).unwrap();

    let error = service.read_file(path_str(&large)).unwrap_err();
    match error {
        FsError::FileTooLarge { limit_mb, .. } => assert_eq!(limit_mb, 50),
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn stat_and_path_existence() {
    let root = TempDir::new("stat");
    let service = FsService::new();
    service.authorize_root(root.str());

    let file = root.join("data.txt");
    fs::write(&file, "12345").unwrap();
    let stat = service.stat(path_str(&file)).unwrap();
    assert_eq!(stat.size, 5);
    assert!(!stat.is_directory);
    assert!(stat.mtime > 0.0);

    assert!(service.path_exists(path_str(&file)).unwrap());
    let missing = root.join("missing.txt");
    assert!(!service.path_exists(path_str(&missing)).unwrap());

    let outside = TempDir::new("stat-outside");
    let outside_file = outside.join("x.txt");
    fs::write(&outside_file, "x").unwrap();
    assert!(matches!(
        service.path_exists(path_str(&outside_file)).unwrap_err(),
        FsError::PathAccessDenied
    ));

    let results = service
        .paths_exist(&[
            path_str(&file).to_string(),
            path_str(&missing).to_string(),
            path_str(&outside_file).to_string(),
        ])
        .unwrap();
    assert_eq!(
        results,
        vec![
            PathExistence::Exists { exists: true },
            PathExistence::Exists { exists: false },
            PathExistence::Error {
                error: PATH_ACCESS_DENIED_MESSAGE.to_string()
            },
        ]
    );

    let too_many: Vec<String> = (0..=128)
        .map(|index| format!("{}/f{index}", root.str()))
        .collect();
    assert!(matches!(
        service.paths_exist(&too_many).unwrap_err(),
        FsError::InvalidInput(_)
    ));
}

#[test]
fn write_file_is_atomic_and_replaces_content() {
    let root = TempDir::new("write");
    let service = FsService::new();
    service.authorize_root(root.str());

    let file = root.join("note.txt");
    service.write_file(path_str(&file), "first").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "first");
    service.write_file(path_str(&file), "second").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "second");

    let leftovers: Vec<String> = fs::read_dir(root.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".ade-tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );

    assert!(matches!(
        service.write_file(root.str(), "x").unwrap_err(),
        FsError::InvalidInput(_)
    ));
}

#[test]
fn create_file_requires_an_existing_parent_and_never_overwrites() {
    let root = TempDir::new("create-file");
    let service = FsService::new();
    service.authorize_root(root.str());

    let file = root.join("new.txt");
    service.create_file(path_str(&file)).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "");

    let error = service.create_file(path_str(&file)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "A file or folder named 'new.txt' already exists in this location"
    );

    let missing_parent = root.join("missing-dir").join("file.txt");
    assert!(matches!(
        service.create_file(path_str(&missing_parent)).unwrap_err(),
        FsError::Io(_)
    ));
}

#[test]
fn create_dir_is_recursive_and_rejects_existing_paths() {
    let root = TempDir::new("create-dir");
    let service = FsService::new();
    service.authorize_root(root.str());

    let nested = root.join("a").join("b").join("c");
    service.create_dir(path_str(&nested)).unwrap();
    assert!(nested.is_dir());

    let error = service.create_dir(path_str(&nested)).unwrap_err();
    assert!(matches!(&error, FsError::AlreadyExists(name) if name == "c"));
}

#[test]
fn rename_moves_across_directories_and_refuses_to_clobber() {
    let root = TempDir::new("rename");
    let service = FsService::new();
    service.authorize_root(root.str());

    let source = root.join("src.txt");
    fs::write(&source, "content").unwrap();
    let destination_dir = root.join("dest");
    fs::create_dir_all(&destination_dir).unwrap();
    let destination = destination_dir.join("moved.txt");

    service
        .rename(path_str(&source), path_str(&destination))
        .unwrap();
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(&destination).unwrap(), "content");

    let other = root.join("other.txt");
    fs::write(&other, "other").unwrap();
    let error = service
        .rename(path_str(&other), path_str(&destination))
        .unwrap_err();
    assert!(matches!(&error, FsError::AlreadyExists(name) if name == "moved.txt"));
    assert_eq!(fs::read_to_string(&destination).unwrap(), "content");
}

#[test]
fn copy_handles_files_and_directories_recursively() {
    let root = TempDir::new("copy");
    let service = FsService::new();
    service.authorize_root(root.str());

    let source_dir = root.join("src");
    fs::create_dir_all(source_dir.join("nested")).unwrap();
    fs::write(source_dir.join("top.txt"), "top").unwrap();
    fs::write(source_dir.join("nested").join("deep.txt"), "deep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("top.txt", source_dir.join("link.txt")).unwrap();
    let destination_dir = root.join("dest");

    service
        .copy(path_str(&source_dir), path_str(&destination_dir))
        .unwrap();
    assert_eq!(
        fs::read_to_string(destination_dir.join("top.txt")).unwrap(),
        "top"
    );
    assert_eq!(
        fs::read_to_string(destination_dir.join("nested").join("deep.txt")).unwrap(),
        "deep"
    );
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(destination_dir.join("link.txt")).unwrap(),
        PathBuf::from("top.txt")
    );

    let single = root.join("single.txt");
    service
        .copy(path_str(&source_dir.join("top.txt")), path_str(&single))
        .unwrap();
    assert_eq!(fs::read_to_string(&single).unwrap(), "top");

    let error = service
        .copy(path_str(&source_dir.join("top.txt")), path_str(&single))
        .unwrap_err();
    assert!(matches!(&error, FsError::AlreadyExists(name) if name == "single.txt"));
}

#[test]
fn delete_refuses_authorized_roots() {
    let root = TempDir::new("delete-root");
    let service = FsService::new();
    service.authorize_root(root.str());

    let error = service.delete_path(root.str()).unwrap_err();
    assert!(matches!(&error, FsError::InvalidInput(_)));
    assert!(root.path().exists());

    let nested = root.join("nested");
    fs::create_dir_all(&nested).unwrap();
    service.authorize_root(path_str(&nested));
    assert!(service.delete_path(path_str(&nested)).is_err());
    assert!(nested.exists());
}

#[test]
fn delete_of_a_missing_path_is_idempotent() {
    let root = TempDir::new("delete-missing");
    let service = FsService::new();
    service.authorize_root(root.str());

    service
        .delete_path(path_str(&root.join("missing.txt")))
        .unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn delete_moves_files_to_trash() {
    let root = TempDir::new("trash");
    let service = FsService::new();
    service.authorize_root(root.str());

    let file = root.join("trash-me.txt");
    fs::write(&file, "trash").unwrap();

    service
        .delete_path(path_str(&file))
        .expect("trash on macOS");
    assert!(!file.exists());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn delete_moves_files_to_trash_when_available() {
    let root = TempDir::new("trash");
    let service = FsService::new();
    service.authorize_root(root.str());

    let file = root.join("trash-me.txt");
    fs::write(&file, "trash").unwrap();

    // Sandboxed/CI environments may have no reachable recycle bin; the
    // contract is "trash failure is an error and the file is untouched".
    match service.delete_path(path_str(&file)) {
        Ok(()) => assert!(!file.exists()),
        Err(FsError::Trash(_)) => assert!(file.exists()),
        Err(other) => panic!("unexpected delete error: {other}"),
    }
}
