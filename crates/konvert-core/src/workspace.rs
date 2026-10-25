//! Dedicated temporary directory for in-progress outputs.
//!
//! FFmpeg writes into `<temp>/Konvert/run-<pid>-<nanos>/` and the finished file is
//! moved to its destination only on success, so cancelled or failed jobs never
//! leave partial files in the user's folders.
//!
//! Each running instance holds an exclusive lock on `.lock` inside its own run
//! directory. At start-up, run directories whose lock can be acquired belong to
//! instances that are gone (e.g. crashed) and are deleted. This needs no record
//! of file names or paths anywhere.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const RUN_PREFIX: &str = "run-";
const LOCK_FILE: &str = ".lock";

/// `<system temp>/Konvert`.
pub fn default_base_dir() -> PathBuf {
    std::env::temp_dir().join("Konvert")
}

#[derive(Debug)]
pub struct Workspace {
    dir: PathBuf,
    lock: Option<File>,
}

impl Workspace {
    /// Removes stale run directories under `base`, then creates and locks a new one.
    pub fn create(base: &Path) -> io::Result<Self> {
        fs::create_dir_all(base)?;
        cleanup_stale(base);

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = base.join(format!("{RUN_PREFIX}{}-{nanos}", std::process::id()));
        fs::create_dir(&dir)?;
        let lock = File::create(dir.join(LOCK_FILE))?;
        lock.try_lock()
            .map_err(|_| io::Error::other("cannot lock the temporary directory"))?;
        Ok(Self {
            dir,
            lock: Some(lock),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Temporary output path for a job (the extension lets FFmpeg pick the muxer).
    pub fn job_output(&self, job_id: u64, ext: &str) -> PathBuf {
        self.dir.join(format!("job-{job_id}.{ext}"))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        // Release the lock before deleting: Windows cannot delete an open file.
        drop(self.lock.take());
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Deletes run directories that are no longer locked by a live instance.
fn cleanup_stale(base: &Path) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_run_dir = entry.file_name().to_string_lossy().starts_with(RUN_PREFIX)
            && entry.file_type().is_ok_and(|t| t.is_dir());
        if !is_run_dir {
            continue;
        }
        let stale = match File::options().write(true).open(path.join(LOCK_FILE)) {
            Ok(lock) => {
                let free = lock.try_lock().is_ok();
                drop(lock);
                free
            }
            // No lock file at all: the instance died while creating the directory.
            Err(e) if e.kind() == io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        if stale {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_base(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("konvert-ws-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        base
    }

    #[test]
    fn workspace_is_removed_on_drop() {
        let base = test_base("drop");
        let ws = Workspace::create(&base).unwrap();
        let dir = ws.dir().to_path_buf();
        fs::write(ws.job_output(1, "mp3"), b"partial").unwrap();
        assert!(dir.is_dir());
        drop(ws);
        assert!(!dir.exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn stale_directories_are_cleaned_but_live_ones_are_kept() {
        let base = test_base("stale");
        // A crashed instance: run directory with an unlocked lock file.
        let stale = base.join("run-1-1");
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join(LOCK_FILE), b"").unwrap();
        fs::write(stale.join("job-1.mp3"), b"partial").unwrap();
        // Unrelated content must be left alone.
        let other = base.join("not-ours");
        fs::create_dir_all(&other).unwrap();

        let live = Workspace::create(&base).unwrap();
        assert!(!stale.exists());
        assert!(other.exists());

        // A second instance must not delete the first one's directory.
        let second = Workspace::create(&base).unwrap();
        assert!(live.dir().is_dir());

        drop(second);
        drop(live);
        fs::remove_dir_all(&base).unwrap();
    }
}
