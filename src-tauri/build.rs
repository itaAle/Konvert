use std::path::Path;

/// Commands the frontend may invoke. Each one must also be granted in
/// `capabilities/default.json` (as `allow-<command-name>`), otherwise Tauri rejects it.
const COMMANDS: &[&str] = &[
    "engine_info",
    "platform_info",
    "app_ready",
    "get_preferences",
    "set_preferences",
    "pick_folder",
    "available_space",
    "open_releases_page",
    "reveal_log",
    "clear_log",
    "confirm_close",
    "analyze_file",
    "unlock_pdf",
    "forget_pdf_password",
    "plan_conversion",
    "pdf_thumbnails",
    "picture_thumbnail",
    "video_frame",
    "pick_audio_file",
    "audio_waveform",
    "start_conversion",
    "start_combined",
    "cancel_conversion",
    "open_output",
    "reveal_output",
    "pick_files",
    "pick_picture",
    "take_launch_files",
    "open_licenses",
];

fn main() {
    // The FFmpeg binaries are bundled as resources; fail early with a clear hint
    // instead of Tauri's generic "resource path doesn't exist" error.
    let ffmpeg = Path::new("binaries/ffmpeg").join(format!("ffmpeg{}", exe_suffix()));
    println!("cargo:rerun-if-changed={}", ffmpeg.display());
    if !ffmpeg.is_file() {
        panic!(
            "\n\nFFmpeg binaries not found in src-tauri/binaries/ffmpeg.\n\
             Run the setup script first:\n  \
             Windows:      powershell -ExecutionPolicy Bypass -File scripts/setup-ffmpeg.ps1\n  \
             Linux/macOS:  bash scripts/setup-ffmpeg.sh\n\n"
        );
    }

    // Same for the OCR models (scripts/setup-ocr.mjs).
    let ocr = Path::new("binaries/ocr/rec.onnx");
    println!("cargo:rerun-if-changed={}", ocr.display());
    if !ocr.is_file() {
        panic!(
            "

OCR models not found in src-tauri/binaries/ocr.
             Run the setup script first:  npm run setup:ocr

"
        );
    }

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}

/// Executable suffix of the *target* platform (build scripts run on the host).
fn exe_suffix() -> &'static str {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        ".exe"
    } else {
        ""
    }
}
