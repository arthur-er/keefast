//! How the native host reaches the app: a Unix socket on Linux, a named pipe on Windows. Both
//! ends frame messages as native messaging does, so the host relays them unchanged.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use interprocess::local_socket::{
    GenericFilePath, Listener, ListenerOptions, Name, Stream, ToFsName, prelude::*,
};

/// The largest message either way. Chromium refuses larger messages from a host.
pub const MAX_MESSAGE: usize = 1024 * 1024;

/// Where the app listens for the native host: a socket in the user's runtime directory, which
/// only the user can reach, or a named pipe named after the user.
pub fn socket_path() -> PathBuf {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_default();
        PathBuf::from(format!(r"\\.\pipe\keefast-browser-{user}"))
    }
    #[cfg(not(windows))]
    {
        dirs::runtime_dir()
            .or_else(dirs::cache_dir)
            .unwrap_or_else(std::env::temp_dir)
            .join("keefast")
            .join("browser.sock")
    }
}

fn name(path: &Path) -> io::Result<Name<'_>> {
    path.to_fs_name::<GenericFilePath>()
}

pub fn connect(path: &Path) -> io::Result<Stream> {
    Stream::connect(name(path)?)
}

/// Starts listening at `path`. Fails with [`io::ErrorKind::AddrInUse`] while another keefast
/// listens there; a socket left behind by one that crashed is replaced.
pub fn listen(path: &Path) -> io::Result<Listener> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        if path.exists() {
            if connect(path).is_ok() {
                return Err(io::ErrorKind::AddrInUse.into());
            }
            std::fs::remove_file(path)?;
        }
    }
    ListenerOptions::new().name(name(path)?).create_sync()
}

/// Reads one message: its length as a native-endian `u32`, then the message. `None` when the
/// other end has closed the connection between messages.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_ne_bytes(length) as usize;
    if length > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a {length}-byte message is too large"),
        ));
    }
    let mut message = vec![0; length];
    reader.read_exact(&mut message)?;
    Ok(Some(message))
}

pub fn write_frame(writer: &mut impl Write, message: &[u8]) -> io::Result<()> {
    let length = u32::try_from(message.len())
        .ok()
        .filter(|&length| length as usize <= MAX_MESSAGE)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
    writer.write_all(&length.to_ne_bytes())?;
    writer.write_all(message)?;
    writer.flush()
}
