//! IPC commands exposed to the frontend.
//!
//! Every command must also be listed in `build.rs` and granted in
//! `capabilities/default.json`. Errors are returned as [`ErrorPayload`]
//! (`{ code, details }`): the UI maps the code to a translated message and shows
//! `details` only on request.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use konvert_core::pdf::Thumbnail;
use konvert_core::{
    ConversionPlan, ConversionRequest, CoreError, ErrorPayload, MediaInfo, Secret, TargetFormat,
    TargetOption, available_targets, compress::compress_format, formats::supported_targets,
    plan_conversion as plan,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use ts_rs::TS;

use crate::launch::LaunchFiles;
use crate::prefs::Preferences;
use crate::state::AppState;

/// The only web address Konvert knows. It is opened in the system browser, and
/// only when the user clicks "Releases page"; the app itself never connects.
pub const RELEASES_URL: &str = "https://github.com/itaAle/Konvert/releases";

/// Extensions offered by the file picker (anything FFmpeg can read is accepted
/// by drag and drop; this list only filters the dialog).
const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "wav", "wave", "flac", "ogg", "oga", "opus", "wma", "aif", "aiff",
    "aifc", "alac", "ape", "wv", "mka", "ac3", "eac3", "mp2", "caf", "mp4", "m4v", "mkv", "mov",
    "qt", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "mts", "3gp", "ogv", "vob",
    "gif", "apng", "jpg", "jpeg", "jpe", "jfif", "png", "webp", "avif", "heic", "heif", "hif",
    "bmp", "tif", "tiff", "ico", "svg", "svgz", "pdf", "docx",
];

/// Extensions FFmpeg reads only when built with libjxl: the Windows and Linux
/// builds, not the macOS one (offered there neither by the picker nor by "Open With").
const LIBJXL_EXTENSIONS: &[&str] = &["jxl"];

/// Every extension the file picker offers on this platform.
fn picker_extensions() -> Vec<&'static str> {
    let libjxl = if cfg!(target_os = "macos") {
        &[][..]
    } else {
        LIBJXL_EXTENSIONS
    };
    MEDIA_EXTENSIONS.iter().chain(libjxl).copied().collect()
}

fn payload(e: CoreError) -> ErrorPayload {
    e.payload()
}

fn io_payload(e: impl std::fmt::Display) -> ErrorPayload {
    payload(CoreError::Io(std::io::Error::other(e.to_string())))
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

// --- Preferences ---------------------------------------------------------------

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> Preferences {
    state.prefs.get()
}

/// Saves the preferences and applies them (theme, debug log) immediately.
#[tauri::command]
pub fn set_preferences(
    window: WebviewWindow,
    state: State<'_, AppState>,
    preferences: Preferences,
) -> Result<(), ErrorPayload> {
    crate::apply_theme(&window, preferences.theme);
    state.log.set_enabled(preferences.debug_logging);
    state.prefs.set(preferences).map_err(io_payload)
}

/// Native folder picker for the destination folder.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, title: String) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
    rx.await.ok().flatten()?.into_path().ok()
}

/// Free bytes on the volume of `dir` (for the "not enough space" warning).
#[tauri::command]
pub fn available_space(dir: PathBuf) -> Option<u64> {
    konvert_core::available_space(&dir)
}

/// Opens the GitHub releases page in the system browser (user click only).
#[tauri::command]
pub fn open_releases_page(app: AppHandle) -> Result<(), ErrorPayload> {
    app.opener()
        .open_url(RELEASES_URL, None::<&str>)
        .map_err(io_payload)
}

/// Shows the debug log in the file manager (or its folder if it is empty).
#[tauri::command]
pub fn reveal_log(app: AppHandle, state: State<'_, AppState>) -> Result<(), ErrorPayload> {
    let path = state.log.path();
    if path.is_file() {
        app.opener().reveal_item_in_dir(path).map_err(io_payload)
    } else {
        let dir = path.parent().ok_or_else(|| io_payload("no log folder"))?;
        std::fs::create_dir_all(dir).map_err(io_payload)?;
        app.opener()
            .open_path(dir.to_string_lossy(), None::<&str>)
            .map_err(io_payload)
    }
}

#[tauri::command]
pub fn clear_log(state: State<'_, AppState>) -> Result<(), ErrorPayload> {
    state.log.clear().map_err(io_payload)
}

/// The user confirmed closing the window although conversions are running.
#[tauri::command]
pub fn confirm_close(window: WebviewWindow, state: State<'_, AppState>) {
    state.close_confirmed.store(true, Ordering::Relaxed);
    state.jobs.cancel_all();
    let _ = window.close();
}

// --- Files and conversions -------------------------------------------------------

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
    /// Format of a smaller copy of the file (its own), when Konvert can make one.
    pub compress: Option<TargetFormat>,
    /// Format of an edited copy of a picture (its own), when Konvert can write it.
    pub edit: Option<TargetFormat>,
}

/// Analyses a local media file and lists the sensible output formats.
#[tauri::command]
pub async fn analyze_file(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<FileAnalysis, ErrorPayload> {
    let password = state.password(&path);
    analysis(&state, path, password.as_ref()).await
}

/// Opens a PDF protected by a password: the password is checked and kept in
/// memory, for this file only, while it is in the list (see
/// `forget_pdf_password`). Never logged nor saved. Gives the analysis.
#[tauri::command]
pub async fn unlock_pdf(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Secret,
) -> Result<FileAnalysis, ErrorPayload> {
    if password.is_empty() || !konvert_core::pdf::is_pdf(&path) {
        return Err(payload(CoreError::PdfWrongPassword));
    }
    let found = analysis(&state, path.clone(), Some(&password)).await?;
    if found.media.document.as_ref().is_some_and(|d| d.locked)
        && let Ok(mut passwords) = state.passwords.lock()
    {
        passwords.insert(path, password);
    }
    Ok(found)
}

/// Forgets the password of a PDF that left the list.
#[tauri::command]
pub fn forget_pdf_password(state: State<'_, AppState>, path: PathBuf) {
    state.forget(&path);
}

async fn analysis(
    state: &AppState,
    path: PathBuf,
    password: Option<&Secret>,
) -> Result<FileAnalysis, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?;
    let media = konvert_core::probe_with(bins, &path, password)
        .await
        .map_err(payload)?;
    // If the engine cannot be asked, formats needing an optional encoder are left out.
    let encoders = state
        .encoders
        .get_or_init(|| async {
            konvert_core::engine_encoders(bins)
                .await
                .unwrap_or_default()
        })
        .await;
    let targets = supported_targets(available_targets(&media, &path), encoders);
    let encodable = |format: &TargetFormat| {
        konvert_core::formats::spec(*format)
            .requires_encoder
            .is_none_or(|encoder| encoders.contains(encoder))
    };
    let compress = compress_format(&media, &path).filter(encodable);
    let edit = konvert_core::edit::edit_format(&media, &path).filter(encodable);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(FileAnalysis {
        path,
        name,
        media,
        targets,
        compress,
        edit,
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

/// Small pictures of some pages of a PDF, for choosing, turning and moving
/// pages. Drawn by Konvert itself on a worker thread; nothing leaves the computer.
#[tauri::command]
pub async fn pdf_thumbnails(
    state: State<'_, AppState>,
    path: PathBuf,
    pages: Vec<u32>,
    size: u32,
) -> Result<Vec<Thumbnail>, ErrorPayload> {
    let password = state.password(&path);
    let cached = state
        .unlocked
        .lock()
        .ok()
        .and_then(|u| u.get(&path).cloned());
    let (data, decrypted) = tauri::async_runtime::spawn_blocking(move || {
        if !konvert_core::pdf::is_pdf(&path) {
            return Err(CoreError::NotMedia);
        }
        // A PDF opened with a password is decrypted once, in memory.
        let data = match (cached, &password) {
            (Some(data), _) => return Ok((data, None)),
            (None, Some(password)) => Arc::new(konvert_core::pdf::load(&path, Some(password))?),
            (None, None) => Arc::new(std::fs::read(&path)?),
        };
        Ok((data.clone(), password.is_some().then_some((path, data))))
    })
    .await
    .map_err(io_payload)?
    .map_err(payload)?;
    if let Some((path, data)) = decrypted
        && state.password(&path).is_some()
        && let Ok(mut unlocked) = state.unlocked.lock()
    {
        unlocked.insert(path, data);
    }
    tauri::async_runtime::spawn_blocking(move || {
        konvert_core::pdf::thumbnails(data.as_ref().clone(), &pages, size)
    })
    .await
    .map_err(io_payload)?
    .map_err(payload)
}

/// A small picture of a photo, shown as its page while arranging the pages of
/// a PDF: a PNG `data:` URL. Made by the bundled engine in the private
/// workspace; nothing leaves the computer.
#[tauri::command]
pub async fn picture_thumbnail(
    state: State<'_, AppState>,
    path: PathBuf,
    size: u32,
) -> Result<String, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?;
    let media = konvert_core::probe(bins, &path).await.map_err(payload)?;
    let id = state.thumbnails.fetch_add(1, Ordering::Relaxed);
    konvert_core::picture_thumbnail(bins, &state.workspace, id, &path, &media, size)
        .await
        .map_err(payload)
}

/// A frame of a video at `at_secs`, as a PNG `data:` URL (the ends of a trim).
#[tauri::command]
pub async fn video_frame(
    state: State<'_, AppState>,
    path: PathBuf,
    at_secs: f64,
    size: u32,
) -> Result<String, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?;
    let media = konvert_core::probe(bins, &path).await.map_err(payload)?;
    let id = state.thumbnails.fetch_add(1, Ordering::Relaxed);
    konvert_core::video_frame(bins, &state.workspace, id, &path, &media, at_secs, size)
        .await
        .map_err(payload)
}

/// The waveform of a file's audio, as a PNG `data:` URL (the bar of a trim).
#[tauri::command]
pub async fn audio_waveform(
    state: State<'_, AppState>,
    path: PathBuf,
    width: u32,
    height: u32,
) -> Result<String, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?;
    let media = konvert_core::probe(bins, &path).await.map_err(payload)?;
    let id = state.thumbnails.fetch_add(1, Ordering::Relaxed);
    konvert_core::audio_waveform(bins, &state.workspace, id, &path, &media, width, height)
        .await
        .map_err(payload)
}

/// Queues a conversion; progress arrives through `job-update` events.
/// The destination comes from the preferences (default: next to the original).
#[tauri::command]
pub fn start_conversion(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
    request: ConversionRequest,
) -> Result<u64, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?.clone();
    if !path.is_file() {
        return Err(payload(CoreError::InputNotFound));
    }
    let output_dir = output_dir(&state, &path)?;
    let passwords = state.passwords_of([path.as_path()]);
    Ok(state.jobs.enqueue(
        app,
        bins,
        state.workspace.clone(),
        path,
        request,
        output_dir,
        passwords,
    ))
}

/// Queues several files that become one PDF, in the order given: pictures
/// become pages, PDFs add all their pages. The PDF is named after the first file.
#[tauri::command]
pub fn start_combined(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<PathBuf>,
    request: ConversionRequest,
) -> Result<u64, ErrorPayload> {
    let bins = state.binaries().map_err(payload)?.clone();
    let Some(first) = paths.first() else {
        return Err(payload(CoreError::InputNotFound));
    };
    if paths.iter().any(|p| !p.is_file()) {
        return Err(payload(CoreError::InputNotFound));
    }
    if request.target != TargetFormat::Pdf {
        return Err(payload(CoreError::UnsupportedConversion {
            details: "only a PDF can combine files".into(),
        }));
    }
    let output_dir = output_dir(&state, first)?;
    let passwords = state.passwords_of(paths.iter().map(PathBuf::as_path));
    Ok(state.jobs.enqueue_combined(
        app,
        bins,
        state.workspace.clone(),
        paths,
        request,
        output_dir,
        passwords,
    ))
}

/// Destination from the preferences, otherwise the folder of `input`.
fn output_dir(state: &AppState, input: &Path) -> Result<PathBuf, ErrorPayload> {
    let output_dir = state
        .prefs
        .get()
        .output_dir
        .or_else(|| input.parent().map(PathBuf::from))
        .ok_or_else(|| payload(CoreError::OutputUnavailable))?;
    if !output_dir.is_dir() {
        return Err(payload(CoreError::OutputUnavailable));
    }
    Ok(output_dir)
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
        .map_err(io_payload)
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
    app.opener().reveal_item_in_dir(path).map_err(io_payload)
}

/// Files Konvert was launched with ("Open with"); returned once.
#[tauri::command]
pub fn take_launch_files(files: State<'_, LaunchFiles>) -> Vec<PathBuf> {
    files.take()
}

/// Opens the bundled third-party licenses with the default text editor.
#[tauri::command]
pub fn open_licenses(app: AppHandle) -> Result<(), ErrorPayload> {
    let path = app
        .path()
        .resource_dir()
        .map_err(io_payload)?
        .join("THIRD_PARTY_LICENSES.txt");
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(io_payload)
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
        .add_filter(filter_name, &picker_extensions())
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

/// Native dialog choosing a PNG or JPEG picture (a watermark). Returns
/// `None` when cancelled.
#[tauri::command]
pub async fn pick_picture(app: AppHandle, title: String, filter_name: String) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .add_filter(filter_name, &["png", "jpg", "jpeg"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    rx.await.ok().flatten()?.into_path().ok()
}

/// Opens the native picker for the audio added to a video (any audio or
/// video file whose audio is used); `None` if cancelled.
#[tauri::command]
pub async fn pick_audio_file(
    app: AppHandle,
    title: String,
    filter_name: String,
) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .add_filter(filter_name, AUDIO_SOURCE_EXTENSIONS)
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    rx.await.ok().flatten()?.into_path().ok()
}

/// Files whose audio can be added to a video.
const AUDIO_SOURCE_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "wma", "aif", "aiff", "ac3", "eac3",
    "mka", "caf", "mp2", "mp4", "mov", "mkv", "webm", "avi",
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{LIBJXL_EXTENSIONS, MEDIA_EXTENSIONS};

    /// Extensions a platform registers: libjxl is not in the macOS engine.
    fn expected(platform: &str) -> BTreeSet<String> {
        let libjxl = if platform == "macos" {
            &[][..]
        } else {
            LIBJXL_EXTENSIONS
        };
        MEDIA_EXTENSIONS
            .iter()
            .chain(libjxl)
            .map(|e| e.to_string())
            .collect()
    }

    #[test]
    fn open_with_covers_every_file_picker_extension() {
        let hooks = include_str!("../windows/installer-hooks.nsh");
        let registered: BTreeSet<String> = hooks
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix("!insertmacro ${OP} \"")?
                    .strip_suffix('"')
            })
            .map(str::to_owned)
            .collect();
        assert_eq!(registered, expected("windows"));
    }

    /// A command works only if it is handled (`lib.rs`), listed (`build.rs`)
    /// and allowed (`capabilities/default.json`): the three lists must match.
    #[test]
    fn every_command_is_handled_listed_and_allowed() {
        let handled: BTreeSet<String> = include_str!("lib.rs")
            .lines()
            .filter_map(|l| l.trim().strip_prefix("commands::")?.strip_suffix(','))
            .map(str::to_owned)
            .collect();
        let build = include_str!("../build.rs");
        let list = &build[build.find("const COMMANDS").unwrap()..];
        let listed: BTreeSet<String> = list[..list.find("];").unwrap()]
            .lines()
            .filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\","))
            .map(str::to_owned)
            .collect();
        let capabilities: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let allowed: BTreeSet<String> = capabilities["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str()?.strip_prefix("allow-"))
            .map(|p| p.replace('-', "_"))
            .collect();
        assert!(handled.contains("pdf_thumbnails"), "{handled:?}");
        assert_eq!(handled, listed);
        assert_eq!(listed, allowed);
    }

    #[test]
    fn macos_and_linux_open_with_cover_every_file_picker_extension() {
        for (platform, conf) in [
            ("macos", include_str!("../tauri.macos.conf.json")),
            ("linux", include_str!("../tauri.linux.conf.json")),
        ] {
            let conf: serde_json::Value = serde_json::from_str(conf).unwrap();
            let associations = conf["bundle"]["fileAssociations"].as_array().unwrap();
            let registered: BTreeSet<String> = associations
                .iter()
                .flat_map(|a| a["ext"].as_array().unwrap())
                .map(|e| e.as_str().unwrap().to_owned())
                .collect();
            assert_eq!(registered, expected(platform), "{platform}");
            for a in associations {
                match platform {
                    // Listed in "Open With", never the default app.
                    "macos" => assert_eq!(a["rank"], "Alternate", "{a}"),
                    _ => assert!(
                        a["mimeType"].as_str().is_some_and(|m| m.contains('/')),
                        "{a}"
                    ),
                }
            }
        }
    }
}
