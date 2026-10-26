//! Files opened with Konvert ("Open with Konvert", drag onto the icon) and
//! startup failures.
//!
//! Windows and Linux pass the files on the command line (to the second instance
//! too, through the single-instance plugin); macOS sends them as an "open
//! documents" event instead, possibly before the window exists.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

/// Event carrying files opened while Konvert is already running.
pub const OPEN_FILES_EVENT: &str = "open-files";

/// Files opened before the UI asked for them (managed state). `None` once the
/// UI has taken them: later files are delivered with [`OPEN_FILES_EVENT`].
pub struct LaunchFiles(Mutex<Option<Vec<PathBuf>>>);

impl Default for LaunchFiles {
    fn default() -> Self {
        Self(Mutex::new(Some(Vec::new())))
    }
}

impl LaunchFiles {
    /// Hands the queued files to the UI, once.
    pub fn take(&self) -> Vec<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .unwrap_or_default()
    }
}

/// Queues `files` until the UI is ready, then delivers them with an event.
pub fn open_files(app: &AppHandle, files: Vec<PathBuf>) {
    if files.is_empty() {
        return;
    }
    let pending = app.state::<LaunchFiles>();
    let mut queue = pending.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(queue) = queue.as_mut() {
        queue.extend(files);
    } else {
        drop(queue);
        let _ = app.emit(OPEN_FILES_EVENT, files);
    }
}

/// Existing files among `args` (program name excluded); relative paths are
/// resolved against `cwd`. Options such as `--foo` are ignored.
pub fn files_from_args(args: impl IntoIterator<Item = OsString>, cwd: &Path) -> Vec<PathBuf> {
    args.into_iter()
        .skip(1)
        .filter(|a| !a.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { cwd.join(p) })
        .filter(|p| p.is_file())
        .collect()
}

/// Shown when the window cannot be created at all: on Windows typically because
/// the Microsoft Edge WebView2 runtime is missing. Exits the process.
pub fn fatal_startup_error(error: &str) -> ! {
    let message = if cfg!(windows) {
        format!(
            "Konvert non può avviarsi: probabilmente manca Microsoft Edge WebView2 Runtime. \
             Installalo dal sito Microsoft e riprova.\n\n\
             Konvert cannot start: the Microsoft Edge WebView2 Runtime is probably missing. \
             Install it from Microsoft's website and try again.\n\n({error})"
        )
    } else {
        format!("Konvert non può avviarsi. / Konvert cannot start.\n\n({error})")
    };
    show_error_box(&message);
    std::process::exit(1);
}

#[cfg(windows)]
fn show_error_box(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(message), wide("Konvert"));
    // SAFETY: both buffers are NUL-terminated UTF-16 strings; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_error_box(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_existing_files_only_and_resolves_relative_paths() {
        let dir = std::env::temp_dir().join(format!("konvert-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("brano con spazi è.wav");
        std::fs::write(&file, b"x").unwrap();

        let args = [
            "konvert.exe",
            "--flag",
            "brano con spazi è.wav",
            "missing.mp3",
        ]
        .map(OsString::from);
        assert_eq!(files_from_args(args, &dir), std::slice::from_ref(&file));

        let absolute = [OsString::from("konvert.exe"), file.clone().into_os_string()];
        assert_eq!(
            files_from_args(absolute, Path::new("C:\\elsewhere")),
            [file]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
