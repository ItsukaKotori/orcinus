use ade_bridge::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let state = AppState::initialize(app.handle())?;
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
        // Debounced settings/ui writes may still be pending; flush them so a
        // quick quit after a change cannot lose the update (spec §4.1).
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.flush_pending_writes();
            }
        }
    });
}
