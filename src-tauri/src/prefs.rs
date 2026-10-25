//! User preferences: the only data Konvert keeps between runs.
//!
//! Stored as human-readable JSON in the app config directory (on Windows
//! `%APPDATA%\app.konvert.desktop\preferences.json`). No history, no file names
//! (except the destination folder the user explicitly chose), no identifiers.
//! The installer leaves this folder untouched on updates.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use konvert_core::QualityPreset;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const FILE_NAME: &str = "preferences.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LanguagePref {
    /// Follow the system language.
    #[default]
    Auto,
    It,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ThemePref {
    /// Follow the system light/dark setting.
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Preferences {
    pub language: LanguagePref,
    pub theme: ThemePref,
    /// Preset selected for new conversions.
    pub default_preset: QualityPreset,
    /// Destination folder; `None` saves next to the original file.
    pub output_dir: Option<PathBuf>,
    /// Write a local diagnostic log (off by default).
    pub debug_logging: bool,
}

/// Loads and saves the preferences file.
pub struct PrefsStore {
    path: PathBuf,
    current: Mutex<Preferences>,
}

impl PrefsStore {
    /// Reads `dir/preferences.json`; a missing or unreadable file gives the defaults.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let current = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Preferences {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Saves atomically (write a temporary file, then rename over the old one).
    pub fn set(&self, prefs: Preferences) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(&prefs).map_err(io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, &self.path)?;
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = prefs;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("konvert-prefs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn missing_file_gives_defaults() {
        let d = dir("missing");
        assert_eq!(PrefsStore::load(&d).get(), Preferences::default());
    }

    #[test]
    fn saves_readable_json_and_reloads_it() {
        let d = dir("roundtrip");
        let store = PrefsStore::load(&d);
        let prefs = Preferences {
            language: LanguagePref::En,
            theme: ThemePref::Dark,
            default_preset: QualityPreset::Small,
            output_dir: Some(PathBuf::from("D:\\Musica convertita")),
            debug_logging: true,
        };
        store.set(prefs.clone()).unwrap();
        let text = fs::read_to_string(d.join(FILE_NAME)).unwrap();
        assert!(text.contains("\"theme\": \"dark\""), "{text}");
        assert_eq!(PrefsStore::load(&d).get(), prefs);
        assert!(!d.join("preferences.json.tmp").exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn corrupt_or_partial_files_fall_back_to_defaults() {
        let d = dir("corrupt");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(FILE_NAME), b"{ not json").unwrap();
        assert_eq!(PrefsStore::load(&d).get(), Preferences::default());

        // Older/newer versions: missing keys get defaults, unknown keys are ignored.
        fs::write(
            d.join(FILE_NAME),
            br#"{ "theme": "light", "futureOption": 1 }"#,
        )
        .unwrap();
        let prefs = PrefsStore::load(&d).get();
        assert_eq!(prefs.theme, ThemePref::Light);
        assert_eq!(prefs.default_preset, QualityPreset::Balanced);
        fs::remove_dir_all(&d).unwrap();
    }
}
