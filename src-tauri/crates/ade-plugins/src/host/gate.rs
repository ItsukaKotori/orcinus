//! The capability gate — pure, deny-by-default.
//!
//! Ported from `orca:src/shared/plugins/plugin-capability-gate.ts` with the
//! same decision order and error strings: unknown method, panel surface,
//! consent (a `None` grant set means unknown/disabled/stale consent and must
//! be indistinguishable from an ungranted capability for plugin code), then
//! capability membership.

use crate::host::api;
use crate::types::Capability;

pub const UNKNOWN_METHOD: &str = "unknown_method";
pub const CAPABILITY_DENIED: &str = "capability_denied";
pub const CONSENT_REQUIRED: &str = "consent_required";
pub const PANEL_FORBIDDEN: &str = "panel_forbidden";
pub const RATE_LIMITED: &str = "rate_limited";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Granted,
    Denied { code: &'static str, error: String },
}

pub fn decide(granted: Option<&[Capability]>, via_panel: bool, method: &str) -> GateDecision {
    let Some(spec) = api::spec(method) else {
        return GateDecision::Denied {
            code: UNKNOWN_METHOD,
            error: format!("unknown host method: {method}"),
        };
    };
    if via_panel && !spec.panel_callable {
        return GateDecision::Denied {
            code: PANEL_FORBIDDEN,
            error: format!("method {method} is not available to sandboxed panels"),
        };
    }
    // Why: a disabled/unknown/stale-consent plugin must fail exactly like an
    // ungranted capability — no probe-able distinction for plugin code.
    let Some(granted) = granted else {
        return GateDecision::Denied {
            code: CONSENT_REQUIRED,
            error: "plugin is not enabled with current consent".to_string(),
        };
    };
    if !granted.contains(&spec.capability) {
        return GateDecision::Denied {
            code: CAPABILITY_DENIED,
            error: format!(
                "plugin does not have the \"{}\" capability",
                spec.capability.as_str()
            ),
        };
    }
    GateDecision::Granted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method_for(capability: Capability) -> &'static str {
        match capability {
            Capability::EventsSubscribe => "events.subscribe",
            Capability::NotificationsShow => "notifications.show",
            Capability::Secrets => "secrets.get",
            Capability::SettingsOwn => "settings.get",
            Capability::Storage => "storage.get",
            Capability::TerminalSend => "terminal.sendText",
            Capability::WorkspaceOpenTab => "workspace.openTab",
            Capability::WorkspaceRead => "workspace.readContext",
        }
    }

    fn denied_code(decision: GateDecision) -> Option<&'static str> {
        match decision {
            GateDecision::Denied { code, .. } => Some(code),
            GateDecision::Granted => None,
        }
    }

    #[test]
    fn matrix_grants_matching_capability_and_denies_the_rest() {
        for capability in Capability::ALL {
            let method = method_for(capability);
            let granted = [capability];
            assert_eq!(
                decide(Some(&granted), false, method),
                GateDecision::Granted,
                "worker with {capability:?} must call {method}"
            );
            for other in Capability::ALL {
                if other == capability {
                    continue;
                }
                let other_grant = [other];
                assert_eq!(
                    denied_code(decide(Some(&other_grant), false, method)),
                    Some(CAPABILITY_DENIED),
                    "{other:?} must not grant {method}"
                );
            }
        }
    }

    #[test]
    fn panel_surface_is_exactly_the_four_panel_callable_methods() {
        let panel_methods = [
            "workspace.readContext",
            "workspace.openTab",
            "terminal.sendText",
            "notifications.show",
        ];
        for spec in api::specs() {
            for via_panel in [false, true] {
                let granted = [spec.capability];
                let decision = decide(Some(&granted), via_panel, spec.name);
                let expected_panel = panel_methods.contains(&spec.name);
                if via_panel && !expected_panel {
                    assert_eq!(
                        denied_code(decision),
                        Some(PANEL_FORBIDDEN),
                        "{} via panel",
                        spec.name
                    );
                } else {
                    assert_eq!(decision, GateDecision::Granted, "{}", spec.name);
                }
            }
        }
    }

    #[test]
    fn unknown_method_is_rejected_first_regardless_of_surface() {
        for via_panel in [false, true] {
            for granted in [None, Some(&Capability::ALL[..])] {
                match decide(granted, via_panel, "workspace.explode") {
                    GateDecision::Denied { code, error } => {
                        assert_eq!(code, UNKNOWN_METHOD);
                        assert_eq!(error, "unknown host method: workspace.explode");
                    }
                    GateDecision::Granted => panic!("unknown method must be denied"),
                }
            }
        }
    }

    #[test]
    fn missing_grant_is_consent_required_not_capability_denied() {
        for spec in api::specs() {
            let via_panel = spec.panel_callable;
            match decide(None, via_panel, spec.name) {
                GateDecision::Denied { code, error } => {
                    assert_eq!(code, CONSENT_REQUIRED, "{}", spec.name);
                    assert_eq!(error, "plugin is not enabled with current consent");
                }
                GateDecision::Granted => panic!("{} must require consent", spec.name),
            }
        }
    }

    #[test]
    fn empty_grant_set_grants_nothing_and_panel_check_precedes_consent() {
        for spec in api::specs() {
            assert_eq!(
                denied_code(decide(Some(&[]), false, spec.name)),
                Some(CAPABILITY_DENIED),
                "{}",
                spec.name
            );
        }
        assert_eq!(
            denied_code(decide(None, true, "storage.get")),
            Some(PANEL_FORBIDDEN)
        );
    }

    #[test]
    fn capability_denied_error_names_the_capability() {
        match decide(Some(&[]), false, "workspace.openTab") {
            GateDecision::Denied { code, error } => {
                assert_eq!(code, CAPABILITY_DENIED);
                assert_eq!(
                    error,
                    "plugin does not have the \"workspace:open-tab\" capability"
                );
            }
            GateDecision::Granted => panic!("empty grant must be denied"),
        }
    }
}
