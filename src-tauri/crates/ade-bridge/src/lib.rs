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
///
/// Exception: `pty_attach`（规格 §3.2 修订二）——`Channel<InvokeResponseBody>`
/// 参数无 `specta::Type`，无法走 `#[specta::specta]`/bindings，故在 specta
/// builder 之外以 `generate_handler!` 手工注册（先于 specta 分发命中；未命中
/// 落回 specta 面，命令清单与 bindings 的其余部分仍不漂移）。
pub fn invoke_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    let specta = specta_export::bridge_builder().invoke_handler();
    move |invoke| {
        if invoke.message.command() == "pty_attach" {
            commands::pty::handle_pty_attach_invoke(invoke);
            true
        } else {
            specta(invoke)
        }
    }
}
