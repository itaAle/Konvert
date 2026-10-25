//! Location of the bundled FFmpeg/FFprobe executables.
//!
//! The executables are shipped inside the application bundle (never downloaded at
//! runtime). Callers pass the directory that contains them: the Tauri app resolves
//! it from its resource directory, the CLI from a flag or an environment variable.

use std::path::{Path, PathBuf};

use crate::error::CoreError;

/// Absolute paths of the two engine executables.
#[derive(Debug, Clone)]
pub struct Binaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Binaries {
    /// Looks for `ffmpeg` and `ffprobe` (with the platform executable suffix) in `dir`.
    pub fn from_dir(dir: &Path) -> Result<Self, CoreError> {
        let ffmpeg = dir.join(executable_name("ffmpeg"));
        let ffprobe = dir.join(executable_name("ffprobe"));
        if ffmpeg.is_file() && ffprobe.is_file() {
            Ok(Self { ffmpeg, ffprobe })
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
