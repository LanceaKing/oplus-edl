//! Publish a completed same-directory partial file without replacing a racer.
use std::{io, path::Path};

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes())?;
    let destination = CString::new(destination.as_os_str().as_bytes())?;
    // SAFETY: both strings are NUL-terminated and live throughout the call.
    // The kernel performs the existence check and rename as one operation.
    #[cfg(target_os = "macos")]
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
pub(crate) fn no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        // Absolute paths also avoid MoveFileEx's relative-path interpretation.
        let path = std::path::absolute(path)?;
        let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"));
        }
        value.push(0);
        Ok(value)
    }
    let source = wide(source)?;
    let destination = wide(destination)?;
    // SAFETY: buffers are valid NUL-terminated UTF-16 for the duration of the call.
    // Zero flags forbid both replacement and cross-volume copy fallback.
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            0,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) fn no_replace(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "exclusive rename unavailable on this platform",
    ))
}
