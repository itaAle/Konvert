//! Application state shared by the IPC commands.

use std::sync::Arc;

use konvert_core::{Binaries, CoreError, Workspace, workspace};
use tauri::{AppHandle, Manager};

use crate::jobs::JobQueue;

pub struct AppState {
    /// `Err` when the bundled engine is missing: the app still starts and the UI
    /// shows a clear error instead of crashing.
    pub binaries: Result<Binaries, String>,
    /// Private temporary directory; removed on exit, cleaned at next start after a crash.
    pub workspace: Arc<Workspace>,
    pub jobs: Arc<JobQueue>,
    /// Mica backdrop applied (Windows 11): the UI can use translucent surfaces.
    pub vibrancy: bool,
}

impl AppState {
    pub fn initialize(app: &AppHandle, vibrancy: bool) -> Result<Self, Box<dyn std::error::Error>> {
        // FFmpeg/FFprobe are bundled as resources under `ffmpeg/` (see tauri.conf.json).
        let binaries = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())
            .and_then(|dir| Binaries::from_dir(&dir.join("ffmpeg")).map_err(|e| e.to_string()));
        let workspace = Arc::new(Workspace::create(&workspace::default_base_dir())?);
        Ok(Self {
            binaries,
            workspace,
            jobs: Arc::default(),
            vibrancy,
        })
    }

    pub fn binaries(&self) -> Result<&Binaries, CoreError> {
        self.binaries.as_ref().map_err(|_| CoreError::EngineMissing)
    }

    /// Stops conversions and removes temporary files (application exit).
    pub fn shutdown(&self) {
        self.jobs.cancel_all();
        self.workspace.close();
    }
}
