//! IPC commands exposed to the frontend.
//!
//! Every command must also be listed in `build.rs` and granted in
//! `capabilities/default.json`. Errors are returned as [`ErrorPayload`]
//! (`{ code, details }`): the UI maps the code to a translated message and shows
//! `details` only on request.

use std::path::PathBuf;

use konvert_core::{
    ConversionPlan, ConversionRequest, CoreError, ErrorPayload, MediaInfo, TargetOption,
    available_targets, plan_conversion as plan,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use ts_rs::TS;

use crate::state::AppState;

/// Extensions offered by the file picker (anything FFmpeg can read is accepted
/// by drag and drop; this list only filters the dialog).
const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "wav", "wave", "flac", "ogg", "oga", "opus", "wma", "aif", "aiff",
    "aifc", "alac", "ape", "wv", "mka", "ac3", "mp4", "m4v", "mkv", "mov", "qt", "webm", "avi",
    "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "mts", "3gp", "ogv", "vob", "gif",
];

fn payload(e: CoreError) -> ErrorPayload {
    e.payload()
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineInfo {
    pub app_version: String,
    pub ffmpeg_version: String,
}

/// Versions of Konvert and of the bundled FFmpeg.
#[tauri::command]
pub async fn engine_info(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<EngineInfo, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?;
    let ffmpeg_version = konvert_core::engine_version(bins).await.map_err(payload)?;
    Ok(EngineInfo {
        app_version: app.package_info().version.to_string(),
        ffmpeg_version,
    })
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlatformInfo {
    /// Translucent window backdrop (Mica) is active.
    pub vibrancy: bool,
}

#[tauri::command]
pub fn platform_info(state: State<'_, AppState>) -> PlatformInfo {
    PlatformInfo {
        vibrancy: state.vibrancy,
    }
}

/// Shows the main window once the UI has rendered (avoids a blank flash).
#[tauri::command]
pub fn app_ready(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// A file ready to be converted.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileAnalysis {
    pub path: PathBuf,
    /// File name for display.
    pub name: String,
    pub media: MediaInfo,
    /// Output formats that make sense for this file.
    pub targets: Vec<TargetOption>,
}

/// Analyses a local media file and lists the sensible output formats.
#[tauri::command]
pub async fn analyze_file(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<FileAnalysis, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?;
    let media = konvert_core::probe(bins, &path).await.map_err(payload)?;
    let targets = available_targets(&media, &path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(FileAnalysis {
        path,
        name,
        media,
        targets,
    })
}

/// Decisions, warnings and size estimate for a conversion (pure, no disk access).
#[tauri::command]
pub fn plan_conversion(
    path: PathBuf,
    media: MediaInfo,
    request: ConversionRequest,
) -> Result<ConversionPlan, ErrorPayload> {
    plan(&media, &request, &path).map_err(payload)
}

/// Queues a conversion; progress arrives through `job-update` events.
#[tauri::command]
pub fn start_conversion(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
    request: ConversionRequest,
    output_dir: Option<PathBuf>,
) -> Result<u64, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?.clone();
    if !path.is_file() {
        return Err(payload(CoreError::InputNotFound));
    }
    // Default destination: next to the original file.
    let output_dir = output_dir
        .or_else(|| path.parent().map(PathBuf::from))
        .ok_or_else(|| payload(CoreError::InputNotFound))?;
    Ok(state.jobs.enqueue(
        app,
        bins,
        state.workspace.clone(),
        path,
        request,
        output_dir,
    ))
}

#[tauri::command]
pub fn cancel_conversion(state: State<'_, AppState>, job_id: u64) -> bool {
    state.jobs.cancel(job_id)
}

/// Opens a file produced in this session with its default application.
#[tauri::command]
pub fn open_output(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: u64,
) -> Result<(), ErrorPayload> {
    let path = state
        .jobs
        .output(job_id)
        .ok_or_else(|| payload(CoreError::InputNotFound))?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| payload(CoreError::Io(std::io::Error::other(e.to_string()))))
}

/// Shows a file produced in this session in the system file manager.
#[tauri::command]
pub fn reveal_output(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: u64,
) -> Result<(), ErrorPayload> {
    let path = state
        .jobs
        .output(job_id)
        .ok_or_else(|| payload(CoreError::InputNotFound))?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| payload(CoreError::Io(std::io::Error::other(e.to_string()))))
}

/// Native "open files" dialog. Returns `None` when cancelled.
#[tauri::command]
pub async fn pick_files(
    app: AppHandle,
    title: String,
    filter_name: String,
) -> Option<Vec<PathBuf>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .add_filter(filter_name, MEDIA_EXTENSIONS)
        .pick_files(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.ok().flatten()?;
    Some(
        picked
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .collect(),
    )
}
