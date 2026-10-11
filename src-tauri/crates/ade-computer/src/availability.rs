//! Provider availability (spec §3.1): macOS 14+ with a built helper. The
//! unavailable message distinguishes an unsupported platform from a missing
//! helper build (the latter points at `pnpm build:computer-macos`).

use std::path::Path;

use thiserror::Error;

use crate::paths::HELPER_APP_NAME;

/// Why the computer-use provider cannot serve requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderUnavailable {
    #[error("computer-use requires macOS 14 or later")]
    PlatformUnsupported,
    #[error("the computer-use helper app is not built")]
    HelperMissing,
}

/// Human-readable `provider_unavailable` message for the CLI/RPC surfaces.
pub fn provider_unavailable_message(reason: ProviderUnavailable, helper: Option<&Path>) -> String {
    match reason {
        ProviderUnavailable::PlatformUnsupported => {
            "Computer use is only supported on macOS 14 or later.".to_string()
        }
        ProviderUnavailable::HelperMissing => {
            let mut message = format!(
                "The computer-use helper app ({HELPER_APP_NAME}) is not available. Run `pnpm build:computer-macos` to build it"
            );
            if let Some(path) = helper {
                message.push_str(&format!(
                    ", or set ORCINUS_COMPUTER_MACOS_HELPER_APP_PATH (checked: {})",
                    path.display()
                ));
            } else {
                message.push('.');
            }
            message
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_unsupported_message_states_the_requirement() {
        let message = provider_unavailable_message(ProviderUnavailable::PlatformUnsupported, None);
        assert!(message.contains("macOS 14"), "message was: {message}");
    }

    #[test]
    fn helper_missing_message_points_at_the_build_command() {
        let message = provider_unavailable_message(ProviderUnavailable::HelperMissing, None);
        assert!(
            message.contains("pnpm build:computer-macos"),
            "message was: {message}"
        );
    }

    #[test]
    fn helper_missing_message_mentions_checked_helper_path_when_known() {
        let helper = Path::new("/tmp/Orcinus Computer Use.app");
        let message =
            provider_unavailable_message(ProviderUnavailable::HelperMissing, Some(helper));
        assert!(
            message.contains("/tmp/Orcinus Computer Use.app"),
            "message was: {message}"
        );
    }

    #[test]
    fn platform_unsupported_display_is_stable() {
        assert_eq!(
            ProviderUnavailable::PlatformUnsupported.to_string(),
            "computer-use requires macOS 14 or later"
        );
    }
}
