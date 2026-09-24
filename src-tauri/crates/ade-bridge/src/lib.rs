pub mod commands;
pub mod errors;
pub mod events;
pub mod json;
pub mod specta_export;
pub mod state;

pub use errors::BridgeError;
pub use state::{AppState, BootstrapPayload};

/// The Tauri invoke handler for every bridge command. Uses the specta builder
/// so the command list and the exported bindings can never drift apart.
pub fn invoke_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    specta_export::bridge_builder().invoke_handler()
}
