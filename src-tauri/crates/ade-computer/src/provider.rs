//! Provider facade (spec §3.2): the `ComputerProvider` trait consumed by the
//! runtime RPC server, its `ProviderClient` implementation, and the
//! `UnavailableProvider` used when the helper is missing.

use std::path::Path;

use serde_json::Value;

use crate::availability::{provider_unavailable_message, ProviderUnavailable};
use crate::client::ProviderClient;
use crate::protocol::{ComputerError, ComputerErrorCode};

/// Computer-use provider surface shared by the real client and the
/// unavailable placeholder, so the RPC server can hold either.
pub trait ComputerProvider: Send {
    /// Handshake capabilities (cached by the client).
    fn capabilities(&mut self) -> Result<Value, ComputerError>;
    /// Raw provider request (action validation/normalization happens inside
    /// the client, mirroring the oracle provider boundary).
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ComputerError>;
}

impl ComputerProvider for ProviderClient {
    fn capabilities(&mut self) -> Result<Value, ComputerError> {
        self.ensure_ready()
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ComputerError> {
        self.call(method, params)
    }
}

/// Placeholder assembled when the helper is unavailable (missing build or
/// unsupported platform): every call fails with `unsupported_capability`
/// carrying the availability message.
pub struct UnavailableProvider {
    message: String,
}

impl UnavailableProvider {
    pub fn new(reason: ProviderUnavailable, helper: Option<&Path>) -> Self {
        Self {
            message: provider_unavailable_message(reason, helper),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    fn unavailable(&self) -> ComputerError {
        ComputerError::new(
            ComputerErrorCode::UnsupportedCapability,
            self.message.clone(),
        )
    }
}

impl ComputerProvider for UnavailableProvider {
    fn capabilities(&mut self) -> Result<Value, ComputerError> {
        Err(self.unavailable())
    }

    fn request(&mut self, _method: &str, _params: Value) -> Result<Value, ComputerError> {
        Err(self.unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn computer_provider_is_object_safe_and_send() {
        fn assert_send_unsized<T: Send + ?Sized>() {}
        assert_send_unsized::<dyn ComputerProvider>();

        let mut provider: Box<dyn ComputerProvider> = Box::new(UnavailableProvider::new(
            ProviderUnavailable::HelperMissing,
            None,
        ));
        let error = provider.request("listApps", json!({})).unwrap_err();
        assert_eq!(error.code, ComputerErrorCode::UnsupportedCapability);
    }

    #[test]
    fn unavailable_provider_answers_both_methods_with_the_availability_message() {
        for reason in [
            ProviderUnavailable::HelperMissing,
            ProviderUnavailable::PlatformUnsupported,
        ] {
            let mut provider = UnavailableProvider::new(reason, None);
            let expected = provider.message().to_string();
            assert!(!expected.is_empty());

            let capabilities_error = provider.capabilities().unwrap_err();
            assert_eq!(
                capabilities_error.code,
                ComputerErrorCode::UnsupportedCapability
            );
            assert_eq!(capabilities_error.message, expected);

            let request_error = provider
                .request("click", json!({"app": "Finder"}))
                .unwrap_err();
            assert_eq!(request_error.code, ComputerErrorCode::UnsupportedCapability);
            assert_eq!(request_error.message, expected);
        }
    }

    #[test]
    fn helper_missing_message_points_at_the_build_command() {
        let provider = UnavailableProvider::new(ProviderUnavailable::HelperMissing, None);
        assert!(
            provider.message().contains("pnpm build:computer-macos"),
            "message was: {}",
            provider.message()
        );
    }
}
