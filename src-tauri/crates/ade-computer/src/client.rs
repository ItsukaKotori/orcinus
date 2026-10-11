//! Provider client (spec §3.2): protocol gate, capabilities cache, single-flight
//! serialization, action validation and verification normalization. Mirrors
//! `orca:src/main/computer/macos-native-provider-client.ts`.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};

use crate::protocol::{ComputerError, ComputerErrorCode};
use crate::transport::HelperTransport;
use crate::validation::{is_action_method, validate_computer_action};
use crate::verification::normalize_action_result;

/// Helper protocol version required by this client (oracle
/// `REQUIRED_MACOS_PROVIDER_PROTOCOL_VERSION`).
pub const PROVIDER_PROTOCOL_VERSION: u64 = 1;

/// Serialized client for one helper app: the first successful `handshake`
/// caches the helper capabilities, and every request goes through one mutex
/// so only a single request is ever in flight (oracle "one at a time"
/// semantics). The helper itself is spawned lazily on first use and restarted
/// by the transport after IO/timeout failures.
pub struct ProviderClient {
    helper_app: PathBuf,
    request_timeout: Duration,
    inner: Mutex<ClientState>,
}

#[derive(Default)]
struct ClientState {
    transport: Option<HelperTransport>,
    capabilities: Option<Value>,
}

impl ProviderClient {
    pub fn new(helper_app: PathBuf, request_timeout: Duration) -> Self {
        Self {
            helper_app,
            request_timeout,
            inner: Mutex::new(ClientState::default()),
        }
    }

    /// Run (or return the cached) `handshake`, enforcing the protocol version
    /// gate. The handshake result is the provider capabilities value.
    pub fn ensure_ready(&mut self) -> Result<Value, ComputerError> {
        let mut state = self.lock_inner();
        self.ensure_ready_locked(&mut state)
    }

    /// Send one request. Action methods are validated before anything is sent
    /// and their results are verification-normalized; all other questions
    /// (`handshake`, listing, ...) pass through unchanged.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, ComputerError> {
        let is_action = is_action_method(method);
        if is_action {
            validate_computer_action(method, &params)?;
        }
        let mut state = self.lock_inner();
        if method != "handshake" {
            self.ensure_ready_locked(&mut state)?;
        }
        let mut result = self.transport(&mut state)?.request(method, &params)?;
        if is_action {
            normalize_action_result(&mut result);
        }
        Ok(result)
    }

    /// Terminate the helper and drop the capabilities cache so the next call
    /// performs a fresh handshake on a fresh helper.
    pub fn shutdown(&mut self) {
        let mut state = self.lock_inner();
        if let Some(mut transport) = state.transport.take() {
            transport.shutdown();
        }
        state.capabilities = None;
    }

    fn lock_inner(&self) -> MutexGuard<'_, ClientState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn ensure_ready_locked(&self, state: &mut ClientState) -> Result<Value, ComputerError> {
        if let Some(capabilities) = state.capabilities.as_ref() {
            return Ok(capabilities.clone());
        }
        let capabilities = self.transport(state)?.request("handshake", &json!({}))?;
        match protocol_version(&capabilities) {
            Some(version) if version == PROVIDER_PROTOCOL_VERSION as f64 => {
                state.capabilities = Some(capabilities.clone());
                Ok(capabilities)
            }
            Some(_) => Err(ComputerError::new(
                ComputerErrorCode::ProviderIncompatible,
                format!(
                    "native macOS provider protocol {} is incompatible with required protocol {PROVIDER_PROTOCOL_VERSION}",
                    capabilities
                        .get("protocolVersion")
                        .map(Value::to_string)
                        .unwrap_or_default()
                ),
            )),
            None => Err(ComputerError::new(
                ComputerErrorCode::ProviderIncompatible,
                format!(
                    "native macOS provider handshake did not report a protocol version (required protocol {PROVIDER_PROTOCOL_VERSION})"
                ),
            )),
        }
    }

    fn transport<'a>(
        &'a self,
        state: &'a mut ClientState,
    ) -> Result<&'a mut HelperTransport, ComputerError> {
        if state.transport.is_none() {
            state.transport = Some(HelperTransport::start(
                &self.helper_app,
                self.request_timeout,
            )?);
        }
        Ok(state.transport.as_mut().expect("transport started above"))
    }
}

/// `protocolVersion` as a whole number (`1` and `1.0` are both protocol 1).
fn protocol_version(capabilities: &Value) -> Option<f64> {
    let number = capabilities.get("protocolVersion")?.as_f64()?;
    (number.fract() == 0.0).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_protocol_version_is_one() {
        assert_eq!(PROVIDER_PROTOCOL_VERSION, 1);
    }

    #[test]
    fn provider_client_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ProviderClient>();
    }

    #[test]
    fn protocol_version_accepts_integer_and_integral_float_forms() {
        assert_eq!(protocol_version(&json!({"protocolVersion": 1})), Some(1.0));
        assert_eq!(
            protocol_version(&json!({"protocolVersion": 1.0})),
            Some(1.0)
        );
        assert_eq!(protocol_version(&json!({"protocolVersion": 2})), Some(2.0));
        assert_eq!(protocol_version(&json!({"protocolVersion": 1.5})), None);
        assert_eq!(protocol_version(&json!({"protocolVersion": "1"})), None);
        assert_eq!(protocol_version(&json!({})), None);
    }
}
