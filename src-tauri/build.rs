use std::path::Path;

/// Commands the frontend may invoke. Each one must also be granted in
/// `capabilities/default.json` (as `allow-<command-name>`), otherwise Tauri rejects it.
const COMMANDS: &[&str] = &[
    "engine_info",
    "platform_info",
    "app_ready",
    "analyze_file",
    "plan_conversion",
    "start_conversion",
    "cancel_conversion",
    "open_output",
    "reveal_output",
    "pick_files",
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
