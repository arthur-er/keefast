//! Attachments between disk and Vault: reading a file to attach it to an Entry, and saving an
//! attachment back to disk. Both block on the filesystem, so the app runs them off the UI thread.

use std::fs;
use std::io;
use std::path::Path;

use keefast_core::Attachment;

use crate::atomic_file;

/// Reads the file at `path` as an attachment named after the file.
pub fn read_attachment(path: &Path) -> io::Result<Attachment> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file"))?
        .to_string_lossy()
        .into_owned();
    Ok(Attachment::new(name, fs::read(path)?))
}

/// Saves an attachment's contents to `path`, replacing whatever file is there all or nothing.
pub fn write_attachment(attachment: &Attachment, path: &Path) -> io::Result<()> {
    atomic_file::replace(path, attachment.data())
}
