//! Structural service surface the host facade delegates to.
//!
//! The Tauri bridge implements [`PluginHostServices`] over live workspace,
//! notification, storage, and tab services; tests and the relay conformance
//! path bind fakes. Everything the host facade needs from the app crosses
//! this seam, so `ade-plugins` stays Tauri-free.

use serde_json::{Map, Value};

use crate::plugin_settings::PluginSettingsStore;
use crate::secrets::SecretStore;
use crate::storage::StorageStore;
use crate::types::{PluginError, PluginScope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeContext {
    pub worktree_id: String,
    pub branch: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTabResult {
    pub instance_id: String,
    pub worktree_id: String,
}

/// Failure modes a host service can report; the code is a stable host error
/// code (e.g. `rate_limited`, `unavailable`, `action_failed`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct HostServiceError {
    code: &'static str,
    message: String,
}

impl HostServiceError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Plugin-private key-value persistence (`storage.*`).
pub trait PluginStorageAccess: Send + Sync {
    fn get(&self, plugin_key: &str, key: &str) -> Result<Option<Value>, PluginError>;
    fn set(&self, plugin_key: &str, key: &str, value: Value) -> Result<(), PluginError>;
    fn delete(&self, plugin_key: &str, key: &str) -> Result<(), PluginError>;
    fn keys(&self, plugin_key: &str) -> Result<Vec<String>, PluginError>;
}

/// Plugin-owned settings (`settings.*`).
pub trait PluginSettingsAccess: Send + Sync {
    fn get_all(&self, plugin_key: &str) -> Result<Map<String, Value>, PluginError>;
    fn set(&self, plugin_key: &str, key: &str, value: Value) -> Result<(), PluginError>;
}

pub trait PluginHostServices: Send + Sync {
    /// The focused worktree, when one exists.
    fn active_worktree_context(&self) -> Option<WorktreeContext>;
    /// Terminals belonging to `worktree_id`; used for the write-target
    /// whitelist immediately before routing plugin input.
    fn list_worktree_terminals(&self, worktree_id: &str) -> Vec<TerminalRef>;
    fn send_terminal_text(
        &self,
        terminal_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<bool, HostServiceError>;
    fn dispatch_notification(
        &self,
        plugin_key: &str,
        title: &str,
        body: Option<&str>,
    ) -> Result<bool, HostServiceError>;
    fn storage(&self) -> &dyn PluginStorageAccess;
    fn settings(&self) -> &dyn PluginSettingsAccess;
    fn secrets(&self) -> &dyn SecretStore;
    /// Events actually subscribed for the plugin (an unknown name is dropped).
    fn subscribe_events(&self, plugin_key: &str, events: &[String]) -> Vec<String>;
    /// Open (or focus) a plugin tab; dedup, instance caps, and rate limiting
    /// are service-layer concerns reported through [`HostServiceError`].
    #[allow(clippy::too_many_arguments)]
    fn open_tab(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
        tab_id: &str,
        title: Option<&str>,
        params: Option<Value>,
        worktree_hint: Option<&str>,
    ) -> Result<OpenTabResult, HostServiceError>;
    /// Tab ids declared by the plugin for the given scope (for `openTab`
    /// validation against `contributes.tabs`).
    fn declared_tabs(
        &self,
        plugin_key: &str,
        scope: PluginScope,
        repo_id: Option<&str>,
    ) -> Vec<String>;
}

impl PluginStorageAccess for StorageStore {
    fn get(&self, plugin_key: &str, key: &str) -> Result<Option<Value>, PluginError> {
        StorageStore::get(self, plugin_key, key)
    }

    fn set(&self, plugin_key: &str, key: &str, value: Value) -> Result<(), PluginError> {
        StorageStore::set(self, plugin_key, key, value)
    }

    fn delete(&self, plugin_key: &str, key: &str) -> Result<(), PluginError> {
        StorageStore::delete(self, plugin_key, key)
    }

    fn keys(&self, plugin_key: &str) -> Result<Vec<String>, PluginError> {
        StorageStore::keys(self, plugin_key)
    }
}

impl PluginSettingsAccess for PluginSettingsStore {
    fn get_all(&self, plugin_key: &str) -> Result<Map<String, Value>, PluginError> {
        PluginSettingsStore::get_all(self, plugin_key)
    }

    fn set(&self, plugin_key: &str, key: &str, value: Value) -> Result<(), PluginError> {
        PluginSettingsStore::set(self, plugin_key, key, value)
    }
}
