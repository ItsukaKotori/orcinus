//! Per-plugin secret vault backed by the macOS Keychain.
//!
//! The oracle stores secrets in an Electron `safeStorage`-encrypted file and
//! fails closed when OS encryption is unavailable. The fork replaces the
//! storage substrate with the system Keychain through the `keyring` crate,
//! keeping the same contract: no plaintext fallback, every failure to reach
//! the Keychain surfaces as the `unavailable` error code, and a missing entry
//! reads as `None` (deleting a missing entry is a no-op).
//!
//! Keychain coordinates: service `dev.itsuka.orcinus.plugin-secrets`, account
//! `<pluginKey>:<key>`. Values are capped at 64 KiB, matching the host-api
//! `secrets.set` schema. [`Keychain`] is the injectable seam: production uses
//! [`SystemKeychain`], tests and downstream crates inject in-memory fakes and
//! never touch the real Keychain.

use std::sync::Arc;

use crate::storage::{validate_plugin_key, validate_storage_key};
use crate::types::{PluginError, INVALID_PARAMS, UNAVAILABLE};

pub const KEYCHAIN_SERVICE: &str = "dev.itsuka.orcinus.plugin-secrets";
/// Host-api `secrets.set` value cap.
pub const SECRET_VALUE_MAX_BYTES: usize = 64 * 1024;

/// Plugin-facing secret access; implemented by [`KeychainSecrets`] in
/// production and by fakes in tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, plugin_key: &str, key: &str) -> Result<Option<String>, PluginError>;
    fn set(&self, plugin_key: &str, key: &str, value: &str) -> Result<(), PluginError>;
    fn delete(&self, plugin_key: &str, key: &str) -> Result<(), PluginError>;
}

/// Failure modes a Keychain backend can report. Only `NoEntry` is benign;
/// everything else is fail-closed and maps to the `unavailable` host code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeychainError {
    NoEntry,
    Unavailable(String),
}

/// Raw Keychain operations. Kept separate from [`SecretStore`] so tests can
/// substitute an in-memory implementation.
pub trait Keychain: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError>;
    fn delete(&self, service: &str, account: &str) -> Result<(), KeychainError>;
}

impl<T: Keychain + ?Sized> Keychain for Arc<T> {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError> {
        (**self).get(service, account)
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        (**self).set(service, account, value)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
        (**self).delete(service, account)
    }
}

fn map_keyring_error(error: keyring::Error) -> KeychainError {
    match error {
        keyring::Error::NoEntry => KeychainError::NoEntry,
        other => KeychainError::Unavailable(other.to_string()),
    }
}

/// The real macOS Keychain, via `keyring`. Every operation failure other than
/// "no entry" is reported as unavailable.
pub struct SystemKeychain;

impl Keychain for SystemKeychain {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError> {
        let entry = keyring::Entry::new(service, account).map_err(map_keyring_error)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_keyring_error(error)),
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        let entry = keyring::Entry::new(service, account).map_err(map_keyring_error)?;
        entry.set_password(value).map_err(map_keyring_error)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
        let entry = keyring::Entry::new(service, account).map_err(map_keyring_error)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_keyring_error(error)),
        }
    }
}

pub struct KeychainSecrets {
    service: String,
    keychain: Box<dyn Keychain>,
}

impl std::fmt::Debug for KeychainSecrets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeychainSecrets")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl KeychainSecrets {
    pub fn new(service: impl Into<String>) -> Self {
        Self::with_keychain(service, SystemKeychain)
    }

    /// Injection seam used by tests and downstream fakes.
    pub fn with_keychain(service: impl Into<String>, keychain: impl Keychain + 'static) -> Self {
        Self {
            service: service.into(),
            keychain: Box::new(keychain),
        }
    }

    fn account_for(plugin_key: &str, key: &str) -> Result<String, PluginError> {
        validate_plugin_key(plugin_key)?;
        validate_storage_key(key)?;
        Ok(format!("{plugin_key}:{key}"))
    }
}

fn unavailable_error(reason: &str) -> PluginError {
    PluginError::new(UNAVAILABLE, format!("OS keychain is unavailable: {reason}"))
}

impl SecretStore for KeychainSecrets {
    fn get(&self, plugin_key: &str, key: &str) -> Result<Option<String>, PluginError> {
        let account = Self::account_for(plugin_key, key)?;
        match self.keychain.get(&self.service, &account) {
            Ok(value) => Ok(value),
            Err(KeychainError::NoEntry) => Ok(None),
            Err(KeychainError::Unavailable(reason)) => Err(unavailable_error(&reason)),
        }
    }

    fn set(&self, plugin_key: &str, key: &str, value: &str) -> Result<(), PluginError> {
        let account = Self::account_for(plugin_key, key)?;
        if value.len() > SECRET_VALUE_MAX_BYTES {
            return Err(PluginError::new(
                INVALID_PARAMS,
                format!("value exceeds {SECRET_VALUE_MAX_BYTES} bytes"),
            ));
        }
        match self.keychain.set(&self.service, &account, value) {
            Ok(()) => Ok(()),
            Err(error) => Err(unavailable_error(&match error {
                KeychainError::Unavailable(reason) => reason,
                KeychainError::NoEntry => "keychain entry was not found".to_string(),
            })),
        }
    }

    fn delete(&self, plugin_key: &str, key: &str) -> Result<(), PluginError> {
        let account = Self::account_for(plugin_key, key)?;
        match self.keychain.delete(&self.service, &account) {
            Ok(()) | Err(KeychainError::NoEntry) => Ok(()),
            Err(KeychainError::Unavailable(reason)) => Err(unavailable_error(&reason)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RESERVED_STORAGE_KEYS;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeKeychain {
        entries: Mutex<HashMap<(String, String), String>>,
        calls: Mutex<Vec<(String, String, String)>>,
        unavailable: Mutex<Option<String>>,
    }

    impl FakeKeychain {
        fn fail_with(&self, reason: &str) {
            *self.unavailable.lock().unwrap() = Some(reason.to_string());
        }

        fn failure(&self) -> Option<KeychainError> {
            self.unavailable
                .lock()
                .unwrap()
                .clone()
                .map(KeychainError::Unavailable)
        }

        fn stored(&self, service: &str, account: &str) -> Option<String> {
            self.entries
                .lock()
                .unwrap()
                .get(&(service.to_string(), account.to_string()))
                .cloned()
        }

        fn calls(&self) -> Vec<(String, String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Keychain for FakeKeychain {
        fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError> {
            self.calls.lock().unwrap().push((
                "get".to_string(),
                service.to_string(),
                account.to_string(),
            ));
            if let Some(failure) = self.failure() {
                return Err(failure);
            }
            Ok(self.stored(service, account))
        }

        fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
            self.calls.lock().unwrap().push((
                "set".to_string(),
                service.to_string(),
                account.to_string(),
            ));
            if let Some(failure) = self.failure() {
                return Err(failure);
            }
            self.entries.lock().unwrap().insert(
                (service.to_string(), account.to_string()),
                value.to_string(),
            );
            Ok(())
        }

        fn delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
            self.calls.lock().unwrap().push((
                "delete".to_string(),
                service.to_string(),
                account.to_string(),
            ));
            if let Some(failure) = self.failure() {
                return Err(failure);
            }
            if self
                .entries
                .lock()
                .unwrap()
                .remove(&(service.to_string(), account.to_string()))
                .is_none()
            {
                return Err(KeychainError::NoEntry);
            }
            Ok(())
        }
    }

    fn store_with(fake: &Arc<FakeKeychain>) -> KeychainSecrets {
        KeychainSecrets::with_keychain(KEYCHAIN_SERVICE, Arc::clone(fake))
    }

    #[test]
    fn uses_the_fixed_keychain_service() {
        assert_eq!(KEYCHAIN_SERVICE, "dev.itsuka.orcinus.plugin-secrets");
    }

    #[test]
    fn round_trips_and_maps_service_and_account() {
        let fake = Arc::new(FakeKeychain::default());
        let store = store_with(&fake);
        store.set("acme.demo", "token", "top-secret").unwrap();
        assert_eq!(
            store.get("acme.demo", "token").unwrap(),
            Some("top-secret".to_string())
        );
        assert_eq!(
            fake.stored(KEYCHAIN_SERVICE, "acme.demo:token"),
            Some("top-secret".to_string())
        );
        assert!(fake.calls().iter().any(|(operation, service, account)| {
            operation == "set" && service == KEYCHAIN_SERVICE && account == "acme.demo:token"
        }));

        store.delete("acme.demo", "token").unwrap();
        assert_eq!(store.get("acme.demo", "token").unwrap(), None);
    }

    #[test]
    fn isolates_plugin_namespaces() {
        let fake = Arc::new(FakeKeychain::default());
        let store = store_with(&fake);
        store.set("acme.first", "token", "first").unwrap();
        assert_eq!(
            store.get("acme.first", "token").unwrap(),
            Some("first".to_string())
        );
        assert_eq!(store.get("acme.second", "token").unwrap(), None);
    }

    #[test]
    fn fails_closed_when_keychain_is_unavailable() {
        let fake = Arc::new(FakeKeychain::default());
        let store = store_with(&fake);
        fake.fail_with("keychain locked");

        let errors = [
            store.get("acme.demo", "token").unwrap_err(),
            store.set("acme.demo", "token", "value").unwrap_err(),
            store.delete("acme.demo", "token").unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.code(), UNAVAILABLE);
            assert!(
                error.message().contains("keychain locked"),
                "unexpected message: {}",
                error.message()
            );
        }
        assert_eq!(fake.stored(KEYCHAIN_SERVICE, "acme.demo:token"), None);
    }

    #[test]
    fn no_entry_reads_none_and_delete_is_idempotent() {
        let fake = Arc::new(FakeKeychain::default());
        let store = store_with(&fake);
        assert_eq!(store.get("acme.demo", "missing").unwrap(), None);
        store.delete("acme.demo", "missing").unwrap();
    }

    #[test]
    fn maps_keyring_errors_to_unavailable() {
        assert_eq!(
            map_keyring_error(keyring::Error::NoEntry),
            KeychainError::NoEntry
        );
        let platform = keyring::Error::PlatformFailure(Box::new(std::io::Error::other("denied")));
        match map_keyring_error(platform) {
            KeychainError::Unavailable(message) => assert!(message.contains("denied")),
            other => panic!("unexpected mapping: {other:?}"),
        }
        let access = keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("locked")));
        assert!(matches!(
            map_keyring_error(access),
            KeychainError::Unavailable(_)
        ));
        assert!(matches!(
            map_keyring_error(keyring::Error::BadEncoding(vec![0xff])),
            KeychainError::Unavailable(_)
        ));
    }

    #[test]
    fn rejects_oversized_values_and_invalid_keys() {
        let fake = Arc::new(FakeKeychain::default());
        let store = store_with(&fake);

        let oversized = "x".repeat(SECRET_VALUE_MAX_BYTES + 1);
        let error = store.set("acme.demo", "token", &oversized).unwrap_err();
        assert_eq!(error.code(), INVALID_PARAMS);
        assert_eq!(
            error.message(),
            format!("value exceeds {SECRET_VALUE_MAX_BYTES} bytes")
        );

        for reserved in RESERVED_STORAGE_KEYS {
            let error = store.set("acme.demo", reserved, "v").unwrap_err();
            assert_eq!(error.code(), INVALID_PARAMS);
            assert_eq!(error.message(), "reserved storage key");
        }
        assert_eq!(
            store.get("acme.demo", "").unwrap_err().code(),
            INVALID_PARAMS
        );

        let error = store.get("demo", "token").unwrap_err();
        assert_eq!(error.code(), INVALID_PARAMS);
        assert!(error.message().contains("unsafe plugin key"));

        assert!(fake.calls().is_empty(), "keychain must not be touched");
    }

    #[test]
    fn accepts_values_at_the_exact_cap() {
        let fake = Arc::new(FakeKeychain::default());
        let store = store_with(&fake);
        let exact = "x".repeat(SECRET_VALUE_MAX_BYTES);
        store.set("acme.demo", "exact", &exact).unwrap();
        assert_eq!(
            store.get("acme.demo", "exact").unwrap().unwrap().len(),
            SECRET_VALUE_MAX_BYTES
        );
    }

    #[test]
    fn system_keychain_construction_is_inert() {
        let _ = KeychainSecrets::new(KEYCHAIN_SERVICE);
    }
}
