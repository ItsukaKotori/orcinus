use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use ade_fs::{FsChangeEvent, FsChangeKind, FsChangedPayload, FsWatcher};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let raw = std::env::temp_dir().join(format!(
            "ade-fs-watch-it-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&raw);
        fs::create_dir_all(&raw).expect("create temp dir");
        // Why: macOS FSEvents reports canonical paths; watching the canonical
        // spelling keeps event paths comparable in assertions.
        Self {
            path: fs::canonicalize(&raw).expect("canonicalize temp dir"),
        }
    }

    fn str(&self) -> &str {
        self.path.to_str().expect("temp dir path is UTF-8")
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct EventCollector {
    receiver: Receiver<FsChangedPayload>,
    events: Vec<FsChangeEvent>,
    worktree_paths: Vec<String>,
}

impl EventCollector {
    fn new(receiver: Receiver<FsChangedPayload>) -> Self {
        Self {
            receiver,
            events: Vec::new(),
            worktree_paths: Vec::new(),
        }
    }

    fn wait_for(
        &mut self,
        timeout: Duration,
        predicate: impl Fn(&FsChangeEvent) -> bool,
    ) -> Option<FsChangeEvent> {
        if let Some(index) = self.events.iter().position(&predicate) {
            return Some(self.events.remove(index));
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .receiver
                .recv_timeout(remaining.min(Duration::from_millis(250)))
            {
                Ok(payload) => {
                    self.worktree_paths.push(payload.worktree_path);
                    self.events.extend(payload.events);
                    if let Some(index) = self.events.iter().position(&predicate) {
                        return Some(self.events.remove(index));
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
        None
    }
}

struct WatchSession {
    // Kept alive for the session and dropped before the temp dir.
    _watcher: FsWatcher,
    collector: EventCollector,
    dir: TempDir,
}

impl WatchSession {
    fn start(name: &str) -> Self {
        let dir = TempDir::new(name);
        let watcher = FsWatcher::new();
        let (sender, receiver) = channel();
        watcher.subscribe(move |payload| {
            let _ = sender.send(payload);
        });
        watcher.watch(dir.str(), "integration");

        let mut session = Self {
            _watcher: watcher,
            collector: EventCollector::new(receiver),
            dir,
        };

        // Warm up the stream so the measured steps never race installation.
        let warmup = session.dir.join(".ade-watch-warmup");
        let warmup_path = path_string(&warmup);
        fs::write(&warmup, "warmup").expect("write warmup file");
        let delivered = session.collector.wait_for(Duration::from_secs(5), |event| {
            event.absolute_path == warmup_path
        });
        assert!(
            delivered.is_some(),
            "watcher did not deliver the warm-up event"
        );
        session
    }

    fn dir(&self) -> &TempDir {
        &self.dir
    }

    fn collector(&mut self) -> &mut EventCollector {
        &mut self.collector
    }
}

#[test]
fn write_file_emits_create_or_update_with_is_directory_false() {
    let mut session = WatchSession::start("write");
    let file = session.dir().join("notes.txt");
    let file_path = path_string(&file);
    let started = Instant::now();
    fs::write(&file, "hello").expect("write file");

    let event = session
        .collector()
        .wait_for(Duration::from_secs(3), |event| {
            event.absolute_path == file_path
        })
        .expect("event for written file");
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "batch flush took {:?}; expected well under the 500ms max wait plus delivery",
        started.elapsed()
    );
    assert!(
        matches!(event.kind, FsChangeKind::Create | FsChangeKind::Update),
        "unexpected kind for a write: {event:?}"
    );
    assert_eq!(event.is_directory, Some(false));

    let root = session.dir().str().to_string();
    assert!(
        session
            .collector()
            .worktree_paths
            .iter()
            .all(|path| *path == root),
        "payload worktreePath must echo the watched root"
    );
}

#[test]
fn create_directory_emits_is_directory_true() {
    let mut session = WatchSession::start("mkdir");
    let directory = session.dir().join("subdir");
    let directory_path = path_string(&directory);
    fs::create_dir(&directory).expect("create dir");

    let event = session
        .collector()
        .wait_for(Duration::from_secs(3), |event| {
            event.absolute_path == directory_path
        })
        .expect("event for created directory");
    assert!(
        matches!(event.kind, FsChangeKind::Create | FsChangeKind::Update),
        "unexpected kind for mkdir: {event:?}"
    );
    assert_eq!(event.is_directory, Some(true));
}

#[test]
fn delete_file_emits_delete_without_is_directory() {
    let mut session = WatchSession::start("delete");
    let file = session.dir().join("doomed.txt");
    let file_path = path_string(&file);
    fs::write(&file, "x").expect("write file");
    assert!(
        session
            .collector()
            .wait_for(Duration::from_secs(3), |event| {
                event.absolute_path == file_path
                    && matches!(event.kind, FsChangeKind::Create | FsChangeKind::Update)
            })
            .is_some(),
        "file creation was not observed before delete"
    );

    fs::remove_file(&file).expect("remove file");
    let event = session
        .collector()
        .wait_for(Duration::from_secs(3), |event| {
            event.absolute_path == file_path && event.kind == FsChangeKind::Delete
        })
        .expect("delete event");
    assert_eq!(event.is_directory, None);
}

#[test]
fn create_then_delete_within_window_is_coalesced() {
    let mut session = WatchSession::start("flash");
    let file = session.dir().join("flash.txt");
    let file_path = path_string(&file);
    fs::write(&file, "x").expect("write file");
    fs::remove_file(&file).expect("remove file");

    let create = session
        .collector()
        .wait_for(Duration::from_secs(2), |event| {
            event.absolute_path == file_path && event.kind == FsChangeKind::Create
        });
    assert!(
        create.is_none(),
        "create must be cancelled by a delete in the same window: {create:?}"
    );
}

#[test]
fn rename_emits_delete_for_source_and_create_for_destination() {
    let mut session = WatchSession::start("rename");
    let from = session.dir().join("old-name.txt");
    let to = session.dir().join("new-name.txt");
    let from_path = path_string(&from);
    let to_path = path_string(&to);

    fs::write(&from, "x").expect("write file");
    assert!(
        session
            .collector()
            .wait_for(Duration::from_secs(3), |event| {
                event.absolute_path == from_path
                    && matches!(event.kind, FsChangeKind::Create | FsChangeKind::Update)
            })
            .is_some(),
        "source creation was not observed before rename"
    );

    fs::rename(&from, &to).expect("rename");
    let deleted = session
        .collector()
        .wait_for(Duration::from_secs(3), |event| {
            event.absolute_path == from_path && event.kind == FsChangeKind::Delete
        });
    let created = session
        .collector()
        .wait_for(Duration::from_secs(3), |event| {
            event.absolute_path == to_path && event.kind == FsChangeKind::Create
        });
    assert!(
        deleted.is_some(),
        "rename must emit delete for the source path"
    );
    assert!(
        created.is_some(),
        "rename must emit create for the destination path"
    );
}
