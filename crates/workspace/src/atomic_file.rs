//! Replacing a file all or nothing, for Vault files and the app config alike.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes a temporary file next to `path`, flushes it to disk, then renames it over `path`, so
/// a crash mid-write never leaves a half-written file. A symlinked file is replaced where the
/// link points, and the link is kept.
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_if(path, bytes, |_| Ok(true)).map(|_| ())
}

/// [`replace`], asking `unchanged` just before the rename whether the file is still as the
/// caller last saw it. If it isn't, the file is left alone and `false` returned.
pub(crate) fn replace_if(
    path: &Path,
    bytes: &[u8],
    unchanged: impl FnOnce(&Path) -> io::Result<bool>,
) -> io::Result<bool> {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let temp = temp_path(&path);
    let result = write_and_rename(&temp, &path, bytes, unchanged);
    if !matches!(result, Ok(true)) {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_and_rename(
    temp: &Path,
    path: &Path,
    bytes: &[u8],
    unchanged: impl FnOnce(&Path) -> io::Result<bool>,
) -> io::Result<bool> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    // Only Unix modes are copied: on Windows a read-only file would make the temporary file
    // read-only too, and replacing a read-only file fails there anyway.
    #[cfg(unix)]
    if let Ok(existing) = fs::metadata(path) {
        file.set_permissions(existing.permissions())?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if !unchanged(path)? {
        return Ok(false);
    }
    fs::rename(temp, path)?;
    sync_parent(path);
    Ok(true)
}

/// A hidden, unique name in the file's directory, so the rename stays on one filesystem.
fn temp_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = format!(
        ".{name}.{}-{}.keefast-tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    path.with_file_name(unique)
}

/// Makes the rename itself durable. Best effort: not every platform can sync a directory.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        if let Ok(dir) = fs::File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}
