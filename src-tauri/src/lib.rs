//! Tauri shell of Konvert: a thin IPC layer over `konvert-core`.
//!
//! The frontend never builds FFmpeg arguments and never receives broad
//! permissions: it can only call the commands listed in `build.rs` and granted
//! in `capabilities/default.json`.

mod commands;
mod jobs;
mod state;

use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Used from Rust only (no frontend permissions): native pickers and
        // "open file / show in folder".
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let vibrancy = app
                .get_webview_window("main")
                .is_some_and(|w| apply_backdrop(&w));
            let state = state::AppState::initialize(app.handle(), vibrancy)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::engine_info,
            commands::platform_info,
            commands::app_ready,
            commands::analyze_file,
            commands::plan_conversion,
            commands::start_conversion,
            commands::cancel_conversion,
            commands::open_output,
            commands::reveal_output,
            commands::pick_files,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Konvert")
        .run(|app, event| {
            if let RunEvent::Exit = event
                && let Some(state) = app.try_state::<state::AppState>()
            {
                state.shutdown();
            }
        });
}

/// Mica backdrop on Windows 11; `false` elsewhere (the UI then paints an opaque
/// background, since the window itself is transparent).
#[cfg(windows)]
fn apply_backdrop(window: &tauri::WebviewWindow) -> bool {
    window_vibrancy::apply_mica(window, None).is_ok()
}

#[cfg(not(windows))]
fn apply_backdrop(_window: &tauri::WebviewWindow) -> bool {
    false
}
