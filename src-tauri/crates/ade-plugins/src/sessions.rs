//! Panel/main-tab/global-view session tokens.
//!
//! Every renderer session (an embedded panel, a main-area tab, or a global
//! view) is bound to a plugin, an install scope, optional panel/tab/instance
//! ids, optional repo/worktree ids, and the manifest revision the renderer
//! materialized the session from. The registry hands out opaque 32-byte
//! base64url tokens and supports invalidating whole classes of sessions when
//! an owner (renderer window), a plugin, or a session kind goes away.
//!
//! `resolve` only answers "is this token still registered, and what was it
//! bound to". Call sites are responsible for re-checking the binding against
//! the current world: the plugin must still be enabled/consented at
//! `manifest_revision`, the scope must still match the active workspace, and
//! window/repo/worktree ids must still exist. A binding that resolves is not
//! by itself sufficient authorization.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use uuid::Uuid;

use crate::types::PluginScope;

/// Number of random bytes behind every session token.
pub const SESSION_TOKEN_BYTES: usize = 32;
/// Default maximum number of live bindings kept by [`SessionRegistry::new`].
pub const DEFAULT_SESSION_CAP: usize = 256;

/// What kind of renderer surface a session backs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionKind {
    Panel,
    MainTab,
    GlobalView,
}

/// Who owns a session; invalidating an owner drops every binding it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOwner {
    Renderer { window_label: String },
    Worker,
}

/// The immutable facts a session token resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBinding {
    pub plugin_key: String,
    pub kind: SessionKind,
    pub panel_id: Option<String>,
    pub tab_id: Option<String>,
    pub instance_id: Option<String>,
    pub scope: PluginScope,
    pub repo_id: Option<String>,
    pub worktree_id: Option<String>,
    pub manifest_revision: String,
    pub owner: SessionOwner,
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64URL[(n >> 18) as usize & 63] as char);
        out.push(B64URL[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64URL[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL[n as usize & 63] as char);
        }
    }
    out
}

fn random_token() -> String {
    let mut bytes = [0u8; SESSION_TOKEN_BYTES];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    encode_base64url(&bytes)
}

struct Entry {
    seq: u64,
    binding: SessionBinding,
}

#[derive(Default)]
struct Inner {
    map: HashMap<String, Entry>,
    next_seq: u64,
}

/// Thread-safe panel/tab session registry. A cap bounds memory: once
/// `issue` pushes the registry past its capacity, the oldest bindings are
/// evicted. Eviction is silent; call sites must treat `resolve -> None` as a
/// dead session regardless of the reason.
pub struct SessionRegistry {
    inner: Mutex<Inner>,
    cap: usize,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionRegistry {
    /// Registry with the default cap of [`DEFAULT_SESSION_CAP`].
    pub fn new() -> Self {
        Self::with_cap(DEFAULT_SESSION_CAP)
    }

    /// Registry capped at `cap` live bindings (at least one).
    pub fn with_cap(cap: usize) -> Self {
        SessionRegistry {
            inner: Mutex::new(Inner::default()),
            cap: cap.max(1),
        }
    }

    /// Registers `binding` and returns its opaque 32-byte base64url token.
    /// Identical bindings receive distinct tokens.
    pub fn issue(&self, binding: SessionBinding) -> String {
        let mut inner = self.lock();
        let mut token = random_token();
        while inner.map.contains_key(&token) {
            token = random_token();
        }
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.map.insert(token.clone(), Entry { seq, binding });
        if inner.map.len() > self.cap {
            let excess = inner.map.len() - self.cap;
            Self::evict_oldest(&mut inner.map, excess);
        }
        token
    }

    /// Returns the binding iff `token` is still registered. Revision/scope/
    /// enablement checks are the caller's responsibility (see module docs).
    pub fn resolve(&self, token: &str) -> Option<SessionBinding> {
        self.lock()
            .map
            .get(token)
            .map(|entry| entry.binding.clone())
    }

    /// Drops every binding owned by `owner`; returns how many were dropped.
    pub fn invalidate_owner(&self, owner: &SessionOwner) -> usize {
        self.retain(|binding| &binding.owner != owner)
    }

    /// Drops every binding for `plugin_key`; returns how many were dropped.
    /// Reserved for full uninstall: scope-limited lifecycles (disable,
    /// project removal, idle reap) must use [`Self::invalidate_plugin_scope`]
    /// so a same-key twin in another scope keeps its sessions.
    pub fn invalidate_plugin(&self, plugin_key: &str) -> usize {
        self.retain(|binding| binding.plugin_key != plugin_key)
    }

    /// Drops every binding of `plugin_key` in exactly the
    /// `(scope, repo_id)` context; returns how many were dropped.
    pub fn invalidate_plugin_scope(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
    ) -> usize {
        self.retain(|binding| {
            !(binding.plugin_key == plugin_key
                && binding.scope == scope
                && binding.repo_id.as_deref() == repo_id)
        })
    }

    /// Drops every binding of `kind`; returns how many were dropped.
    pub fn invalidate_kind(&self, kind: SessionKind) -> usize {
        self.retain(|binding| binding.kind != kind)
    }

    /// Drops the oldest bindings until at most `max_entries` remain; returns
    /// how many were dropped. `issue` applies the same rule automatically
    /// against the registry's configured cap. Expiry is capacity-driven, not
    /// wall-clock age; stale-by-revision bindings are rejected at call sites.
    pub fn sweep(&self, max_entries: usize) -> usize {
        let mut inner = self.lock();
        if inner.map.len() <= max_entries {
            return 0;
        }
        let excess = inner.map.len() - max_entries;
        Self::evict_oldest(&mut inner.map, excess);
        excess
    }

    /// Live binding count.
    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn retain(&self, keep: impl Fn(&SessionBinding) -> bool) -> usize {
        let mut inner = self.lock();
        let before = inner.map.len();
        inner.map.retain(|_, entry| keep(&entry.binding));
        before - inner.map.len()
    }

    fn evict_oldest(map: &mut HashMap<String, Entry>, count: usize) {
        let mut order: Vec<(u64, String)> = map
            .iter()
            .map(|(token, entry)| (entry.seq, token.clone()))
            .collect();
        order.sort_unstable();
        for (_, token) in order.into_iter().take(count) {
            map.remove(&token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(plugin: &str, kind: SessionKind, owner: SessionOwner) -> SessionBinding {
        SessionBinding {
            plugin_key: plugin.to_string(),
            kind,
            panel_id: Some("panel-1".to_string()),
            tab_id: None,
            instance_id: None,
            scope: PluginScope::Global,
            repo_id: None,
            worktree_id: None,
            manifest_revision: "rev-1".to_string(),
            owner,
        }
    }

    fn renderer(label: &str) -> SessionOwner {
        SessionOwner::Renderer {
            window_label: label.to_string(),
        }
    }

    fn charset_ok(token: &str) -> bool {
        token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }

    #[test]
    fn issue_resolve_round_trip() {
        let registry = SessionRegistry::new();
        let expected = binding("acme.hello", SessionKind::Panel, renderer("main"));
        let token = registry.issue(expected.clone());
        assert_eq!(registry.resolve(&token), Some(expected));
    }

    #[test]
    fn token_is_32_bytes_base64url() {
        let registry = SessionRegistry::new();
        let token = registry.issue(binding("acme.hello", SessionKind::Panel, renderer("main")));
        assert_eq!(token.len(), 43);
        assert!(charset_ok(&token), "non-urlsafe char in {token}");
    }

    #[test]
    fn unknown_token_resolves_none() {
        let registry = SessionRegistry::new();
        assert_eq!(registry.resolve("not-a-token"), None);
    }

    #[test]
    fn encoder_matches_rfc4648_urlsafe() {
        assert_eq!(encode_base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode_base64url(b"fo"), "Zm8");
        assert_eq!(encode_base64url(b"f"), "Zg");
        assert_eq!(encode_base64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(encode_base64url(&[0u8; SESSION_TOKEN_BYTES]).len(), 43);
    }

    #[test]
    fn invalidate_owner_only_drops_matching_owner() {
        let registry = SessionRegistry::new();
        let a = registry.issue(binding("acme.hello", SessionKind::Panel, renderer("main")));
        let b = registry.issue(binding(
            "acme.hello",
            SessionKind::MainTab,
            renderer("other"),
        ));
        let c = registry.issue(binding(
            "acme.hello",
            SessionKind::Panel,
            SessionOwner::Worker,
        ));

        assert_eq!(registry.invalidate_owner(&renderer("main")), 1);
        assert_eq!(registry.resolve(&a), None);
        assert!(registry.resolve(&b).is_some());
        assert!(registry.resolve(&c).is_some());
    }

    #[test]
    fn invalidate_plugin_only_drops_matching_plugin() {
        let registry = SessionRegistry::new();
        let a = registry.issue(binding("acme.hello", SessionKind::Panel, renderer("main")));
        let b = registry.issue(binding(
            "acme.hello",
            SessionKind::GlobalView,
            renderer("main"),
        ));
        let c = registry.issue(binding(
            "other.plugin",
            SessionKind::Panel,
            renderer("main"),
        ));

        assert_eq!(registry.invalidate_plugin("acme.hello"), 2);
        assert_eq!(registry.resolve(&a), None);
        assert_eq!(registry.resolve(&b), None);
        assert!(registry.resolve(&c).is_some());
    }

    fn scoped_binding(
        plugin: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        owner: SessionOwner,
    ) -> SessionBinding {
        SessionBinding {
            plugin_key: plugin.to_string(),
            kind: SessionKind::Panel,
            panel_id: Some("panel-1".to_string()),
            tab_id: None,
            instance_id: None,
            scope,
            repo_id: repo_id.map(str::to_string),
            worktree_id: None,
            manifest_revision: "rev-1".to_string(),
            owner,
        }
    }

    #[test]
    fn invalidate_plugin_scope_only_drops_the_matching_context() {
        let registry = SessionRegistry::new();
        let global = registry.issue(scoped_binding(
            "acme.hello",
            PluginScope::Global,
            None,
            renderer("main"),
        ));
        let project_one = registry.issue(scoped_binding(
            "acme.hello",
            PluginScope::Project,
            Some("repo-1"),
            renderer("main"),
        ));
        let project_two = registry.issue(scoped_binding(
            "acme.hello",
            PluginScope::Project,
            Some("repo-2"),
            renderer("main"),
        ));
        let other = registry.issue(scoped_binding(
            "other.plugin",
            PluginScope::Project,
            Some("repo-1"),
            renderer("main"),
        ));

        assert_eq!(
            registry.invalidate_plugin_scope("acme.hello", PluginScope::Project, Some("repo-1")),
            1
        );
        assert!(registry.resolve(&global).is_some(), "global twin survives");
        assert_eq!(registry.resolve(&project_one), None);
        assert!(
            registry.resolve(&project_two).is_some(),
            "other repo survives"
        );
        assert!(registry.resolve(&other).is_some(), "other key survives");
    }

    #[test]
    fn invalidate_plugin_drops_every_scope_for_full_uninstall() {
        let registry = SessionRegistry::new();
        let global = registry.issue(scoped_binding(
            "acme.hello",
            PluginScope::Global,
            None,
            renderer("main"),
        ));
        let project = registry.issue(scoped_binding(
            "acme.hello",
            PluginScope::Project,
            Some("repo-1"),
            renderer("main"),
        ));
        let other = registry.issue(scoped_binding(
            "other.plugin",
            PluginScope::Global,
            None,
            renderer("main"),
        ));

        assert_eq!(registry.invalidate_plugin("acme.hello"), 2);
        assert_eq!(registry.resolve(&global), None);
        assert_eq!(registry.resolve(&project), None);
        assert!(registry.resolve(&other).is_some());
    }

    #[test]
    fn invalidate_kind_only_drops_matching_kind() {
        let registry = SessionRegistry::new();
        let panel = registry.issue(binding("acme.hello", SessionKind::Panel, renderer("main")));
        let tab = registry.issue(binding(
            "acme.hello",
            SessionKind::MainTab,
            renderer("main"),
        ));
        let view = registry.issue(binding(
            "acme.hello",
            SessionKind::GlobalView,
            renderer("main"),
        ));

        assert_eq!(registry.invalidate_kind(SessionKind::MainTab), 1);
        assert!(registry.resolve(&panel).is_some());
        assert_eq!(registry.resolve(&tab), None);
        assert!(registry.resolve(&view).is_some());
    }

    #[test]
    fn sweep_drops_oldest_over_cap() {
        let registry = SessionRegistry::new();
        let tokens: Vec<String> = (0..5)
            .map(|i| {
                registry.issue(binding(
                    &format!("acme.p{i}"),
                    SessionKind::Panel,
                    renderer("main"),
                ))
            })
            .collect();

        assert_eq!(registry.sweep(2), 3);
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.resolve(&tokens[0]), None);
        assert_eq!(registry.resolve(&tokens[1]), None);
        assert_eq!(registry.resolve(&tokens[2]), None);
        assert!(registry.resolve(&tokens[3]).is_some());
        assert!(registry.resolve(&tokens[4]).is_some());
        assert_eq!(registry.sweep(2), 0);
    }

    #[test]
    fn issue_auto_sweeps_past_configured_cap() {
        let registry = SessionRegistry::with_cap(2);
        let first = registry.issue(binding("acme.one", SessionKind::Panel, renderer("main")));
        let second = registry.issue(binding("acme.two", SessionKind::Panel, renderer("main")));
        let third = registry.issue(binding("acme.three", SessionKind::Panel, renderer("main")));

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.resolve(&first), None);
        assert!(registry.resolve(&second).is_some());
        assert!(registry.resolve(&third).is_some());
    }

    #[test]
    fn identical_bindings_get_distinct_tokens() {
        let registry = SessionRegistry::new();
        let binding = binding("acme.hello", SessionKind::Panel, renderer("main"));
        let first = registry.issue(binding.clone());
        let second = registry.issue(binding.clone());

        assert_ne!(first, second);
        assert_eq!(registry.resolve(&first), Some(binding.clone()));
        assert_eq!(registry.resolve(&second), Some(binding));
    }

    #[test]
    fn invalidated_token_cannot_be_resolved_twice() {
        let registry = SessionRegistry::new();
        let token = registry.issue(binding("acme.hello", SessionKind::Panel, renderer("main")));
        registry.invalidate_plugin("acme.hello");
        assert_eq!(registry.resolve(&token), None);
        assert_eq!(registry.invalidate_plugin("acme.hello"), 0);
    }
}
