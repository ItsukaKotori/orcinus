//! Consent fingerprint, scoped consent keys, and activation-state derivation.
//!
//! The fingerprint algorithm is a verbatim port of
//! `orca:src/shared/plugins/plugin-consent-fingerprint.ts` +
//! `plugin-capabilities.ts` + `plugin-consent-state.ts`: sha256 (base64) of the
//! canonical capability set, plus `\0trusted-node-worker` when the plugin ships
//! a `main`, plus `\0instructional-content:<contentKey>` when it contributes
//! keybindings, vmRecipes, or agents. Tabs and panels are sandboxed UI
//! contributions and deliberately do not enter the instructional identity
//! (spec §3.1). Activation state is `disabled > pending(mismatch) > approved`.
//!
//! Fork specifics (spec §3.3): consent and enablement are recorded per scope
//! under `global:<key>` / `project:<repoId>:<key>`; both scopes are independent
//! and the state store is injected through the [`ConsentState`] trait because
//! the settings files are main-owned (ade-bridge).

use sha2::{Digest, Sha256};

use crate::manifest::Manifest;
use crate::types::{canonical_capability_set, PluginScope};

/// Consent records are addressed per scope so global and project installs of
/// the same key are approved (and revoked) independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentKeyScope<'a> {
    Global,
    Project { repo_id: &'a str },
}

/// `global:<key>` or `project:<repoId>:<key>`.
///
/// Returns `None` for a project scope without a repo id and for an empty key;
/// callers surface that as an invalid request rather than writing a shared
/// bucket.
pub fn scoped_key(key: &str, scope: ConsentKeyScope<'_>) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    match scope {
        ConsentKeyScope::Global => Some(format!("global:{key}")),
        ConsentKeyScope::Project { repo_id } => {
            if repo_id.is_empty() {
                return None;
            }
            Some(format!("project:{repo_id}:{key}"))
        }
    }
}

/// Parses `global:<key>` / `project:<repoId>:<key>` back into its parts. Only
/// used for round-trip bookkeeping (e.g. migrating settings records); malformed
/// records return `None`.
pub fn parse_scoped_key(value: &str) -> Option<(PluginScope, Option<String>, String)> {
    if let Some(key) = value.strip_prefix("global:") {
        return (!key.is_empty()).then(|| (PluginScope::Global, None, key.to_string()));
    }
    let rest = value.strip_prefix("project:")?;
    let (repo_id, key) = rest.split_once(':')?;
    if repo_id.is_empty() || key.is_empty() {
        return None;
    }
    Some((
        PluginScope::Project,
        Some(repo_id.to_string()),
        key.to_string(),
    ))
}

/// True when the plugin carries executable instructional contributions whose
/// bytes must be covered by consent. Tabs and panels are sandboxed UI entries
/// and intentionally excluded.
pub fn has_instructional_contributions(manifest: &Manifest) -> bool {
    !manifest.contributes.keybindings.is_empty()
        || !manifest.contributes.vm_recipes.is_empty()
        || !manifest.contributes.agents.is_empty()
}

/// Canonical consent identity of a manifest.
///
/// `content_key` is the immutable content identity of the installed/dev tree
/// (the content hash); it enters the fingerprint only when instructional
/// contributions exist, mirroring the oracle. When the caller cannot resolve
/// the identity yet, the oracle substitutes the literal `unresolved` so an
/// approval can never silently bind to bytes nobody hashed.
pub fn fingerprint(manifest: &Manifest, content_key: &str) -> String {
    let capabilities = canonical_capability_set(&manifest.capabilities);
    let worker_identity = if manifest.main.is_none() {
        ""
    } else {
        "\0trusted-node-worker"
    };
    let instructional_identity = if has_instructional_contributions(manifest) {
        format!("\0instructional-content:{content_key}")
    } else {
        String::new()
    };
    let canonical = format!("{capabilities}{worker_identity}{instructional_identity}");
    format!("sha256-{}", base64(&Sha256::digest(canonical.as_bytes())))
}

/// Derived activation state. `disabled > pending > approved` — a disabled
/// plugin never reports pending even when its fingerprint drifted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationState {
    Disabled,
    Pending,
    Approved,
}

/// Main-owned consent/enablement records, injected by ade-bridge. Keys are
/// [`scoped_key`] values; fingerprints are [`fingerprint`] values.
///
/// Mutators take `&self` and implementations own interior mutability (a mutex
/// or the settings store's own locking): the service holds the state behind an
/// `Arc<dyn ConsentState>` and must be able to record decisions through shared
/// references. Reads return owned fingerprints for the same reason.
pub trait ConsentState: Send + Sync {
    fn consent_for(&self, scoped_key: &str) -> Option<String>;
    fn is_disabled(&self, scoped_key: &str) -> bool;
    fn set_consent(&self, scoped_key: &str, fingerprint: &str);
    fn set_disabled(&self, scoped_key: &str, disabled: bool);

    /// Derives the activation state for one plugin at one scope.
    fn activation(
        &self,
        key: &str,
        scope: ConsentKeyScope<'_>,
        current_fingerprint: &str,
    ) -> Option<ActivationState> {
        let scoped_key = scoped_key(key, scope)?;
        if self.is_disabled(&scoped_key) {
            return Some(ActivationState::Disabled);
        }
        match self.consent_for(&scoped_key) {
            Some(recorded) if recorded == current_fingerprint => Some(ActivationState::Approved),
            _ => Some(ActivationState::Pending),
        }
    }

    /// True when the plugin was consented before but its fingerprint changed —
    /// the UI shows "needs re-approval" instead of a first-install prompt.
    fn needs_reconsent(
        &self,
        key: &str,
        scope: ConsentKeyScope<'_>,
        current_fingerprint: &str,
    ) -> bool {
        let Some(scoped_key) = scoped_key(key, scope) else {
            return false;
        };
        self.consent_for(&scoped_key)
            .is_some_and(|recorded| recorded != current_fingerprint)
    }
}

pub fn activation_state(
    state: &(impl ConsentState + ?Sized),
    scoped_key: &str,
    current_fingerprint: &str,
) -> ActivationState {
    if state.is_disabled(scoped_key) {
        return ActivationState::Disabled;
    }
    match state.consent_for(scoped_key) {
        Some(recorded) if recorded == current_fingerprint => ActivationState::Approved,
        _ => ActivationState::Pending,
    }
}

/// True when a consent record exists but no longer matches the current
/// fingerprint. A first install (no record) is not a re-consent.
pub fn needs_reconsent(
    state: &(impl ConsentState + ?Sized),
    scoped_key: &str,
    current_fingerprint: &str,
) -> bool {
    state
        .consent_for(scoped_key)
        .is_some_and(|recorded| recorded != current_fingerprint)
}

/// Standard base64 (no line breaks), matching Node's `Buffer.toString('base64')`.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(third & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory [`ConsentState`] for tests; production records live in
    /// main-owned settings and are injected by ade-bridge.
    #[derive(Debug, Default)]
    pub struct FakeConsentState {
        consents: Mutex<HashMap<String, String>>,
        disabled: Mutex<Vec<String>>,
    }

    impl FakeConsentState {
        pub fn with_consent(scoped_key: &str, fingerprint: &str) -> Self {
            let state = Self::default();
            state.set_consent(scoped_key, fingerprint);
            state
        }
    }

    impl ConsentState for FakeConsentState {
        fn consent_for(&self, scoped_key: &str) -> Option<String> {
            self.consents
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(scoped_key)
                .cloned()
        }

        fn is_disabled(&self, scoped_key: &str) -> bool {
            self.disabled
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|key| key == scoped_key)
        }

        fn set_consent(&self, scoped_key: &str, fingerprint: &str) {
            self.consents
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(scoped_key.to_string(), fingerprint.to_string());
        }

        fn set_disabled(&self, scoped_key: &str, disabled: bool) {
            let mut keys = self
                .disabled
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            keys.retain(|key| key != scoped_key);
            if disabled {
                keys.push(scoped_key.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::FakeConsentState;
    use super::*;
    use crate::manifest::{parse_manifest, Contributes, KeybindingContrib, VmRecipeContrib};
    use crate::types::Capability;
    use serde_json::json;

    fn manifest_json(capabilities: serde_json::Value, main: Option<&str>) -> Vec<u8> {
        let mut value = json!({
            "manifestVersion": 1,
            "id": "demo",
            "publisher": "orca-samples",
            "name": "Demo",
            "version": "1.0.0",
            "engines": { "orca": ">=1.0.0" },
            "pluginApi": 1,
            "capabilities": capabilities
        });
        if let Some(main) = main {
            value["main"] = json!(main);
        }
        serde_json::to_vec(&value).unwrap()
    }

    fn parse(capabilities: serde_json::Value, main: Option<&str>) -> Manifest {
        parse_manifest(&manifest_json(capabilities, main)).unwrap()
    }

    fn legacy_fingerprint(capabilities: &[Capability]) -> String {
        format!(
            "sha256-{}",
            base64(&Sha256::digest(
                canonical_capability_set(capabilities).as_bytes()
            ))
        )
    }

    #[test]
    fn scoped_keys_follow_global_and_project_layout() {
        assert_eq!(
            scoped_key("orca-samples.demo", ConsentKeyScope::Global).as_deref(),
            Some("global:orca-samples.demo")
        );
        assert_eq!(
            scoped_key(
                "orca-samples.demo",
                ConsentKeyScope::Project { repo_id: "repo-1" }
            )
            .as_deref(),
            Some("project:repo-1:orca-samples.demo")
        );
        assert_eq!(scoped_key("", ConsentKeyScope::Global), None);
        assert_eq!(
            scoped_key(
                "orca-samples.demo",
                ConsentKeyScope::Project { repo_id: "" }
            ),
            None
        );
    }

    #[test]
    fn scoped_keys_round_trip() {
        assert_eq!(
            parse_scoped_key("global:orca-samples.demo"),
            Some((PluginScope::Global, None, "orca-samples.demo".to_string()))
        );
        assert_eq!(
            parse_scoped_key("project:repo-1:orca-samples.demo"),
            Some((
                PluginScope::Project,
                Some("repo-1".to_string()),
                "orca-samples.demo".to_string()
            ))
        );
        assert_eq!(parse_scoped_key("orca-samples.demo"), None);
        assert_eq!(parse_scoped_key("global:"), None);
        assert_eq!(parse_scoped_key("project::demo"), None);
        assert_eq!(parse_scoped_key("project:repo-1:"), None);
    }

    #[test]
    fn is_stable_across_capability_order_and_duplicate_declarations() {
        let first = parse(
            json!([{ "kind": "workspace:read" }, { "kind": "storage" }, { "kind": "workspace:read" }]),
            None,
        );
        let second = parse(
            json!([{ "kind": "storage" }, { "kind": "workspace:read" }]),
            None,
        );

        assert_eq!(
            fingerprint(&first, "unresolved"),
            fingerprint(&second, "unresolved")
        );
    }

    #[test]
    fn preserves_capability_only_fingerprints_for_existing_panel_plugins() {
        let parsed = parse(
            json!([{ "kind": "workspace:read" }, { "kind": "storage" }]),
            None,
        );
        assert_eq!(
            fingerprint(&parsed, "unresolved"),
            legacy_fingerprint(&[Capability::WorkspaceRead, Capability::Storage])
        );
        assert_eq!(
            fingerprint(&parsed, "a".repeat(64).as_str()),
            legacy_fingerprint(&[Capability::WorkspaceRead, Capability::Storage])
        );
        // Golden vector: sha256("[]") base64. Empty capability list + no main
        // + no instructional contributions.
        assert_eq!(
            fingerprint(&parse(json!([]), None), "unresolved"),
            "sha256-T1PNoYwrqgwDVLtfmj7L5e0Sq02OEbqHPC8RFhICuUU="
        );
    }

    #[test]
    fn changes_when_a_panel_only_plugin_gains_a_trusted_node_worker() {
        let panel_only = parse(json!([]), None);
        let with_worker = parse(json!([]), Some("worker.js"));

        let panel_fingerprint = fingerprint(&panel_only, "unresolved");
        let worker_fingerprint = fingerprint(&with_worker, "unresolved");
        assert_ne!(worker_fingerprint, panel_fingerprint);

        let state = FakeConsentState::with_consent("global:orca-samples.demo", &panel_fingerprint);
        assert_eq!(
            activation_state(&state, "global:orca-samples.demo", &worker_fingerprint),
            ActivationState::Pending
        );
        assert!(needs_reconsent(
            &state,
            "global:orca-samples.demo",
            &worker_fingerprint
        ));
        state.set_consent("global:orca-samples.demo", &worker_fingerprint);
        assert_eq!(
            activation_state(&state, "global:orca-samples.demo", &worker_fingerprint),
            ActivationState::Approved
        );
    }

    #[test]
    fn requires_reconsent_when_instructional_content_bytes_change() {
        let mut manifest = parse(json!([]), None);
        manifest.contributes = Contributes {
            vm_recipes: vec![VmRecipeContrib {
                path: "recipes/cloud.json".to_string(),
            }],
            ..Contributes::default()
        };
        let first = fingerprint(&manifest, &"a".repeat(64));
        let second = fingerprint(&manifest, &"b".repeat(64));

        assert_ne!(second, first);
        let state = FakeConsentState::with_consent("global:orca-samples.demo", &first);
        assert_eq!(
            activation_state(&state, "global:orca-samples.demo", &second),
            ActivationState::Pending
        );
    }

    #[test]
    fn does_not_invalidate_inert_content_consent_when_only_its_content_hash_changes() {
        let manifest = parse(json!([]), None);
        assert_eq!(
            fingerprint(&manifest, &"a".repeat(64)),
            fingerprint(&manifest, &"b".repeat(64))
        );

        let mut keybindings = parse(json!([]), None);
        keybindings.contributes = Contributes {
            keybindings: vec![KeybindingContrib {
                command: "demo.run".to_string(),
                key: "Cmd+Shift+R".to_string(),
                when: None,
            }],
            ..Contributes::default()
        };
        assert_ne!(
            fingerprint(&keybindings, &"a".repeat(64)),
            fingerprint(&keybindings, &"b".repeat(64))
        );
    }

    #[test]
    fn tabs_and_panels_are_not_instructional_but_agents_and_keybindings_are() {
        let mut manifest = parse(json!([]), None);
        assert!(!has_instructional_contributions(&manifest));

        manifest.contributes = Contributes {
            panels: vec![crate::manifest::PanelContrib {
                id: "panel".to_string(),
                title: "Panel".to_string(),
                icon: None,
                entry: "panel.html".to_string(),
            }],
            tabs: vec![crate::manifest::TabContrib {
                id: "tab".to_string(),
                title: "Tab".to_string(),
                icon: None,
                entry: "tab.html".to_string(),
            }],
            ..Contributes::default()
        };
        assert!(!has_instructional_contributions(&manifest));
        assert_eq!(
            fingerprint(&manifest, &"a".repeat(64)),
            fingerprint(&manifest, &"b".repeat(64))
        );

        manifest.contributes.agents = vec![crate::manifest::AgentContrib {
            path: "agents/reviewer.json".to_string(),
        }];
        assert!(has_instructional_contributions(&manifest));
        assert_ne!(
            fingerprint(&manifest, &"a".repeat(64)),
            fingerprint(&manifest, &"b".repeat(64))
        );
    }

    #[test]
    fn activation_state_three_states_and_needs_reconsent() {
        let state = FakeConsentState::with_consent("global:orca-samples.demo", &"a".repeat(64));
        assert_eq!(
            state.activation(
                "orca-samples.demo",
                ConsentKeyScope::Global,
                &"a".repeat(64)
            ),
            Some(ActivationState::Approved)
        );
        assert!(!state.needs_reconsent(
            "orca-samples.demo",
            ConsentKeyScope::Global,
            &"a".repeat(64)
        ));

        assert_eq!(
            state.activation(
                "orca-samples.demo",
                ConsentKeyScope::Global,
                &"b".repeat(64)
            ),
            Some(ActivationState::Pending)
        );
        assert!(state.needs_reconsent(
            "orca-samples.demo",
            ConsentKeyScope::Global,
            &"b".repeat(64)
        ));

        state.set_disabled("global:orca-samples.demo", true);
        assert_eq!(
            state.activation(
                "orca-samples.demo",
                ConsentKeyScope::Global,
                &"b".repeat(64)
            ),
            Some(ActivationState::Disabled)
        );

        let fresh = FakeConsentState::default();
        assert_eq!(
            fresh.activation(
                "orca-samples.demo",
                ConsentKeyScope::Global,
                &"a".repeat(64)
            ),
            Some(ActivationState::Pending)
        );
        assert!(!fresh.needs_reconsent(
            "orca-samples.demo",
            ConsentKeyScope::Global,
            &"a".repeat(64)
        ));
    }

    #[test]
    fn scopes_are_consented_independently() {
        let fingerprint = "sha256-one";
        let state = FakeConsentState::with_consent("global:orca-samples.demo", fingerprint);
        assert_eq!(
            state.activation("orca-samples.demo", ConsentKeyScope::Global, fingerprint),
            Some(ActivationState::Approved)
        );
        assert_eq!(
            state.activation(
                "orca-samples.demo",
                ConsentKeyScope::Project { repo_id: "repo-1" },
                fingerprint
            ),
            Some(ActivationState::Pending)
        );

        state.set_consent("project:repo-1:orca-samples.demo", fingerprint);
        state.set_consent("project:repo-2:orca-samples.demo", "sha256-two");
        assert_eq!(
            state.activation(
                "orca-samples.demo",
                ConsentKeyScope::Project { repo_id: "repo-1" },
                fingerprint
            ),
            Some(ActivationState::Approved)
        );
        assert_eq!(
            state.activation(
                "orca-samples.demo",
                ConsentKeyScope::Project { repo_id: "repo-2" },
                fingerprint
            ),
            Some(ActivationState::Pending)
        );
    }

    #[test]
    fn approval_fingerprint_is_base64_of_sha256_of_the_canonical_identity() {
        let mut manifest = parse(json!([{ "kind": "storage" }]), Some("worker.js"));
        let expected = {
            let capabilities = canonical_capability_set(&manifest.capabilities);
            let canonical = format!("{capabilities}\0trusted-node-worker");
            format!("sha256-{}", base64(&Sha256::digest(canonical.as_bytes())))
        };
        assert_eq!(fingerprint(&manifest, "unresolved"), expected);

        manifest.contributes.agents = vec![crate::manifest::AgentContrib {
            path: "agents/reviewer.json".to_string(),
        }];
        let content_key = "a".repeat(64);
        let canonical = format!(
            "{}\0trusted-node-worker\0instructional-content:{content_key}",
            canonical_capability_set(&manifest.capabilities)
        );
        assert_eq!(
            fingerprint(&manifest, &content_key),
            format!("sha256-{}", base64(&Sha256::digest(canonical.as_bytes())))
        );
    }
}
