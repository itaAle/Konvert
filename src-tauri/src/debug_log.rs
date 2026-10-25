//! Optional local diagnostic log ("debug mode", off by default).
//!
//! When enabled, Konvert appends technical events to `konvert.log` in the app
//! log directory. The log never leaves the computer and never contains full
//! paths or file names: every known path is replaced with a placeholder by a
//! [`Redactor`] before writing.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FILE_NAME: &str = "konvert.log";

pub struct DebugLog {
    path: PathBuf,
    enabled: AtomicBool,
    file: Mutex<Option<File>>,
}

impl DebugLog {
    pub fn new(dir: &Path, enabled: bool) -> Self {
        Self {
            path: dir.join(FILE_NAME),
            enabled: AtomicBool::new(enabled),
            file: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            // Close the file so it can be deleted or moved.
            *self.file.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Appends one line (already redacted) if debug mode is on.
    pub fn write(&self, message: &str) {
        if !self.is_enabled() {
            return;
        }
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            if let Some(dir) = self.path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            *guard = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(file) = guard.as_mut() {
            for line in message.lines() {
                let _ = writeln!(file, "{} {line}", utc_timestamp(SystemTime::now()));
            }
        }
    }

    /// Deletes the log file.
    pub fn clear(&self) -> io::Result<()> {
        *self.file.lock().unwrap_or_else(|e| e.into_inner()) = None;
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Replaces paths and file names with placeholders.
#[derive(Default)]
pub struct Redactor {
    replacements: Vec<(String, String)>,
}

impl Redactor {
    pub fn new() -> Self {
        let mut r = Self::default();
        // Fallback for any other path under the user's profile.
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            r.push(home.to_string_lossy().into_owned(), "~".to_owned());
        }
        r
    }

    /// Redacts `path`, its file name and its stem as `<label>`.
    pub fn path(mut self, path: &Path, label: &str) -> Self {
        let placeholder = format!("<{label}>");
        self.push(path.to_string_lossy().into_owned(), placeholder.clone());
        if let Some(name) = path.file_name() {
            self.push(name.to_string_lossy().into_owned(), placeholder.clone());
        }
        if let Some(stem) = path.file_stem() {
            self.push(stem.to_string_lossy().into_owned(), placeholder);
        }
        self
    }

    fn push(&mut self, secret: String, placeholder: String) {
        // Very short names (e.g. "a") would garble unrelated text.
        if secret.chars().count() >= 3 && !self.replacements.iter().any(|(s, _)| *s == secret) {
            self.replacements.push((secret, placeholder));
            // Longest first, so a full path wins over its file name.
            self.replacements
                .sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
        }
    }

    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (secret, placeholder) in &self.replacements {
            out = out.replace(secret.as_str(), placeholder);
        }
        out
    }
}

/// `2026-10-25 21:33:07Z` without any date library.
fn utc_timestamp(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01 00:00:00Z");
        let t = UNIX_EPOCH + Duration::from_secs(1_792_964_000);
        assert_eq!(utc_timestamp(t), "2026-10-25 21:33:20Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(utc_timestamp(leap), "2000-02-29 00:00:00Z");
    }

    #[test]
    fn redactor_removes_paths_names_and_stems() {
        let input = Path::new(r"C:\Musica\Mia canzone segreta.wav");
        let r = Redactor::default()
            .path(input, "input")
            .path(Path::new(r"D:\Uscita"), "output-dir");
        let stderr = r"file:C:\Musica\Mia canzone segreta.wav: Invalid data; title=Mia canzone segreta; D:\Uscita\x";
        assert_eq!(
            r.apply(stderr),
            r"file:<input>: Invalid data; title=<input>; <output-dir>\x"
        );
    }

    #[test]
    fn log_is_written_only_when_enabled() {
        let dir = std::env::temp_dir().join(format!("konvert-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let log = DebugLog::new(&dir, false);
        log.write("hidden");
        assert!(!log.path().exists());

        log.set_enabled(true);
        log.write("first\nsecond");
        let text = fs::read_to_string(log.path()).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(
            text.lines()
                .all(|l| l.ends_with("first") || l.ends_with("second"))
        );

        log.clear().unwrap();
        assert!(!log.path().exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
