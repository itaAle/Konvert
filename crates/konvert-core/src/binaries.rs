//! Location of the bundled FFmpeg/FFprobe executables and OCR models.
//!
//! They are shipped inside the application bundle (never downloaded at runtime).
//! Callers pass the directory that contains the executables: the Tauri app resolves
//! it from its resource directory, the CLI from a flag or an environment variable.
//! The OCR models are in the `ocr` folder next to it.

use std::path::{Path, PathBuf};

use crate::error::CoreError;

/// Absolute paths of the two engine executables and of the OCR models.
#[derive(Debug, Clone)]
pub struct Binaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    /// Folder of the OCR models (see [`crate::ocr::Ocr::load`]).
    pub ocr: PathBuf,
}

impl Binaries {
    /// Looks for `ffmpeg` and `ffprobe` (with the platform executable suffix) in `dir`;
    /// the OCR models are expected in the `ocr` folder next to `dir`.
    pub fn from_dir(dir: &Path) -> Result<Self, CoreError> {
        let ffmpeg = dir.join(executable_name("ffmpeg"));
        let ffprobe = dir.join(executable_name("ffprobe"));
        let ocr = dir.parent().unwrap_or(dir).join("ocr");
        if ffmpeg.is_file() && ffprobe.is_file() {
            Ok(Self {
                ffmpeg,
                ffprobe,
                ocr,
            })
        } else {
            Err(CoreError::EngineMissing)
        }
    }
}

/// `ffmpeg` → `ffmpeg.exe` on Windows, unchanged elsewhere.
pub fn executable_name(base: &str) -> String {
    format!("{base}{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binaries_are_reported() {
        let dir = std::env::temp_dir().join("konvert-test-no-binaries-here");
        assert!(matches!(
            Binaries::from_dir(&dir),
            Err(CoreError::EngineMissing)
        ));
    }

    #[test]
    fn executable_name_uses_platform_suffix() {
        let name = executable_name("ffmpeg");
        if cfg!(windows) {
            assert_eq!(name, "ffmpeg.exe");
        } else {
            assert_eq!(name, "ffmpeg");
        }
    }
}
