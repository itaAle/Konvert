//! Application state shared by the IPC commands.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use konvert_core::{Binaries, CoreError, Workspace, workspace};
use tauri::{AppHandle, Manager};

use crate::debug_log::DebugLog;
use crate::jobs::JobQueue;
use crate::prefs::PrefsStore;

/// First number of the private files of picture thumbnails, far from the
/// numbers of conversion jobs (which start at 1).
const THUMBNAIL_IDS: u64 = 1 << 48;

pub struct AppState {
    /// `Err` when the bundled engine is missing: the app still starts and the UI
    /// shows a clear error instead of crashing.
    pub binaries: Result<Binaries, String>,
    /// Private temporary directory; removed on exit, cleaned at next start after a crash.
    pub workspace: Arc<Workspace>,
    pub jobs: Arc<JobQueue>,
    pub prefs: PrefsStore,
    pub log: Arc<DebugLog>,
    /// Mica backdrop applied (Windows 11): the UI can use translucent surfaces.
    pub vibrancy: bool,
    /// The user confirmed closing the window while conversions were running.
    pub close_confirmed: AtomicBool,
    /// Encoders of the bundled FFmpeg, read once (some formats depend on them).
    pub encoders: tokio::sync::OnceCell<HashSet<String>>,
    /// Numbers of the private files of picture thumbnails (apart from jobs).
    pub thumbnails: AtomicU64,
}

impl AppState {
    pub fn initialize(app: &AppHandle, vibrancy: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let paths = app.path();
        // FFmpeg/FFprobe are bundled as resources under `ffmpeg/` (see tauri.conf.json).
        let binaries = paths
            .resource_dir()
            .map_err(|e| e.to_string())
            .and_then(|dir| Binaries::from_dir(&dir.join("ffmpeg")).map_err(|e| e.to_string()));
        let workspace = Arc::new(Workspace::create(&workspace::default_base_dir())?);
        let prefs = PrefsStore::load(&paths.app_config_dir()?);
        let log = Arc::new(DebugLog::new(
            &paths.app_log_dir()?,
            prefs.get().debug_logging,
        ));
        log.write(&format!(
            "Konvert {} started on {} {} (Mica: {vibrancy})",
            app.package_info().version,
            std::env::consts::OS,
            std::env::consts::ARCH,
        ));
        Ok(Self {
            binaries,
            workspace,
            jobs: Arc::new(JobQueue::new(log.clone())),
            prefs,
            log,
            vibrancy,
            close_confirmed: AtomicBool::new(false),
            encoders: tokio::sync::OnceCell::new(),
            thumbnails: AtomicU64::new(THUMBNAIL_IDS),
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
