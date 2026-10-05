use ade_bridge::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .on_window_event(|window, event| {
            // Window-close quit (spec §3.4): `CloseRequested` is the only event
            // where the webview is still alive — tauri-runtime-wry emits
            // `RunEvent::ExitRequested { code: None }` from inside `Destroyed`,
            // i.e. after the WKWebView is gone, so the flush handshake can never
            // be acked there. Latch-gated: the flush waiter's programmatic
            // `window.close()` re-enters this handler and must NOT be prevented
            // again, or the quit never converges.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if let Some(webview) = window.get_webview_window(window.label()) {
                    if let Some(state) = window.try_state::<ade_bridge::AppState>() {
                        if state.begin_close_with_session_flush(webview) {
                            api.prevent_close();
                        }
                    }
                }
            }
        })
        .setup(|app| {
            // PtyHost：进程内数据面订阅 + 会话注册表（规格 §3.2 修订二：下行走
            // `pty_attach` 命令的 Tauri Channel 分块，无端口/token；exit watcher
            // 挂 tauri 异步运行时；spawned/exit 事件回调由 `AppState::initialize`
            // 转 Tauri emit（`pty:spawned`/`pty:exit`）。
            let pty_host = ade_pty::PtyHost::start(tauri::async_runtime::handle().inner().clone())?;
            let state = AppState::initialize(app.handle(), pty_host)?;
            // Bootstrap payload must be in place before the document parses so
            // `settings.getSync()`/`platform.get()` can read it synchronously.
            let bootstrap = state.bootstrap_payload();
            let script = format!(
                "window.__ADE_BOOTSTRAP__ = {};",
                serde_json::to_string(&bootstrap)?
            );
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Orcinus")
            .inner_size(1440.0, 900.0)
            .initialization_script(&script)
            .build()?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(ade_bridge::invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while building Orcinus");

    app.run(|app_handle, event| {
        // 仅剩 Exit 收尾（`if let` 保持 clippy 干净）：窗口关闭的 flush 握手
        // 编排已迁至 builder 的 `on_window_event`（CloseRequested 时 webview
        // 仍存活；ExitRequested 发生于 Destroyed 之后，emit 只能到达已销毁
        // 的 webview，故该拦截已拆除）。
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<AppState>() {
                // Debounced settings/ui writes may still be pending; flush them so a
                // quick quit after a change cannot lose the update (spec §4.1).
                state.flush_pending_writes();
                if let Err(error) = state.session_store().checkpoint_truncate() {
                    eprintln!("[ade] failed to checkpoint session store on exit: {error}");
                }
                // 逐会话 kill（带 2s+2s 升级时限）——订阅流随会话退出自然终止
                // （规格 §3.1：app 退出全量收尾）。
                state.pty_host.shutdown_all();
            }
        }
    });
}
