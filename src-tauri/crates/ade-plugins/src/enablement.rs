//! The single write path for consent approval and enablement.
//!
//! Semantics ported from `orca:src/main/plugins/plugin-enablement.ts`:
//!
//! - Approving records the *current* fingerprint under the scoped key and
//!   clears any disable. A same-key install can change while the consent
//!   dialog is open, so approval first requires
//!   `reviewed_fingerprint == current_fingerprint` and otherwise refuses with
//!   `consent_stale` — a decision is never applied to capabilities or a worker
//!   trust tier the user did not review.
//! - Declining disables the plugin so it never re-prompts on later launches.
//! - Enabling never bypasses consent: it only clears the disable bit. With a
//!   missing or stale consent record the plugin remains `pending` and the
//!   caller must run the consent flow instead.
//!
//! The fork records both decisions per scope (`global:<key>` /
//! `project:<repoId>:<key>`, spec §3.3), so approving a project install never
//! approves the global one.

use crate::consent::ConsentState;
use crate::types::{PluginError, CONSENT_STALE};

/// Records the user's consent-dialog answer for one scoped plugin.
pub fn approve(
    state: &dyn ConsentState,
    scoped_key: &str,
    reviewed_fingerprint: &str,
    current_fingerprint: &str,
) -> Result<(), PluginError> {
    if reviewed_fingerprint != current_fingerprint {
        return Err(stale_consent(scoped_key));
    }
    state.set_consent(scoped_key, current_fingerprint);
    state.set_disabled(scoped_key, false);
    Ok(())
}

/// Records a declined consent dialog: the plugin is disabled and no consent
/// record is written, so it stays disabled (not pending) on later launches
/// until the user explicitly approves it.
pub fn decline(state: &dyn ConsentState, scoped_key: &str) {
    state.set_disabled(scoped_key, true);
}

/// Enables or disables an already-consented plugin. Enabling only clears the
/// disable bit; it never writes a consent record, so a plugin without a
/// current approval stays pending and must run the consent flow.
pub fn set_enabled(state: &dyn ConsentState, scoped_key: &str, enabled: bool) {
    state.set_disabled(scoped_key, !enabled);
}

fn stale_consent(scoped_key: &str) -> PluginError {
    PluginError::new(
        CONSENT_STALE,
        format!("plugin {scoped_key} changed since its permissions were reviewed"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::test_support::FakeConsentState;
    use crate::consent::{activation_state, needs_reconsent, ActivationState};

    const KEY: &str = "global:orca-samples.demo";

    #[test]
    fn approve_records_the_current_fingerprint_and_clears_disable() {
        let state = FakeConsentState::default();
        state.set_disabled(KEY, true);

        approve(&state, KEY, "sha256-current", "sha256-current").unwrap();

        assert_eq!(state.consent_for(KEY).as_deref(), Some("sha256-current"));
        assert!(!state.is_disabled(KEY));
        assert_eq!(
            activation_state(&state, KEY, "sha256-current"),
            ActivationState::Approved
        );
    }

    #[test]
    fn approve_refuses_a_fingerprint_race() {
        let state = FakeConsentState::default();

        let error = approve(&state, KEY, "sha256-reviewed", "sha256-current").unwrap_err();

        assert_eq!(error.code(), CONSENT_STALE);
        assert_eq!(
            error.message(),
            "plugin global:orca-samples.demo changed since its permissions were reviewed"
        );
        assert_eq!(state.consent_for(KEY), None);
        assert_eq!(
            activation_state(&state, KEY, "sha256-current"),
            ActivationState::Pending
        );
    }

    #[test]
    fn declining_disables_without_recording_consent() {
        let state = FakeConsentState::default();

        decline(&state, KEY);

        assert!(state.is_disabled(KEY));
        assert_eq!(state.consent_for(KEY), None);
        assert!(!needs_reconsent(&state, KEY, "sha256-current"));
        assert_eq!(
            activation_state(&state, KEY, "sha256-current"),
            ActivationState::Disabled
        );
    }

    #[test]
    fn enable_never_bypasses_consent() {
        let state = FakeConsentState::default();
        set_enabled(&state, KEY, true);

        assert!(!state.is_disabled(KEY));
        assert_eq!(state.consent_for(KEY), None);
        assert_eq!(
            activation_state(&state, KEY, "sha256-current"),
            ActivationState::Pending
        );
    }

    #[test]
    fn enable_stays_pending_when_the_fingerprint_drifted() {
        let state = FakeConsentState::default();
        state.set_consent(KEY, "sha256-old");

        set_enabled(&state, KEY, true);

        assert_eq!(state.consent_for(KEY).as_deref(), Some("sha256-old"));
        assert_eq!(
            activation_state(&state, KEY, "sha256-new"),
            ActivationState::Pending
        );
        assert!(needs_reconsent(&state, KEY, "sha256-new"));
    }

    #[test]
    fn disable_and_reenable_round_trip_for_an_approved_plugin() {
        let state = FakeConsentState::default();
        approve(&state, KEY, "sha256-current", "sha256-current").unwrap();

        set_enabled(&state, KEY, false);
        assert_eq!(
            activation_state(&state, KEY, "sha256-current"),
            ActivationState::Disabled
        );

        set_enabled(&state, KEY, true);
        assert_eq!(
            activation_state(&state, KEY, "sha256-current"),
            ActivationState::Approved
        );
    }

    #[test]
    fn project_approval_does_not_approve_global() {
        let state = FakeConsentState::default();
        approve(
            &state,
            "project:repo-1:orca-samples.demo",
            "sha256-current",
            "sha256-current",
        )
        .unwrap();

        assert_eq!(
            activation_state(&state, "project:repo-1:orca-samples.demo", "sha256-current"),
            ActivationState::Approved
        );
        assert_eq!(
            activation_state(&state, "global:orca-samples.demo", "sha256-current"),
            ActivationState::Pending
        );
    }

    #[test]
    fn approve_after_review_race_recovers_with_the_fresh_fingerprint() {
        let state = FakeConsentState::default();
        let error = approve(&state, KEY, "sha256-reviewed", "sha256-current").unwrap_err();
        assert_eq!(error.code(), CONSENT_STALE);

        approve(&state, KEY, "sha256-current", "sha256-current").unwrap();
        assert_eq!(
            activation_state(&state, KEY, "sha256-current"),
            ActivationState::Approved
        );
    }
}
