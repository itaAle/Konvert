//! Free disk space, used to warn before a conversion that may not fit.

use std::path::Path;

/// Bytes available to the current user on the volume containing `dir`.
#[cfg(windows)]
pub fn available_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated; unused out-parameters may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

/// Bytes available to the current user on the file system containing `dir`.
#[cfg(unix)]
pub fn available_space(dir: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is a valid C string and `stat` a properly sized buffer.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dir_has_some_space() {
        assert!(available_space(&std::env::temp_dir()).is_some_and(|b| b > 0));
    }

    #[test]
    fn missing_dir_gives_none() {
        assert_eq!(
            available_space(Path::new("Z:\\konvert\\does\\not\\exist")),
            None
        );
    }
}
