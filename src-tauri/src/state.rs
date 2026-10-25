//! Application state shared by the IPC commands.

use konvert_core::{Binaries, CoreError, Workspace, workspace};
use tauri::{AppHandle, Manager};

pub struct AppState {
    /// `Err` when the bundled engine is missing: the app still starts and the UI
    /// shows a clear error instead of crashing.
    pub binaries: Result<Binaries, String>,
    /// Private temporary directory; removed on exit, cleaned at next start after a crash.
    #[allow(dead_code)] // used by the conversion commands added in the next milestone
    pub workspace: Workspace,
}

impl AppState {
    pub fn initialize(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        // FFmpeg/FFprobe are bundled as resources under `ffmpeg/` (see tauri.conf.json).
        let binaries = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())
            .and_then(|dir| Binaries::from_dir(&dir.join("ffmpeg")).map_err(|e| e.to_string()));
        let workspace = Workspace::create(&workspace::default_base_dir())?;
        Ok(Self {
            binaries,
            workspace,
        })
    }

    pub fn binaries(&self) -> Result<&Binaries, CoreError> {
        self.binaries.as_ref().map_err(|_| CoreError::EngineMissing)
    }
}
