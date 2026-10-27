//! Tauri shell of Konvert: a thin IPC layer over `konvert-core`.
//!
//! The frontend never builds FFmpeg arguments and never receives broad
//! permissions: it can only call the commands listed in `build.rs` and granted
//! in `capabilities/default.json`.

mod commands;
mod debug_log;
mod jobs;
mod launch;
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
    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch ("Open with" while Konvert
        // is running) hands its files to this instance and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let files = launch::files_from_args(argv.into_iter().map(Into::into), cwd.as_ref());
            launch::open_files(app, files);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Used from Rust only (no frontend permissions): native pickers and
        // "open file / show in folder / releases page".
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Managed before setup: macOS may deliver opened files before the window exists.
        .manage(launch::LaunchFiles::default())
        .setup(|app| {
            let cwd = std::env::current_dir().unwrap_or_default();
            launch::open_files(
                app.handle(),
                launch::files_from_args(std::env::args_os(), &cwd),
            );
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
            commands::pdf_thumbnails,
            commands::picture_thumbnail,
            commands::start_conversion,
            commands::start_combined,
            commands::cancel_conversion,
            commands::open_output,
            commands::reveal_output,
            commands::pick_files,
            commands::take_launch_files,
            commands::open_licenses,
        ])
        .build(tauri::generate_context!());

    // Without a window (e.g. WebView2 missing) explain the problem instead of
    // exiting silently.
    let app = app.unwrap_or_else(|e| launch::fatal_startup_error(&e.to_string()));
    app.run(|app, event| match event {
        // ⌘Q on macOS quits without closing the window first: ask as for ×.
        RunEvent::ExitRequested { api, .. } => {
            if let Some(state) = app.try_state::<AppState>()
                && state.jobs.has_active()
                && !state.close_confirmed.load(Ordering::Relaxed)
            {
                api.prevent_exit();
                let _ = app.emit(CLOSE_REQUESTED_EVENT, ());
            }
        }
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                state.shutdown();
            }
        }
        // "Open with", double-click on a document and drops on the Dock icon.
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => {
            let files = urls
                .into_iter()
                .filter_map(|url| url.to_file_path().ok())
                .filter(|path| path.is_file())
                .collect();
            launch::open_files(app, files);
        }
        _ => {}
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
