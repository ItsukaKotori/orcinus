//! Dev-path watcher: recursive `notify` watches over the configured dev
//! plugin paths (§3.8) with the same batching contract as
//! `ade-fs/src/watch.rs` (trailing-edge debounce, hard cap measured from the
//! first raw event). Missing dev paths are skipped without error; [`stop`]
//! uninstalls the OS watcher and joins the flush thread.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::{Event, RecursiveMode, Watcher};

use crate::types::{PluginError, IO_ERROR, UNAVAILABLE};

/// Default trailing-edge debounce applied after the most recent raw event.
pub const DEV_WATCH_BATCH_TRAILING_MS: u64 = 150;
/// Hard cap measured from the first raw event in a batch.
pub const DEV_WATCH_BATCH_MAX_WAIT_MS: u64 = 500;
/// Flush-thread poll interval so `stop` never blocks on an idle receiver.
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Watches dev plugin paths and delivers coalesced change batches.
pub struct DevWatcher {
    paths: Vec<PathBuf>,
    batch: Duration,
    running: Option<Running>,
}

struct Running {
    /// Dropping the notify watcher stops the OS stream and its callback.
    watcher: notify::RecommendedWatcher,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl DevWatcher {
    /// Create a watcher over `paths`; `batch` is the trailing-edge debounce
    /// (defaults to [`DEV_WATCH_BATCH_TRAILING_MS`] at call sites).
    pub fn new(paths: &[PathBuf], batch: Duration) -> DevWatcher {
        Self {
            paths: paths.to_vec(),
            batch,
            running: None,
        }
    }

    /// Install recursive OS watches and deliver coalesced change batches
    /// (unique paths, arrival order) to `on_change`. A missing dev path is
    /// skipped without error.
    pub fn start(
        &mut self,
        on_change: impl Fn(Vec<PathBuf>) + Send + 'static,
    ) -> Result<(), PluginError> {
        if self.running.is_some() {
            return Err(PluginError::new(
                UNAVAILABLE,
                "dev watcher is already running",
            ));
        }

        let (sender, receiver) = mpsc::channel::<PathBuf>();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            if let Ok(event) = result {
                forward_event_paths(&sender, event);
            }
        })
        .map_err(|error| {
            PluginError::new(
                IO_ERROR,
                format!("failed to create dev path watcher: {error}"),
            )
        })?;

        for path in &self.paths {
            if !path.exists() {
                continue;
            }
            let _ = watcher.watch(path, RecursiveMode::Recursive);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let trailing = self.batch.max(Duration::from_millis(1));
        let max_wait = Duration::from_millis(DEV_WATCH_BATCH_MAX_WAIT_MS);
        let join = thread::spawn(move || {
            run_batch_worker(receiver, worker_stop, trailing, max_wait, on_change);
        });

        self.running = Some(Running {
            watcher,
            stop,
            join: Some(join),
        });
        Ok(())
    }

    /// Uninstall the OS watcher and join the flush thread. Idempotent; a
    /// pending batch is delivered before the thread exits.
    pub fn stop(&mut self) {
        let Some(mut running) = self.running.take() else {
            return;
        };
        running.stop.store(true, Ordering::SeqCst);
        // Why: dropping the notify watcher stops the OS stream and drops the
        // event callback (and its sender); the stop flag bounds the join even
        // if a backend thread lingers briefly.
        drop(running.watcher);
        if let Some(join) = running.join.take() {
            let _ = join.join();
        }
    }
}

fn forward_event_paths(sender: &Sender<PathBuf>, event: Event) {
    for path in event.paths {
        let _ = sender.send(path);
    }
}

/// Coalesce raw path events into trailing-edge batches capped at `max_wait`
/// from the first event, mirroring `ade-fs/src/watch.rs`. Deduplicates paths
/// per batch while preserving first-seen order.
fn run_batch_worker(
    receiver: Receiver<PathBuf>,
    stop: Arc<AtomicBool>,
    trailing: Duration,
    max_wait: Duration,
    on_change: impl Fn(Vec<PathBuf>) + Send + 'static,
) {
    let mut batch: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    loop {
        let first_at = match receiver.recv_timeout(STOP_POLL_INTERVAL) {
            Ok(path) => {
                push_unique(&mut batch, &mut seen, path);
                Instant::now()
            }
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };

        loop {
            let now = Instant::now();
            let deadline = (first_at + max_wait).min(now + trailing);
            let wait = deadline.saturating_duration_since(now);
            if wait.is_zero() {
                break;
            }
            match receiver.recv_timeout(wait) {
                Ok(path) => push_unique(&mut batch, &mut seen, path),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        if !batch.is_empty() {
            let paths = std::mem::take(&mut batch);
            seen.clear();
            on_change(paths);
        }

        if stop.load(Ordering::SeqCst) {
            return;
        }
    }
}

fn push_unique(batch: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        batch.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn canonical_dir(dir: &TempDir) -> PathBuf {
        fs::canonicalize(dir.path()).expect("canonicalize temp dir")
    }

    type BatchLog = Arc<Mutex<Vec<Vec<PathBuf>>>>;

    fn batch_sink() -> (BatchLog, impl Fn(Vec<PathBuf>) + Send + 'static) {
        let batches = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&batches);
        (batches, move |paths| {
            sink.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(paths)
        })
    }

    fn lock_batches(
        batches: &Mutex<Vec<Vec<PathBuf>>>,
    ) -> std::sync::MutexGuard<'_, Vec<Vec<PathBuf>>> {
        batches
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn contains_path(batches: &Mutex<Vec<Vec<PathBuf>>>, expected: &Path) -> bool {
        lock_batches(batches)
            .iter()
            .flatten()
            .any(|path| path == expected)
    }

    fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if condition() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn start_delivers_created_file_path_coalesced() {
        let dir = TempDir::new().expect("temp dir");
        let root = canonical_dir(&dir);
        let (batches, sink) = batch_sink();
        let mut watcher = DevWatcher::new(std::slice::from_ref(&root), Duration::from_millis(40));
        watcher.start(sink).expect("start watcher");

        let file = root.join("hello.txt");
        fs::write(&file, "x").expect("write file");

        assert!(
            wait_until(Duration::from_secs(10), || contains_path(&batches, &file)),
            "expected a change for {file:?}, got {:?}",
            lock_batches(&batches)
        );
        for batch in lock_batches(&batches).iter() {
            let unique: HashSet<&PathBuf> = batch.iter().collect();
            assert_eq!(unique.len(), batch.len(), "a batch must dedup paths");
        }
        watcher.stop();
    }

    #[test]
    fn start_tolerates_missing_paths() {
        let dir = TempDir::new().expect("temp dir");
        let missing = dir.path().join("missing-dev-plugin");
        let (batches, sink) = batch_sink();
        let mut watcher = DevWatcher::new(&[missing], Duration::from_millis(40));
        watcher
            .start(sink)
            .expect("a missing dev path is skipped without error");
        watcher.stop();
        assert!(lock_batches(&batches).is_empty());
    }

    #[test]
    fn watches_multiple_paths() {
        let dir_a = TempDir::new().expect("temp dir a");
        let dir_b = TempDir::new().expect("temp dir b");
        let root_a = canonical_dir(&dir_a);
        let root_b = canonical_dir(&dir_b);
        let (batches, sink) = batch_sink();
        let mut watcher =
            DevWatcher::new(&[root_a.clone(), root_b.clone()], Duration::from_millis(40));
        watcher.start(sink).expect("start watcher");

        let file_a = root_a.join("a.txt");
        let file_b = root_b.join("b.txt");
        fs::write(&file_a, "a").expect("write a");
        fs::write(&file_b, "b").expect("write b");

        assert!(
            wait_until(Duration::from_secs(10), || {
                contains_path(&batches, &file_a) && contains_path(&batches, &file_b)
            }),
            "expected changes for both roots, got {:?}",
            lock_batches(&batches)
        );
        watcher.stop();
    }

    #[test]
    fn stop_joins_worker_and_stops_delivery() {
        let dir = TempDir::new().expect("temp dir");
        let root = canonical_dir(&dir);
        let (batches, sink) = batch_sink();
        let mut watcher = DevWatcher::new(std::slice::from_ref(&root), Duration::from_millis(40));
        watcher.start(sink).expect("start watcher");
        assert!(watcher.running.is_some(), "worker thread is running");

        watcher.stop();
        assert!(watcher.running.is_none(), "stop clears the running state");
        watcher.stop();

        fs::write(root.join("after-stop.txt"), "x").expect("write file");
        thread::sleep(Duration::from_millis(400));
        assert!(
            lock_batches(&batches).is_empty(),
            "no events after stop, got {:?}",
            lock_batches(&batches)
        );
    }
}
