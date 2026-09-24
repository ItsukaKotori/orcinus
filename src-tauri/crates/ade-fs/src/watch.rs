use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::Serialize;

/// Trailing-edge debounce applied after the most recent raw watcher event.
pub const WATCH_BATCH_TRAILING_MS: u64 = 150;
/// Hard cap measured from the first raw event in a batch; a busy tree flushes
/// at this point even while events keep arriving.
pub const WATCH_BATCH_MAX_WAIT_MS: u64 = 500;
/// Raw events beyond this count in one batch collapse to a single `overflow`.
pub const MAX_BATCHED_WATCHER_EVENTS: usize = 5000;
/// High-churn directories filtered out before batching (notify offers no
/// daemon-side exclusion), so their events never count toward the batch cap
/// nor reach the renderer; mirrors the oracle `WATCHER_IGNORE_DIRS`.
pub const WATCHER_IGNORE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "dist",
    "build",
    ".next",
    ".cache",
    "target",
    ".venv",
    "__pycache__",
];
/// Grace period before a root with no subscribers is physically uninstalled.
pub const WATCHER_TEARDOWN_GRACE_MS: u64 = 30_000;
/// Upper bound for concurrent `stat` probes when filling `isDirectory`.
pub const DIRECTORY_STAT_CONCURRENCY: usize = 8;

/// Event kind union mirrored from the renderer contract
/// (`src/shared/filesystem-entry-types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FsChangeKind {
    Create,
    Update,
    Delete,
    Rename,
    Overflow,
}

/// One filesystem change delivered downstream.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChangeEvent {
    pub kind: FsChangeKind,
    pub absolute_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_absolute_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_directory: Option<bool>,
}

/// Batch payload delivered to the downstream emitter callback.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChangedPayload {
    pub worktree_path: String,
    pub events: Vec<FsChangeEvent>,
}

/// Backend-agnostic raw event handed to [`coalesce_events`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawEvent {
    Create(PathBuf),
    Update(PathBuf),
    Delete(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
}

/// Coalesce one batch of raw watcher events into the downstream event list.
///
/// Rules (oracle parity, `filesystem-watcher-local-events.ts`):
/// - one event per path wins by recency;
/// - `delete -> create` keeps both, ordered delete then create;
/// - `create -> delete` cancels both;
/// - a rename folds into `delete(from) + create(to)`;
/// - events inside [`WATCHER_IGNORE_DIRS`] (relative to the watched root) are
///   dropped, including either side of a rename;
/// - more than [`MAX_BATCHED_WATCHER_EVENTS`] raw events become a single
///   `overflow` with `absolutePath = root`.
pub fn coalesce_events(root: &str, raw: Vec<RawEvent>) -> Vec<FsChangeEvent> {
    coalesce_events_with_ignore_roots(std::slice::from_ref(&PathBuf::from(root)), root, raw)
}

fn coalesce_events_with_ignore_roots(
    ignore_roots: &[PathBuf],
    root: &str,
    raw: Vec<RawEvent>,
) -> Vec<FsChangeEvent> {
    if raw.len() > MAX_BATCHED_WATCHER_EVENTS {
        return vec![overflow_event(root)];
    }

    let mut last_by_path = OrderedLastEvents::default();
    let mut delete_before_create = OrderedPathSet::default();

    for event in raw {
        match event {
            RawEvent::Rename { from, to } => {
                apply_coalesced(
                    &mut last_by_path,
                    &mut delete_before_create,
                    FsChangeKind::Delete,
                    from,
                );
                apply_coalesced(
                    &mut last_by_path,
                    &mut delete_before_create,
                    FsChangeKind::Create,
                    to,
                );
            }
            RawEvent::Create(path) => apply_coalesced(
                &mut last_by_path,
                &mut delete_before_create,
                FsChangeKind::Create,
                path,
            ),
            RawEvent::Update(path) => apply_coalesced(
                &mut last_by_path,
                &mut delete_before_create,
                FsChangeKind::Update,
                path,
            ),
            RawEvent::Delete(path) => apply_coalesced(
                &mut last_by_path,
                &mut delete_before_create,
                FsChangeKind::Delete,
                path,
            ),
        }
    }

    let mut events = Vec::new();
    // Why: a path that was deleted and re-created emits the delete first so the
    // renderer clears the old subtree before re-reading the new entry.
    for path in delete_before_create.iter() {
        if !is_ignored_path(ignore_roots, path) {
            events.push(FsChangeEvent::new(FsChangeKind::Delete, path));
        }
    }
    for (path, kind) in last_by_path.iter() {
        if !is_ignored_path(ignore_roots, path) {
            events.push(FsChangeEvent::new(*kind, path));
        }
    }
    events
}

fn apply_coalesced(
    last_by_path: &mut OrderedLastEvents,
    delete_before_create: &mut OrderedPathSet,
    kind: FsChangeKind,
    path: PathBuf,
) {
    if let Some(previous) = last_by_path.get(&path) {
        if previous == FsChangeKind::Delete && kind == FsChangeKind::Create {
            delete_before_create.insert(path.clone());
        }
        // Why: create followed by delete nets out to nothing; keeping the
        // create would make the renderer add a path that no longer exists.
        if previous == FsChangeKind::Create && kind == FsChangeKind::Delete {
            last_by_path.remove(&path);
            delete_before_create.remove(&path);
            return;
        }
    }

    last_by_path.set(path.clone(), kind);

    // Why: a later non-create event supersedes the pending delete, otherwise
    // the output would carry a spurious delete for a path that still exists.
    if kind != FsChangeKind::Create {
        delete_before_create.remove(&path);
    }
}

fn overflow_event(root: &str) -> FsChangeEvent {
    FsChangeEvent {
        kind: FsChangeKind::Overflow,
        absolute_path: root.to_string(),
        old_absolute_path: None,
        is_directory: None,
    }
}

fn is_ignored_path(ignore_roots: &[PathBuf], path: &Path) -> bool {
    for root in ignore_roots {
        if let Ok(relative) = path.strip_prefix(root) {
            return relative.components().any(is_ignored_component);
        }
    }
    path.components().any(is_ignored_component)
}

fn is_ignored_component(component: Component<'_>) -> bool {
    match component {
        Component::Normal(name) => WATCHER_IGNORE_DIRS
            .iter()
            .any(|ignored| name == OsStr::new(ignored)),
        _ => false,
    }
}

impl FsChangeEvent {
    fn new(kind: FsChangeKind, path: &Path) -> Self {
        Self {
            kind,
            absolute_path: path.to_string_lossy().into_owned(),
            old_absolute_path: None,
            is_directory: None,
        }
    }
}

/// Insertion-ordered map of path -> last observed event kind. Rust's
/// `HashMap` has no stable iteration order, and the oracle emits paths in the
/// order of their first insertion (a JS `Map` keeps the original position when
/// a key is set again), so slots plus an index keep that contract.
#[derive(Default)]
struct OrderedLastEvents {
    slots: Vec<Option<(PathBuf, FsChangeKind)>>,
    index: HashMap<PathBuf, usize>,
}

impl OrderedLastEvents {
    fn get(&self, path: &Path) -> Option<FsChangeKind> {
        self.index
            .get(path)
            .and_then(|index| self.slots[*index].as_ref())
            .map(|(_, kind)| *kind)
    }

    fn set(&mut self, path: PathBuf, kind: FsChangeKind) {
        if let Some(index) = self.index.get(&path).copied() {
            self.slots[index] = Some((path, kind));
            return;
        }
        let index = self.slots.len();
        self.index.insert(path.clone(), index);
        self.slots.push(Some((path, kind)));
    }

    fn remove(&mut self, path: &Path) -> bool {
        match self.index.remove(path) {
            Some(index) => {
                self.slots[index] = None;
                true
            }
            None => false,
        }
    }

    fn iter(&self) -> impl Iterator<Item = &(PathBuf, FsChangeKind)> {
        self.slots.iter().flatten()
    }
}

/// Insertion-ordered set mirroring the oracle's `deleteBeforeCreate` Set.
#[derive(Default)]
struct OrderedPathSet {
    slots: Vec<Option<PathBuf>>,
    index: HashMap<PathBuf, usize>,
}

impl OrderedPathSet {
    fn insert(&mut self, path: PathBuf) {
        if self.index.contains_key(&path) {
            return;
        }
        let index = self.slots.len();
        self.index.insert(path.clone(), index);
        self.slots.push(Some(path));
    }

    fn remove(&mut self, path: &Path) {
        if let Some(index) = self.index.remove(path) {
            self.slots[index] = None;
        }
    }

    fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.slots.iter().flatten()
    }
}

// ── Batch buffering ──────────────────────────────────────────────────

/// Message handed from the notify backend thread to a root's flush worker.
enum WatchMsg {
    Raw(RawEvent),
    /// Watcher error or dropped-event rescan; collapses the batch downstream.
    Overflow,
}

#[derive(Default)]
struct BatchBuffer {
    events: Vec<RawEvent>,
    overflowed: bool,
}

impl BatchBuffer {
    fn apply(&mut self, msg: WatchMsg) {
        match msg {
            WatchMsg::Overflow => self.mark_overflow(),
            WatchMsg::Raw(event) => self.push(event),
        }
    }

    fn push(&mut self, event: RawEvent) {
        if self.overflowed {
            return;
        }
        if self.events.len() + 1 > MAX_BATCHED_WATCHER_EVENTS {
            // Why: once precision is too expensive, keeping every path only
            // burns memory before the flush sends the same overflow refresh.
            self.mark_overflow();
            return;
        }
        self.events.push(event);
    }

    fn mark_overflow(&mut self) {
        self.events.clear();
        self.overflowed = true;
    }

    fn take(&mut self) -> (Vec<RawEvent>, bool) {
        (
            std::mem::take(&mut self.events),
            std::mem::take(&mut self.overflowed),
        )
    }
}

// ── Concurrency helpers ──────────────────────────────────────────────

/// Run `map` over `items` on at most `concurrency` scoped threads, preserving
/// input order in the output. Used for the `isDirectory` probe so a large
/// deletion batch cannot swamp the blocking pool.
fn map_with_concurrency<T, R>(
    items: &[T],
    concurrency: usize,
    map: impl Fn(&T) -> R + Sync,
) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    if items.is_empty() {
        return Vec::new();
    }
    let workers = concurrency.max(1).min(items.len());
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= items.len() {
                    break;
                }
                let value = map(&items[index]);
                lock(&results)[index] = Some(value);
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .into_iter()
        .map(|value| value.expect("every index is assigned exactly once"))
        .collect()
}

/// Outcome of stat-ing an event path at flush time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathProbe {
    Directory,
    File,
    /// The path no longer exists; a surviving create/update for it is stale.
    Missing,
    /// The path could not be classified (dangling symlink, permission error,
    /// fd exhaustion): keep the reported kind and leave `isDirectory` unset.
    Unknown,
}

/// Classify the path, following symlinks for the file/directory answer.
fn probe_path(path: &Path) -> PathProbe {
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                PathProbe::Directory
            } else {
                PathProbe::File
            }
        }
        Err(_) => match fs::symlink_metadata(path) {
            // Why: only NotFound means the path is gone. A dangling symlink is
            // still a directory entry (parcel's O_SYMLINK existence probe), and
            // EACCES/EPERM/EMFILE/EIO must not turn a create/update into a
            // delete.
            Err(error) if error.kind() == io::ErrorKind::NotFound => PathProbe::Missing,
            _ => PathProbe::Unknown,
        },
    }
}

// ── Clock ────────────────────────────────────────────────────────────

/// Injectable time source; tests replace it to drive the teardown grace
/// period without sleeping.
pub trait WatchClock: Send + Sync + 'static {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
pub struct SystemWatchClock;

impl WatchClock for SystemWatchClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Manually advanced clock for tests.
#[derive(Debug)]
pub struct ManualWatchClock {
    base: Instant,
    offset: Mutex<Duration>,
}

impl ManualWatchClock {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            offset: Mutex::new(Duration::ZERO),
        }
    }

    pub fn advance(&self, delta: Duration) {
        *lock(&self.offset) += delta;
    }
}

impl Default for ManualWatchClock {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchClock for ManualWatchClock {
    fn now(&self) -> Instant {
        self.base + *lock(&self.offset)
    }
}

// ── Subscription management ──────────────────────────────────────────

type SharedCallback = Arc<dyn Fn(FsChangedPayload) + Send + Sync + 'static>;

/// Per-root watcher shared by all subscribers of that root.
struct RootWatch {
    /// Dropping the notify watcher stops the OS stream and disconnects the
    /// flush worker.
    _watcher: notify::RecommendedWatcher,
    subscribers: HashSet<String>,
    /// Set when the last subscriber leaves; a new subscriber clears it.
    pending_drop_at: Option<Instant>,
}

struct WatcherState {
    clock: Arc<dyn WatchClock>,
    roots: Mutex<HashMap<PathBuf, RootWatch>>,
    /// Roots whose install failed; retries are suppressed (negative cache).
    failed_roots: Mutex<HashSet<PathBuf>>,
    callback: Mutex<Option<SharedCallback>>,
}

/// Root-level shared filesystem watcher.
///
/// `watch`/`unwatch` are reference-counted per root: the OS watcher is
/// installed once and reused while any subscriber remains. The last
/// subscriber leaving only schedules teardown [`WATCHER_TEARDOWN_GRACE_MS`]
/// later, so rapid worktree switches reuse the native stream. Install
/// failures are negatively cached and surface downstream as a single
/// `overflow`.
pub struct FsWatcher {
    inner: Arc<WatcherState>,
}

impl FsWatcher {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemWatchClock))
    }

    pub fn with_clock(clock: Arc<dyn WatchClock>) -> Self {
        Self {
            inner: Arc::new(WatcherState {
                clock,
                roots: Mutex::new(HashMap::new()),
                failed_roots: Mutex::new(HashSet::new()),
                callback: Mutex::new(None),
            }),
        }
    }

    /// Register the downstream emitter callback. Task 8 wires this to Tauri
    /// events; calling again replaces the previous callback.
    pub fn subscribe<F>(&self, callback: F)
    where
        F: Fn(FsChangedPayload) + Send + Sync + 'static,
    {
        *lock(&self.inner.callback) = Some(Arc::new(callback));
    }

    /// Add a subscriber to `root`, installing the watcher on first use.
    /// Failures are negatively cached and reported downstream as `overflow`.
    pub fn watch(&self, root: &str, subscriber_id: &str) {
        let key = PathBuf::from(root);
        if lock(&self.inner.failed_roots).contains(&key) {
            return;
        }

        let mut roots = lock(&self.inner.roots);
        if let Some(existing) = roots.get_mut(&key) {
            existing.subscribers.insert(subscriber_id.to_string());
            existing.pending_drop_at = None;
            return;
        }

        let ignore_roots = ignore_roots_for(&key);
        match install_root_watch(&key, &ignore_roots) {
            Ok((mut root_watch, receiver)) => {
                root_watch.subscribers.insert(subscriber_id.to_string());
                roots.insert(key.clone(), root_watch);
                drop(roots);
                let state = Arc::downgrade(&self.inner);
                thread::spawn(move || run_flush_worker(key, ignore_roots, state, receiver));
            }
            Err(_) => {
                drop(roots);
                self.mark_failed(&key);
            }
        }
    }

    /// Remove a subscriber from `root`; unknown roots/subscribers are a no-op.
    pub fn unwatch(&self, root: &str, subscriber_id: &str) {
        let key = PathBuf::from(root);
        let mut roots = lock(&self.inner.roots);
        let Some(existing) = roots.get_mut(&key) else {
            return;
        };
        existing.subscribers.remove(subscriber_id);
        if !existing.subscribers.is_empty() || existing.pending_drop_at.is_some() {
            return;
        }
        existing.pending_drop_at =
            Some(self.inner.clock.now() + Duration::from_millis(WATCHER_TEARDOWN_GRACE_MS));
        drop(roots);

        // Why: the timer only matters for real time; tests drive
        // `sweep_expired_roots` with an injected clock instead of sleeping.
        let state = Arc::downgrade(&self.inner);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(WATCHER_TEARDOWN_GRACE_MS));
            if let Some(state) = state.upgrade() {
                state.sweep_expired_roots();
            }
        });
    }

    /// Number of roots with a live OS watcher; exposed for tests and
    /// diagnostics.
    pub fn watched_root_count(&self) -> usize {
        lock(&self.inner.roots).len()
    }

    fn mark_failed(&self, root: &Path) {
        lock(&self.inner.failed_roots).insert(root.to_path_buf());
        let callback = lock(&self.inner.callback).clone();
        if let Some(callback) = callback {
            let root_string = root.to_string_lossy().into_owned();
            callback(FsChangedPayload {
                worktree_path: root_string.clone(),
                events: vec![overflow_event(&root_string)],
            });
        }
    }
}

impl Default for FsWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl WatcherState {
    /// Drop every root whose grace period elapsed without new subscribers.
    fn sweep_expired_roots(&self) {
        let now = self.clock.now();
        let mut expired = Vec::new();
        {
            let mut roots = lock(&self.roots);
            let keys: Vec<PathBuf> = roots
                .iter()
                .filter(|(_, watch)| {
                    watch.subscribers.is_empty()
                        && watch.pending_drop_at.is_some_and(|at| at <= now)
                })
                .map(|(path, _)| path.clone())
                .collect();
            for key in keys {
                if let Some(watch) = roots.remove(&key) {
                    expired.push(watch);
                }
            }
        }
        // Drop outside the lock: uninstalling joins the OS watcher thread.
        drop(expired);
    }
}

fn install_root_watch(
    root: &Path,
    ignore_roots: &[PathBuf],
) -> notify::Result<(RootWatch, Receiver<WatchMsg>)> {
    let (sender, receiver) = mpsc::channel();
    let filter_roots = ignore_roots.to_vec();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        match result {
            Ok(event) => {
                if event.need_rescan() {
                    // Why: dropped FSEvents/inotify events lose path precision;
                    // one overflow asks the renderer for a conservative refresh.
                    let _ = sender.send(WatchMsg::Overflow);
                    return;
                }
                for raw in raw_events_from_notify(event) {
                    forward_raw_event(&filter_roots, &sender, raw);
                }
            }
            Err(_) => {
                let _ = sender.send(WatchMsg::Overflow);
            }
        }
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok((
        RootWatch {
            _watcher: watcher,
            subscribers: HashSet::new(),
            pending_drop_at: None,
        },
        receiver,
    ))
}

/// Bases used for the ignore-directory check. macOS FSEvents reports
/// canonical paths, so a symlinked root needs its canonical spelling too.
fn ignore_roots_for(root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![root.to_path_buf()];
    if let Ok(canonical) = fs::canonicalize(root) {
        if canonical != roots[0] {
            roots.push(canonical);
        }
    }
    roots
}

fn run_flush_worker(
    root: PathBuf,
    ignore_roots: Vec<PathBuf>,
    state: Weak<WatcherState>,
    receiver: Receiver<WatchMsg>,
) {
    let trailing = Duration::from_millis(WATCH_BATCH_TRAILING_MS);
    let max_wait = Duration::from_millis(WATCH_BATCH_MAX_WAIT_MS);
    let mut buffer = BatchBuffer::default();

    loop {
        match receiver.recv() {
            Ok(msg) => buffer.apply(msg),
            // All senders dropped: the root watch was torn down.
            Err(_) => return,
        }

        let first_at = Instant::now();
        loop {
            let now = Instant::now();
            // Trailing-edge debounce, capped at max wait from the first event.
            let deadline = (first_at + max_wait).min(now + trailing);
            let wait = deadline.saturating_duration_since(now);
            if wait.is_zero() {
                break;
            }
            match receiver.recv_timeout(wait) {
                Ok(msg) => buffer.apply(msg),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        let Some(state) = state.upgrade() else {
            return;
        };
        flush_batch(&root, &ignore_roots, &state, &mut buffer);
    }
}

fn flush_batch(
    root: &Path,
    ignore_roots: &[PathBuf],
    state: &WatcherState,
    buffer: &mut BatchBuffer,
) {
    let (raw, overflowed) = buffer.take();
    if raw.is_empty() && !overflowed {
        return;
    }

    let root_string = root.to_string_lossy().into_owned();
    let callback = {
        let roots = lock(&state.roots);
        match roots.get(root) {
            // Why: during the teardown grace period there is no subscriber to
            // deliver to; skip the payload instead of emitting into the void.
            Some(watch) if !watch.subscribers.is_empty() => {}
            _ => return,
        }
        lock(&state.callback).clone()
    };
    let Some(callback) = callback else {
        return;
    };

    let events = if overflowed || raw.len() > MAX_BATCHED_WATCHER_EVENTS {
        vec![overflow_event(&root_string)]
    } else {
        let coalesced = coalesce_events_with_ignore_roots(ignore_roots, &root_string, raw);
        if coalesced.is_empty() {
            return;
        }
        let probed = map_with_concurrency(&coalesced, DIRECTORY_STAT_CONCURRENCY, |event| {
            // Why: a deleted path cannot be stat'd; leave isDirectory undefined
            // and let the renderer infer it from its dir cache.
            if event.kind == FsChangeKind::Delete {
                return (FsChangeKind::Delete, None);
            }
            match probe_path(Path::new(&event.absolute_path)) {
                PathProbe::Directory => (event.kind, Some(true)),
                PathProbe::File => (event.kind, Some(false)),
                // Why: FSEvents coalesces a delete's flags with create/modify
                // bits, so the surviving last event can be a create/update for
                // a path that no longer exists; only a NotFound stat downgrades
                // it to delete (parcel's ambiguous-flag stat does the same).
                PathProbe::Missing => (FsChangeKind::Delete, None),
                PathProbe::Unknown => (event.kind, None),
            }
        });
        coalesced
            .into_iter()
            .zip(probed)
            .map(|(mut event, (kind, is_directory))| {
                event.kind = kind;
                event.is_directory = is_directory;
                event
            })
            .collect()
    };

    callback(FsChangedPayload {
        worktree_path: root_string,
        events,
    });
}

fn raw_events_from_notify(event: Event) -> Vec<RawEvent> {
    let Event { kind, paths, .. } = event;
    match kind {
        EventKind::Create(_) => paths.into_iter().map(RawEvent::Create).collect(),
        EventKind::Remove(_) => paths.into_iter().map(RawEvent::Delete).collect(),
        EventKind::Modify(ModifyKind::Name(mode)) => match mode {
            RenameMode::Both if paths.len() >= 2 => vec![RawEvent::Rename {
                from: paths[0].clone(),
                to: paths[1].clone(),
            }],
            RenameMode::From => paths.into_iter().map(RawEvent::Delete).collect(),
            RenameMode::To => paths.into_iter().map(RawEvent::Create).collect(),
            // FSEvents does not correlate the two sides of a rename; existence
            // disambiguates the renamed path (parcel's FSEvents backend does
            // the same).
            _ => paths.into_iter().map(resolve_ambiguous_rename).collect(),
        },
        EventKind::Modify(_) => paths.into_iter().map(RawEvent::Update).collect(),
        // Imprecise backends collapse every kind into `Any`; a refresh is the
        // safe interpretation.
        EventKind::Any => paths.into_iter().map(RawEvent::Update).collect(),
        EventKind::Access(_) | EventKind::Other => Vec::new(),
    }
}

fn resolve_ambiguous_rename(path: PathBuf) -> RawEvent {
    // `symlink_metadata` counts a broken symlink as existing, matching
    // parcel's O_SYMLINK existence probe.
    if fs::symlink_metadata(&path).is_ok() {
        RawEvent::Create(path)
    } else {
        RawEvent::Delete(path)
    }
}

/// Drop ignored-directory events before they reach the batch counter, so a
/// `node_modules`/`.git` storm can neither fill the 5000-event cap nor emit a
/// spurious overflow. A rename is dropped only when both sides are ignored;
/// coalesce later discards whichever side is ignored.
fn forward_raw_event(ignore_roots: &[PathBuf], sender: &Sender<WatchMsg>, event: RawEvent) {
    if raw_event_is_ignored(ignore_roots, &event) {
        return;
    }
    let _ = sender.send(WatchMsg::Raw(event));
}

fn raw_event_is_ignored(ignore_roots: &[PathBuf], event: &RawEvent) -> bool {
    match event {
        RawEvent::Create(path) | RawEvent::Update(path) | RawEvent::Delete(path) => {
            is_ignored_path(ignore_roots, path)
        }
        RawEvent::Rename { from, to } => {
            is_ignored_path(ignore_roots, from) && is_ignored_path(ignore_roots, to)
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    const ROOT: &str = "/work";

    fn create(path: &str) -> RawEvent {
        RawEvent::Create(PathBuf::from(path))
    }

    fn update(path: &str) -> RawEvent {
        RawEvent::Update(PathBuf::from(path))
    }

    fn delete(path: &str) -> RawEvent {
        RawEvent::Delete(PathBuf::from(path))
    }

    fn rename(from: &str, to: &str) -> RawEvent {
        RawEvent::Rename {
            from: PathBuf::from(from),
            to: PathBuf::from(to),
        }
    }

    fn event(kind: FsChangeKind, path: &str) -> FsChangeEvent {
        FsChangeEvent {
            kind,
            absolute_path: path.to_string(),
            old_absolute_path: None,
            is_directory: None,
        }
    }

    #[test]
    fn coalesce_keeps_last_event_per_path() {
        let events = coalesce_events(
            ROOT,
            vec![
                create("/work/a.txt"),
                update("/work/a.txt"),
                update("/work/a.txt"),
            ],
        );
        assert_eq!(events, vec![event(FsChangeKind::Update, "/work/a.txt")]);
    }

    #[test]
    fn coalesce_delete_then_create_keeps_both_in_order() {
        let events = coalesce_events(ROOT, vec![delete("/work/a.txt"), create("/work/a.txt")]);
        assert_eq!(
            events,
            vec![
                event(FsChangeKind::Delete, "/work/a.txt"),
                event(FsChangeKind::Create, "/work/a.txt"),
            ]
        );
    }

    #[test]
    fn coalesce_create_then_delete_cancels() {
        let events = coalesce_events(ROOT, vec![create("/work/a.txt"), delete("/work/a.txt")]);
        assert!(events.is_empty());
    }

    #[test]
    fn coalesce_delete_create_update_drops_stale_delete() {
        // Oracle: a non-create event after delete→create supersedes the stale
        // delete, so only the update survives.
        let events = coalesce_events(
            ROOT,
            vec![
                delete("/work/a.txt"),
                create("/work/a.txt"),
                update("/work/a.txt"),
            ],
        );
        assert_eq!(events, vec![event(FsChangeKind::Update, "/work/a.txt")]);
    }

    #[test]
    fn coalesce_delete_create_create_keeps_delete_then_create() {
        let events = coalesce_events(
            ROOT,
            vec![
                delete("/work/a.txt"),
                create("/work/a.txt"),
                create("/work/a.txt"),
            ],
        );
        assert_eq!(
            events,
            vec![
                event(FsChangeKind::Delete, "/work/a.txt"),
                event(FsChangeKind::Create, "/work/a.txt"),
            ]
        );
    }

    #[test]
    fn coalesce_create_delete_create_keeps_single_create() {
        let events = coalesce_events(
            ROOT,
            vec![
                create("/work/a.txt"),
                delete("/work/a.txt"),
                create("/work/a.txt"),
            ],
        );
        assert_eq!(events, vec![event(FsChangeKind::Create, "/work/a.txt")]);
    }

    #[test]
    fn coalesce_folds_rename_into_delete_and_create() {
        let events = coalesce_events(ROOT, vec![rename("/work/old.txt", "/work/new.txt")]);
        assert_eq!(
            events,
            vec![
                event(FsChangeKind::Delete, "/work/old.txt"),
                event(FsChangeKind::Create, "/work/new.txt"),
            ]
        );
    }

    #[test]
    fn coalesce_rename_to_ignored_directory_keeps_only_delete() {
        let events = coalesce_events(
            ROOT,
            vec![rename("/work/src/a.rs", "/work/node_modules/a.rs")],
        );
        assert_eq!(events, vec![event(FsChangeKind::Delete, "/work/src/a.rs")]);
    }

    #[test]
    fn coalesce_rename_from_ignored_directory_keeps_only_create() {
        let events = coalesce_events(ROOT, vec![rename("/work/target/a.rs", "/work/src/a.rs")]);
        assert_eq!(events, vec![event(FsChangeKind::Create, "/work/src/a.rs")]);
    }

    #[test]
    fn coalesce_filters_events_inside_ignored_directories() {
        let events = coalesce_events(
            ROOT,
            vec![
                create("/work/node_modules/pkg/index.js"),
                update("/work/.git/index"),
                create("/work/src/main.rs"),
            ],
        );
        assert_eq!(
            events,
            vec![event(FsChangeKind::Create, "/work/src/main.rs")]
        );
    }

    #[test]
    fn coalesce_ignore_check_is_relative_to_root() {
        let events = coalesce_events("/work/target", vec![create("/work/target/src/main.rs")]);
        assert_eq!(
            events,
            vec![event(FsChangeKind::Create, "/work/target/src/main.rs")]
        );
    }

    #[test]
    fn coalesce_overflow_over_event_cap() {
        let raw = (0..=MAX_BATCHED_WATCHER_EVENTS)
            .map(|index| create(&format!("/work/file-{index}.txt")))
            .collect();
        let events = coalesce_events(ROOT, raw);
        assert_eq!(events, vec![event(FsChangeKind::Overflow, ROOT)]);
    }

    #[test]
    fn coalesce_at_event_cap_is_not_overflow() {
        let raw = (0..MAX_BATCHED_WATCHER_EVENTS)
            .map(|index| create(&format!("/work/file-{index}.txt")))
            .collect();
        let events = coalesce_events(ROOT, raw);
        assert_eq!(events.len(), MAX_BATCHED_WATCHER_EVENTS);
        assert!(events.iter().all(|evt| evt.kind == FsChangeKind::Create));
    }

    // ── Batch / probe / subscription ─────────────────────────────────

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let unique = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let raw = std::env::temp_dir().join(format!(
                "ade-fs-watch-unit-{name}-{}-{unique}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&raw);
            fs::create_dir_all(&raw).expect("create temp dir");
            Self {
                path: fs::canonicalize(&raw).expect("canonicalize temp dir"),
            }
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
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn payload_sink() -> (
        Arc<Mutex<Vec<FsChangedPayload>>>,
        impl Fn(FsChangedPayload) + Send + Sync + 'static,
    ) {
        let payloads = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&payloads);
        (payloads, move |payload| lock(&sink).push(payload))
    }

    #[test]
    fn batch_overflow_after_event_cap() {
        let mut buffer = BatchBuffer::default();
        for index in 0..MAX_BATCHED_WATCHER_EVENTS {
            buffer.push(create(&format!("/work/file-{index}.txt")));
        }
        assert_eq!(buffer.events.len(), MAX_BATCHED_WATCHER_EVENTS);
        assert!(!buffer.overflowed);

        buffer.push(create("/work/one-more.txt"));
        assert!(buffer.overflowed);
        assert!(buffer.events.is_empty());

        let (raw, overflowed) = buffer.take();
        assert!(raw.is_empty());
        assert!(overflowed);
        assert!(!buffer.overflowed);
    }

    #[test]
    fn batch_overflow_discards_later_events() {
        let mut buffer = BatchBuffer::default();
        buffer.mark_overflow();
        buffer.push(create("/work/a.txt"));
        assert!(buffer.events.is_empty());
        assert!(buffer.overflowed);
    }

    #[test]
    fn map_with_concurrency_preserves_order() {
        let items: Vec<usize> = (0..64).collect();
        let mapped = map_with_concurrency(&items, 8, |value| value * 2);
        let expected: Vec<usize> = items.iter().map(|value| value * 2).collect();
        assert_eq!(mapped, expected);
    }

    #[test]
    fn probe_path_classifies_existing_missing_and_unstatable() {
        let dir = TempDir::new("probe");
        let file = dir.join("file.txt");
        fs::write(&file, "x").expect("write file");
        let sub = dir.join("sub");
        fs::create_dir(&sub).expect("create dir");

        assert_eq!(probe_path(&file), PathProbe::File);
        assert_eq!(probe_path(&sub), PathProbe::Directory);
        assert_eq!(probe_path(&dir.join("missing.txt")), PathProbe::Missing);
        // ENOTDIR: a non-NotFound stat error must stay Unknown, not Missing.
        assert_eq!(
            probe_path(&file.join("child")),
            PathProbe::Unknown,
            "a non-NotFound stat failure must not be treated as a deletion"
        );
    }

    #[cfg(unix)]
    #[test]
    fn probe_path_treats_dangling_symlink_as_unknown() {
        let dir = TempDir::new("probe-symlink");
        let dangling = dir.join("dangling-link");
        std::os::unix::fs::symlink(dir.join("missing-target"), &dangling).expect("create symlink");

        // Why: a dangling symlink is still a directory entry (parcel's
        // O_SYMLINK probe), so it must not downgrade a create/update.
        assert_eq!(probe_path(&dangling), PathProbe::Unknown);
    }

    #[test]
    fn flush_fills_is_directory_for_create_and_update_only() {
        let dir = TempDir::new("flush");
        let file = dir.join("file.txt");
        fs::write(&file, "x").expect("write file");
        let sub = dir.join("sub");
        fs::create_dir(&sub).expect("create dir");
        let missing = dir.join("missing.txt");

        let watcher = FsWatcher::new();
        let (payloads, callback) = payload_sink();
        watcher.subscribe(callback);
        watcher.watch(dir.str(), "subscriber");

        let mut buffer = BatchBuffer::default();
        buffer.push(RawEvent::Create(file.clone()));
        buffer.push(RawEvent::Create(sub.clone()));
        buffer.push(RawEvent::Update(file.clone()));
        buffer.push(RawEvent::Delete(missing.clone()));
        flush_batch(
            dir.path(),
            &[dir.path().to_path_buf()],
            &watcher.inner,
            &mut buffer,
        );

        let payloads = lock(&payloads);
        assert_eq!(payloads.len(), 1);
        let payload = &payloads[0];
        assert_eq!(payload.worktree_path, dir.str());

        let find = |path: &Path| {
            payload
                .events
                .iter()
                .find(|event| Path::new(&event.absolute_path) == path)
                .unwrap_or_else(|| panic!("missing event for {}", path.display()))
        };

        assert_eq!(find(&file).kind, FsChangeKind::Update);
        assert_eq!(find(&file).is_directory, Some(false));
        assert_eq!(find(&sub).kind, FsChangeKind::Create);
        assert_eq!(find(&sub).is_directory, Some(true));
        assert_eq!(find(&missing).kind, FsChangeKind::Delete);
        assert_eq!(find(&missing).is_directory, None);
    }

    #[test]
    fn flush_downgrades_vanished_create_or_update_to_delete() {
        let dir = TempDir::new("flush-vanished");
        let missing = dir.join("gone.txt");
        let watcher = FsWatcher::new();
        let (payloads, callback) = payload_sink();
        watcher.subscribe(callback);
        watcher.watch(dir.str(), "subscriber");

        let mut buffer = BatchBuffer::default();
        buffer.push(RawEvent::Create(missing.clone()));
        buffer.push(RawEvent::Update(missing.clone()));
        flush_batch(
            dir.path(),
            &[dir.path().to_path_buf()],
            &watcher.inner,
            &mut buffer,
        );

        let payloads = lock(&payloads);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].events.len(), 1);
        assert_eq!(payloads[0].events[0].kind, FsChangeKind::Delete);
        assert_eq!(payloads[0].events[0].is_directory, None);
    }

    #[test]
    fn flush_keeps_kind_for_unstatable_path() {
        let dir = TempDir::new("flush-unstatable");
        let file = dir.join("file.txt");
        fs::write(&file, "x").expect("write file");
        // ENOTDIR makes stat fail with a non-NotFound error.
        let unstatable = file.join("child");

        let watcher = FsWatcher::new();
        let (payloads, callback) = payload_sink();
        watcher.subscribe(callback);
        watcher.watch(dir.str(), "subscriber");

        let mut buffer = BatchBuffer::default();
        buffer.push(RawEvent::Create(unstatable.clone()));
        flush_batch(
            dir.path(),
            &[dir.path().to_path_buf()],
            &watcher.inner,
            &mut buffer,
        );

        let payloads = lock(&payloads);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].events.len(), 1);
        assert_eq!(payloads[0].events[0].kind, FsChangeKind::Create);
        assert_eq!(payloads[0].events[0].is_directory, None);
    }

    #[test]
    fn ignored_directory_events_are_dropped_before_batching() {
        let ignore_roots = vec![PathBuf::from(ROOT)];
        let (sender, receiver) = mpsc::channel();
        for index in 0..(MAX_BATCHED_WATCHER_EVENTS + 10) {
            forward_raw_event(
                &ignore_roots,
                &sender,
                create(&format!("/work/node_modules/pkg-{index}.js")),
            );
        }
        assert!(
            receiver.try_recv().is_err(),
            "an ignored-directory storm must not reach the batch counter"
        );

        forward_raw_event(&ignore_roots, &sender, create("/work/src/main.rs"));
        assert!(matches!(
            receiver.try_recv(),
            Ok(WatchMsg::Raw(RawEvent::Create(path))) if path == Path::new("/work/src/main.rs")
        ));
    }

    #[test]
    fn ignored_rename_is_dropped_only_when_both_sides_are_ignored() {
        let ignore_roots = vec![PathBuf::from(ROOT)];
        let (sender, receiver) = mpsc::channel();

        forward_raw_event(
            &ignore_roots,
            &sender,
            rename("/work/node_modules/a.js", "/work/node_modules/b.js"),
        );
        assert!(receiver.try_recv().is_err());

        forward_raw_event(
            &ignore_roots,
            &sender,
            rename("/work/src/a.rs", "/work/node_modules/a.rs"),
        );
        assert!(matches!(
            receiver.try_recv(),
            Ok(WatchMsg::Raw(RawEvent::Rename { .. }))
        ));
    }

    #[test]
    fn raw_events_from_notify_maps_kinds_and_renames() {
        use notify::event::{AccessKind, CreateKind, DataChange, RemoveKind};

        let root = PathBuf::from("/work");
        let file = root.join("a.txt");

        assert_eq!(
            raw_events_from_notify(
                Event::new(EventKind::Create(CreateKind::File)).add_path(file.clone())
            ),
            vec![RawEvent::Create(file.clone())]
        );
        assert_eq!(
            raw_events_from_notify(
                Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                    .add_path(file.clone())
            ),
            vec![RawEvent::Update(file.clone())]
        );
        assert_eq!(
            raw_events_from_notify(
                Event::new(EventKind::Remove(RemoveKind::File)).add_path(file.clone())
            ),
            vec![RawEvent::Delete(file.clone())]
        );

        let from = root.join("old.txt");
        let to = root.join("new.txt");
        assert_eq!(
            raw_events_from_notify(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                    .add_path(from.clone())
                    .add_path(to.clone())
            ),
            vec![RawEvent::Rename { from, to }]
        );

        assert!(raw_events_from_notify(
            Event::new(EventKind::Access(AccessKind::Any)).add_path(file)
        )
        .is_empty());
    }

    #[test]
    fn raw_events_resolve_ambiguous_rename_by_existence() {
        let dir = TempDir::new("rename-any");
        let existing = dir.join("existing.txt");
        fs::write(&existing, "x").expect("write file");
        let missing = dir.join("missing.txt");

        assert_eq!(
            raw_events_from_notify(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
                    .add_path(existing.clone())
            ),
            vec![RawEvent::Create(existing)]
        );
        assert_eq!(
            raw_events_from_notify(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
                    .add_path(missing.clone())
            ),
            vec![RawEvent::Delete(missing)]
        );
    }

    #[test]
    fn flush_emits_single_overflow_payload() {
        let dir = TempDir::new("flush-overflow");
        let watcher = FsWatcher::new();
        let (payloads, callback) = payload_sink();
        watcher.subscribe(callback);
        watcher.watch(dir.str(), "subscriber");

        let mut buffer = BatchBuffer::default();
        buffer.mark_overflow();
        flush_batch(
            dir.path(),
            &[dir.path().to_path_buf()],
            &watcher.inner,
            &mut buffer,
        );

        let payloads = lock(&payloads);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].worktree_path, dir.str());
        assert_eq!(
            payloads[0].events,
            vec![event(FsChangeKind::Overflow, dir.str())]
        );
    }

    #[test]
    fn grace_period_defers_drop_until_clock_advances() {
        let dir = TempDir::new("grace");
        let clock = Arc::new(ManualWatchClock::new());
        let watcher = FsWatcher::with_clock(clock.clone());
        watcher.watch(dir.str(), "a");
        assert_eq!(watcher.watched_root_count(), 1);

        watcher.unwatch(dir.str(), "a");
        {
            let roots = lock(&watcher.inner.roots);
            let root = roots.get(dir.path()).expect("root still installed");
            assert_eq!(
                root.pending_drop_at,
                Some(clock.now() + Duration::from_millis(WATCHER_TEARDOWN_GRACE_MS))
            );
        }

        watcher.inner.sweep_expired_roots();
        assert_eq!(watcher.watched_root_count(), 1);

        clock.advance(Duration::from_millis(WATCHER_TEARDOWN_GRACE_MS - 1));
        watcher.inner.sweep_expired_roots();
        assert_eq!(watcher.watched_root_count(), 1);

        clock.advance(Duration::from_millis(1));
        watcher.inner.sweep_expired_roots();
        assert_eq!(watcher.watched_root_count(), 0);
    }

    #[test]
    fn new_subscriber_cancels_pending_drop() {
        let dir = TempDir::new("grace-cancel");
        let clock = Arc::new(ManualWatchClock::new());
        let watcher = FsWatcher::with_clock(clock.clone());
        watcher.watch(dir.str(), "a");
        watcher.unwatch(dir.str(), "a");

        watcher.watch(dir.str(), "b");
        {
            let roots = lock(&watcher.inner.roots);
            let root = roots.get(dir.path()).expect("root still installed");
            assert_eq!(root.pending_drop_at, None);
            assert!(root.subscribers.contains("b"));
        }

        clock.advance(Duration::from_millis(WATCHER_TEARDOWN_GRACE_MS * 2));
        watcher.inner.sweep_expired_roots();
        assert_eq!(watcher.watched_root_count(), 1);
    }

    #[test]
    fn same_root_shares_one_watcher_and_refcounts_subscribers() {
        let dir = TempDir::new("refcount");
        let watcher = FsWatcher::new();
        watcher.watch(dir.str(), "a");
        watcher.watch(dir.str(), "b");
        assert_eq!(watcher.watched_root_count(), 1);
        {
            let roots = lock(&watcher.inner.roots);
            let root = roots.get(dir.path()).expect("root installed");
            assert_eq!(root.subscribers.len(), 2);
        }

        watcher.unwatch(dir.str(), "a");
        {
            let roots = lock(&watcher.inner.roots);
            let root = roots.get(dir.path()).expect("root installed");
            assert!(root.pending_drop_at.is_none());
        }

        watcher.unwatch(dir.str(), "b");
        {
            let roots = lock(&watcher.inner.roots);
            let root = roots.get(dir.path()).expect("root installed");
            assert!(root.pending_drop_at.is_some());
        }
        assert_eq!(watcher.watched_root_count(), 1);
    }

    #[test]
    fn install_failure_is_negatively_cached_and_emits_overflow() {
        let dir = TempDir::new("failed");
        let missing = dir.join("missing-root");
        let watcher = FsWatcher::new();
        let (payloads, callback) = payload_sink();
        watcher.subscribe(callback);

        watcher.watch(missing.to_str().expect("UTF-8 path"), "a");
        assert_eq!(watcher.watched_root_count(), 0);
        assert!(lock(&watcher.inner.failed_roots).contains(&missing));
        {
            let payloads = lock(&payloads);
            assert_eq!(payloads.len(), 1);
            assert_eq!(payloads[0].worktree_path, missing.to_string_lossy());
            assert_eq!(
                payloads[0].events,
                vec![event(FsChangeKind::Overflow, &missing.to_string_lossy())]
            );
        }

        watcher.watch(missing.to_str().expect("UTF-8 path"), "b");
        assert_eq!(lock(&payloads).len(), 1);
    }
}
