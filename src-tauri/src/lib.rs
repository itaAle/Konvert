//! Tauri shell of Konvert: a thin IPC layer over `konvert-core`.
//!
//! The frontend never builds FFmpeg arguments and never receives broad
//! permissions: it can only call the commands listed in `build.rs` and granted
//! in `capabilities/default.json`.

mod commands;
mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = state::AppState::initialize(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::engine_info,
            commands::probe_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Konvert");
}
