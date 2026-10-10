//! Short-lived list cache for the AI vault scanner. One slot keyed by the
//! normalized scan inputs, mirroring the reference's single cached list: the
//! panel polls the same scope repeatedly, and a delete invalidates the slot.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::scanner::{effective_limit, normalize_scope_paths, ScanRequest};

/// Cache lifetime, mirroring the reference's 60s TTL.
pub const LIST_CACHE_TTL: Duration = Duration::from_secs(60);

pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

struct CacheEntry {
    key: String,
    result: Value,
    expires_at: Instant,
}

struct CacheState {
    /// Bumped on every invalidation. A scan that started before an
    /// invalidation must not write its stale result back into the cache.
    generation: u64,
    entry: Option<CacheEntry>,
}

pub struct ListCache {
    state: Mutex<CacheState>,
    ttl: Duration,
    now: Clock,
}

impl ListCache {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(Instant::now))
    }

    pub fn with_clock(now: Clock) -> Self {
        Self::with_ttl_and_clock(LIST_CACHE_TTL, now)
    }

    pub fn with_ttl_and_clock(ttl: Duration, now: Clock) -> Self {
        Self {
            state: Mutex::new(CacheState {
                generation: 0,
                entry: None,
            }),
            ttl,
            now,
        }
    }

    /// Return the cached result for this request when one is fresh, otherwise
    /// run `scan` and remember its result (except cancelled scans).
    pub fn get_or_scan<F>(&self, request: &ScanRequest, scan: F) -> Value
    where
        F: FnOnce() -> Value,
    {
        let key = cache_key(request);
        let generation = {
            let state = lock(&self.state);
            if !request.force {
                if let Some(entry) = &state.entry {
                    if entry.key == key && entry.expires_at > (self.now)() {
                        return entry.result.clone();
                    }
                }
            }
            state.generation
        };
        let result = scan();
        if !is_cancelled_result(&result) {
            let mut state = lock(&self.state);
            // A delete landing mid-scan must not be undone by this stale write.
            if state.generation == generation {
                state.entry = Some(CacheEntry {
                    key,
                    result: result.clone(),
                    expires_at: (self.now)() + self.ttl,
                });
            }
        }
        result
    }

    /// Drop the cached list after a session delete (or any other mutation).
    pub fn invalidate(&self) {
        let mut state = lock(&self.state);
        state.generation = state.generation.wrapping_add(1);
        state.entry = None;
    }
}

impl Default for ListCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Key = normalized scope paths (order-insensitive) + the effective limit.
fn cache_key(request: &ScanRequest) -> String {
    let mut scopes = normalize_scope_paths(&request.scope_paths);
    scopes.sort();
    scopes.dedup();
    format!(
        "{}\u{1f}{}",
        scopes.join("\u{1e}"),
        effective_limit(request.limit)
    )
}

fn is_cancelled_result(result: &Value) -> bool {
    result
        .get("cancelled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    fn request(scope_paths: &[&str], limit: usize, force: bool) -> ScanRequest {
        ScanRequest {
            scope_paths: scope_paths.iter().map(|scope| scope.to_string()).collect(),
            limit,
            force,
            request_token: None,
        }
    }

    fn fake_clock() -> (Arc<Mutex<Instant>>, Clock) {
        let now = Arc::new(Mutex::new(Instant::now()));
        let handle = now.clone();
        (
            now,
            Arc::new(move || {
                *handle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
            }),
        )
    }

    fn advance(now: &Mutex<Instant>, seconds: u64) {
        let mut now = now.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *now += Duration::from_secs(seconds);
    }

    #[test]
    fn serves_the_cached_result_within_the_ttl() {
        let (now, clock) = fake_clock();
        let cache = ListCache::with_clock(clock);
        let scans = Cell::new(0);
        let scan = || {
            scans.set(scans.get() + 1);
            json!({ "sessions": [], "issues": [], "scannedAt": "t" })
        };
        let request = request(&[], 100, false);

        let first = cache.get_or_scan(&request, scan);
        let second = cache.get_or_scan(&request, scan);
        assert_eq!(scans.get(), 1);
        assert_eq!(first, second);

        advance(&now, 59);
        cache.get_or_scan(&request, scan);
        assert_eq!(scans.get(), 1);

        advance(&now, 2);
        cache.get_or_scan(&request, scan);
        assert_eq!(scans.get(), 2);
    }

    #[test]
    fn force_bypasses_the_cache_and_refreshes_it() {
        let (_, clock) = fake_clock();
        let cache = ListCache::with_clock(clock);
        let scans = Cell::new(0);
        let scan = || {
            scans.set(scans.get() + 1);
            json!({ "sessions": [], "issues": [], "scannedAt": "t" })
        };

        cache.get_or_scan(&request(&[], 100, false), scan);
        assert_eq!(scans.get(), 1);

        cache.get_or_scan(&request(&[], 100, true), scan);
        assert_eq!(scans.get(), 2);

        cache.get_or_scan(&request(&[], 100, false), scan);
        assert_eq!(scans.get(), 2);
    }

    #[test]
    fn cache_key_ignores_scope_order_but_not_scope_or_limit() {
        let (_, clock) = fake_clock();
        let cache = ListCache::with_clock(clock);
        let scans = Cell::new(0);
        let scan = || {
            scans.set(scans.get() + 1);
            json!({ "sessions": [], "issues": [], "scannedAt": "t" })
        };

        cache.get_or_scan(&request(&["/b", "/a"], 50, false), scan);
        cache.get_or_scan(&request(&["/a", "/b"], 50, false), scan);
        assert_eq!(scans.get(), 1);

        cache.get_or_scan(&request(&["/a"], 50, false), scan);
        assert_eq!(scans.get(), 2);

        cache.get_or_scan(&request(&["/a", "/b"], 60, false), scan);
        assert_eq!(scans.get(), 3);
    }

    #[test]
    fn invalidate_forces_the_next_call_to_rescan() {
        let (_, clock) = fake_clock();
        let cache = ListCache::with_clock(clock);
        let scans = Cell::new(0);
        let scan = || {
            scans.set(scans.get() + 1);
            json!({ "sessions": [], "issues": [], "scannedAt": "t" })
        };
        let request = request(&[], 100, false);

        cache.get_or_scan(&request, scan);
        assert_eq!(scans.get(), 1);
        cache.invalidate();
        cache.get_or_scan(&request, scan);
        assert_eq!(scans.get(), 2);
    }

    #[test]
    fn cancelled_results_are_not_cached() {
        let (_, clock) = fake_clock();
        let cache = ListCache::with_clock(clock);
        let scans = Cell::new(0);
        let cancelled = || {
            scans.set(scans.get() + 1);
            json!({ "sessions": [], "issues": [], "scannedAt": "t", "cancelled": true })
        };
        let scan = || {
            scans.set(scans.get() + 1);
            json!({ "sessions": [], "issues": [], "scannedAt": "t2" })
        };
        let request = request(&[], 100, false);

        let result = cache.get_or_scan(&request, cancelled);
        assert_eq!(result["cancelled"], json!(true));
        let result = cache.get_or_scan(&request, scan);
        assert_eq!(result["scannedAt"], json!("t2"));
        assert_eq!(scans.get(), 2);
    }

    #[test]
    fn an_invalidation_during_a_scan_blocks_the_stale_cache_write() {
        let (_, clock) = fake_clock();
        let cache = ListCache::with_clock(clock);
        let scans = Cell::new(0);
        let scan = || {
            scans.set(scans.get() + 1);
            cache.invalidate();
            json!({ "sessions": [], "issues": [], "scannedAt": "t" })
        };
        let request = request(&[], 100, false);

        cache.get_or_scan(&request, scan);
        cache.get_or_scan(&request, scan);
        assert_eq!(scans.get(), 2);
    }
}
