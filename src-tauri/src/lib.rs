use ade_bridge::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
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
        match event {
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                // code: None = window-close-initiated quit (spec §3.4): give the
                // renderer one flush window, then exit from the waiter thread.
                // Some(_) = explicit exit() from that waiter — pass through.
                if code.is_some() {
                    return;
                }
                if let Some(state) = app_handle.try_state::<AppState>() {
                    state.flush_session_then_exit();
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
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
            _ => {}
        }
    });
}
