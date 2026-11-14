use anyhow::{Result, bail};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

/// Reject special inputs before opening; recheck the opened handle for replacement races.
pub(crate) fn open_regular(path: &Path) -> Result<File> {
    if !fs::metadata(path)?.is_file() {
        bail!("input is not a regular file: {}", path.display());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A regular file replaced by a FIFO must not block between the two checks.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        bail!("input is not a regular file: {}", path.display());
    }
    Ok(file)
}
