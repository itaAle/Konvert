//! Output file naming and the final "move into place" step.
//!
//! Konvert never overwrites an existing file: if `song.mp3` exists the output
//! becomes `song (1).mp3`, then `song (2).mp3`, and so on. The final move uses
//! an OS primitive that fails instead of replacing, so even a file created by
//! another program between the check and the move is never overwritten.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// Upper bound for the `(n)` suffix, to avoid looping forever on odd file systems.
const MAX_SUFFIX: u32 = 10_000;

/// Candidate file name number `n`: `stem.ext` for 0, `stem (n).ext` otherwise.
pub fn candidate_name(stem: &OsStr, ext: &str, n: u32) -> OsString {
    let mut name = stem.to_os_string();
    if n > 0 {
        name.push(format!(" ({n})"));
    }
    if !ext.is_empty() {
        name.push(".");
        name.push(ext);
    }
    name
}

/// First path `dir/stem[ (n)].ext` for which `exists` returns `false`.
pub fn unique_path(
    dir: &Path,
    stem: &OsStr,
    ext: &str,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    (0..=MAX_SUFFIX)
        .map(|n| dir.join(candidate_name(stem, ext, n)))
        .find(|p| !exists(p))
}

/// Moves `src` to the first free `dir/stem[ (n)].ext` and returns the final path.
pub fn move_to_unique(src: &Path, dir: &Path, stem: &OsStr, ext: &str) -> io::Result<PathBuf> {
    for n in 0..=MAX_SUFFIX {
        let dst = dir.join(candidate_name(stem, ext, n));
        if dst.exists() {
            continue;
        }
        match move_no_replace(src, &dst) {
            Ok(()) => return Ok(dst),
            // Created by someone else in the meantime: try the next name.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free output file name",
    ))
}

/// Creates the first free folder `dir/stem[ (n)]` and returns its path.
pub fn create_unique_dir(dir: &Path, stem: &OsStr) -> io::Result<PathBuf> {
    for n in 0..=MAX_SUFFIX {
        let candidate = dir.join(candidate_name(stem, "", n));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            // Taken (by a file or a folder): try the next name.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free output folder name",
    ))
}

/// Moves the `.ext` files of `src_dir`, in name order, into a new folder
/// `dir/stem[ (n)]` as `stem-001.ext`, `stem-002.ext`, ... Returns the folder,
/// the number of files and their total size. If a move fails, the new folder
/// is removed again (it only holds files moved by this call).
pub fn move_series(
    src_dir: &Path,
    dir: &Path,
    stem: &OsStr,
    ext: &str,
) -> io::Result<(PathBuf, u32, u64)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(src_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == ext))
        .collect();
    files.sort();
    let folder = create_unique_dir(dir, stem)?;
    let digits = files.len().to_string().len().max(3);
    let mut bytes = 0;
    for (i, file) in files.iter().enumerate() {
        let mut name = stem.to_os_string();
        name.push(format!("-{:0digits$}.{ext}", i + 1));
        let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        if let Err(e) = move_no_replace(file, &folder.join(name)) {
            let _ = std::fs::remove_dir_all(&folder);
            return Err(e);
        }
        bytes += size;
    }
    Ok((folder, files.len() as u32, bytes))
}

/// Moves a file, failing with `AlreadyExists` instead of replacing `dst`.
/// Works across volumes (falls back to copy + delete).
#[cfg(windows)]
pub fn move_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_COPY_ALLOWED, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    fn wide(p: &Path) -> Vec<u16> {
        verbatim(p).encode_wide().chain(Some(0)).collect()
    }

    let (src_w, dst_w) = (wide(src), wide(dst));
    // Without MOVEFILE_REPLACE_EXISTING the call fails if `dst` exists.
    // SAFETY: both buffers are valid, NUL-terminated UTF-16 strings.
    let ok = unsafe {
        MoveFileExW(
            src_w.as_ptr(),
            dst_w.as_ptr(),
            MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Adds the `\\?\` prefix to long absolute paths so Win32 calls accept them.
#[cfg(windows)]
fn verbatim(p: &Path) -> OsString {
    let s = p.as_os_str();
    let text = s.to_string_lossy();
    if text.len() < 248 || text.starts_with(r"\\?\") || !p.is_absolute() {
        return s.to_os_string();
    }
    let mut out = OsString::new();
    if let Some(unc) = text.strip_prefix(r"\\") {
        out.push(r"\\?\UNC\");
        out.push(unc);
    } else {
        out.push(r"\\?\");
        out.push(s);
    }
    out
}

/// Moves a file, failing with `AlreadyExists` instead of replacing `dst`.
/// Works across file systems and on file systems without hard links (exFAT/FAT
/// USB drives, some network shares) by falling back to copy + delete.
#[cfg(not(windows))]
pub fn move_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    // `link` fails if `dst` exists, which gives no-replace semantics.
    match std::fs::hard_link(src, dst) {
        Ok(()) => std::fs::remove_file(src),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
        // `create_new` keeps the no-replace semantics of the copy.
        Err(_) => {
            let mut from = std::fs::File::open(src)?;
            let mut to = std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(dst)?;
            if let Err(e) = io::copy(&mut from, &mut to).and_then(|_| to.sync_all()) {
                drop(to);
                let _ = std::fs::remove_file(dst);
                return Err(e);
            }
            std::fs::remove_file(src)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn first_candidate_is_the_plain_name() {
        let p = unique_path(Path::new("out"), OsStr::new("brano"), "mp3", |_| false).unwrap();
        assert_eq!(p, Path::new("out").join("brano.mp3"));
    }

    #[test]
    fn taken_names_get_a_numeric_suffix() {
        let taken: HashSet<PathBuf> = ["brano.mp3", "brano (1).mp3"]
            .iter()
            .map(|n| Path::new("out").join(n))
            .collect();
        let p = unique_path(Path::new("out"), OsStr::new("brano"), "mp3", |p| {
            taken.contains(p)
        })
        .unwrap();
        assert_eq!(p, Path::new("out").join("brano (2).mp3"));
    }

    #[test]
    fn unicode_and_spaces_are_preserved() {
        let name = candidate_name(OsStr::new("Canzone d’estate – è più bella 🎵"), "flac", 3);
        assert_eq!(
            name,
            OsStr::new("Canzone d’estate – è più bella 🎵 (3).flac")
        );
    }

    #[test]
    fn move_never_replaces_existing_files() {
        let dir = std::env::temp_dir().join(format!(
            "konvert-naming-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("cartella con spazi è.txt");
        std::fs::write(&existing, b"original").unwrap();
        let src = dir.join("src.tmp");
        std::fs::write(&src, b"new").unwrap();

        let out = move_to_unique(&src, &dir, OsStr::new("cartella con spazi è"), "txt").unwrap();

        assert_eq!(out, dir.join("cartella con spazi è (1).txt"));
        assert_eq!(std::fs::read(&existing).unwrap(), b"original");
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        assert!(!src.exists());
        assert_eq!(
            move_no_replace(&out, &existing).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_series_goes_into_a_new_numbered_folder() {
        let dir = std::env::temp_dir().join(format!(
            "konvert-naming-test-{}-{}",
            std::process::id(),
            line!()
        ));
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        for (name, data) in [
            ("frame-000002.png", &b"bb"[..]),
            ("frame-000001.png", b"a"),
            ("frame-000003.png", b"ccc"),
            ("other.txt", b"ignored"),
        ] {
            std::fs::write(work.join(name), data).unwrap();
        }
        // A file and a folder already use the plain name.
        std::fs::write(dir.join("Vacanze è"), b"").unwrap();
        std::fs::create_dir(dir.join("Vacanze è (1)")).unwrap();

        let (folder, count, bytes) =
            move_series(&work, &dir, OsStr::new("Vacanze è"), "png").unwrap();

        assert_eq!(folder, dir.join("Vacanze è (2)"));
        assert_eq!((count, bytes), (3, 6));
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "Vacanze è-001.png",
                "Vacanze è-002.png",
                "Vacanze è-003.png"
            ]
        );
        assert_eq!(
            std::fs::read(folder.join("Vacanze è-002.png")).unwrap(),
            b"bb"
        );
        assert!(work.join("other.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
