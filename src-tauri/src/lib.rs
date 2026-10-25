//! Tauri shell of Konvert: a thin IPC layer over `konvert-core`.
//!
//! The frontend never builds FFmpeg arguments and never receives broad
//! permissions: it can only call the commands listed in `build.rs` and granted
//! in `capabilities/default.json`.

mod commands;
mod debug_log;
mod jobs;
mod prefs;
mod state;

use std::sync::atomic::Ordering;

use tauri::{Emitter, Manager, RunEvent, Theme, WebviewWindow, WindowEvent};

use crate::prefs::ThemePref;
use crate::state::AppState;

/// Event asking the UI to confirm closing while conversions are running.
const CLOSE_REQUESTED_EVENT: &str = "close-requested";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Used from Rust only (no frontend permissions): native pickers and
        // "open file / show in folder / releases page".
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let window = app.get_webview_window("main");
            let vibrancy = window.as_ref().is_some_and(apply_backdrop);
            let state = AppState::initialize(app.handle(), vibrancy)?;
            if let Some(window) = &window {
                apply_theme(window, state.prefs.get().theme);
            }
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Do not lose a long conversion to an accidental click on ×.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.jobs.has_active() && !state.close_confirmed.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.emit(CLOSE_REQUESTED_EVENT, ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::engine_info,
            commands::platform_info,
            commands::app_ready,
            commands::get_preferences,
            commands::set_preferences,
            commands::pick_folder,
            commands::available_space,
            commands::open_releases_page,
            commands::reveal_log,
            commands::clear_log,
            commands::confirm_close,
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
                && let Some(state) = app.try_state::<AppState>()
            {
                state.shutdown();
            }
        });
}

/// Window theme (title bar and Mica tint). The web content follows the same
/// preference through `data-theme` on the document.
pub(crate) fn apply_theme(window: &WebviewWindow, theme: ThemePref) {
    let _ = window.set_theme(match theme {
        ThemePref::Auto => None,
        ThemePref::Light => Some(Theme::Light),
        ThemePref::Dark => Some(Theme::Dark),
    });
}

/// Mica backdrop on Windows 11; `false` elsewhere (the UI then paints an opaque
/// background, since the window itself is transparent).
#[cfg(windows)]
fn apply_backdrop(window: &WebviewWindow) -> bool {
    window_vibrancy::apply_mica(window, None).is_ok()
}

#[cfg(not(windows))]
fn apply_backdrop(_window: &WebviewWindow) -> bool {
    false
}
