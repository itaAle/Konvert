//! IPC commands exposed to the frontend.
//!
//! Errors are returned as [`ErrorPayload`] (`{ code, details }`): the UI maps the
//! code to a translated message and shows `details` only on request.

use std::path::PathBuf;

use konvert_core::{ErrorPayload, MediaInfo};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub app_version: String,
    pub ffmpeg_version: String,
}

/// Versions of Konvert and of the bundled FFmpeg.
#[tauri::command]
pub async fn engine_info(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<EngineInfo, ErrorPayload> {
    let bins = state.binaries().map_err(|e| e.payload())?;
    let ffmpeg_version = konvert_core::engine_version(bins)
        .await
        .map_err(|e| e.payload())?;
    Ok(EngineInfo {
        app_version: app.package_info().version.to_string(),
        ffmpeg_version,
    })
}

/// Analyses a local media file.
#[tauri::command]
pub async fn probe_file(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<MediaInfo, ErrorPayload> {
    let bins = state.binaries().map_err(|e| e.payload())?;
    konvert_core::probe(bins, &path)
        .await
        .map_err(|e| e.payload())
}
