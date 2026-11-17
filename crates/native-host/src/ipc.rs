//! How the native host reaches the app: a Unix socket on Linux, a named pipe on Windows. Both
//! ends frame messages as native messaging does, so the host relays them unchanged.
//!
//! `keefast-agent` reaches the app the same way, at an endpoint of its own named `agent`
//! beside the browser's (ADR 0012 update "In-app host"), with the same peer checks.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

use interprocess::local_socket::{
    GenericFilePath, Listener, ListenerOptions, Name, Stream, ToFsName, prelude::*,
};

/// The largest message either way. Chromium refuses larger messages from a host.
pub const MAX_MESSAGE: usize = 1024 * 1024;

/// Where the app listens for the native host: a socket in the user's runtime directory, which
/// only the user can reach, or a named pipe named after the user, which only the user can
/// connect to. Without a runtime directory there is nowhere private for the socket, so this
/// fails rather than fall back to a directory other users share.
pub fn socket_path() -> io::Result<PathBuf> {
    endpoint_path(BROWSER)
}

/// The browser's endpoint, which the native host relays to.
pub const BROWSER: &str = "browser";
/// `keefast-agent`'s endpoint, apart from the browser's so no extension can reach it.
pub const AGENT: &str = "agent";

/// Where the app listens for the program `name` names ([`BROWSER`] or [`AGENT`]), private to
/// the user as [`socket_path`] says.
pub fn endpoint_path(name: &str) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_default();
        Ok(PathBuf::from(format!(r"\\.\pipe\keefast-{name}-{user}")))
    }
    #[cfg(not(windows))]
    {
        endpoint_path_in(dirs::runtime_dir(), name)
    }
}

/// The browser socket's path in `runtime_dir`, the user's runtime directory, if there is one.
#[cfg(not(windows))]
pub fn socket_path_in(runtime_dir: Option<PathBuf>) -> io::Result<PathBuf> {
    endpoint_path_in(runtime_dir, BROWSER)
}

/// The socket `name`'s path in `runtime_dir`, the user's runtime directory, if there is one.
#[cfg(not(windows))]
pub fn endpoint_path_in(runtime_dir: Option<PathBuf>, name: &str) -> io::Result<PathBuf> {
    let dir = runtime_dir.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "no runtime directory (XDG_RUNTIME_DIR) to keep keefast's {name} socket private in"
            ),
        )
    })?;
    Ok(dir.join("keefast").join(format!("{name}.sock")))
}

fn name(path: &Path) -> io::Result<Name<'_>> {
    path.to_fs_name::<GenericFilePath>()
}

/// Connects to the app at `path`, if it runs as the same user as this process. Another user's
/// is refused with [`io::ErrorKind::PermissionDenied`], so nothing is relayed to it.
pub fn connect(path: &Path) -> io::Result<Stream> {
    let stream = Stream::connect(name(path)?)?;
    if !owner::is_same_user(&stream)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is served by another user", path.display()),
        ));
    }
    Ok(stream)
}

/// Starts listening at `path`. Fails with [`io::ErrorKind::AddrInUse`] while another keefast
/// listens there; a socket left behind by one that crashed is replaced. On Unix the socket's
/// directory must be the user's own and closed to others, or it fails with
/// [`io::ErrorKind::PermissionDenied`]. On Windows only the user can connect to the pipe.
pub fn listen(path: &Path) -> io::Result<Listener> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
            owner::ensure_private(dir)?;
        }
        if path.exists() {
            if connect(path).is_ok() {
                return Err(io::ErrorKind::AddrInUse.into());
            }
            std::fs::remove_file(path)?;
        }
        ListenerOptions::new().name(name(path)?).create_sync()
    }
    #[cfg(windows)]
    {
        use interprocess::os::windows::local_socket::ListenerOptionsExt;

        ListenerOptions::new()
            .name(name(path)?)
            .security_descriptor(owner::only_this_user()?)
            .create_sync()
    }
}

/// Reads one message: its length as a native-endian `u32`, then the message. `None` when the
/// other end has closed the connection between messages.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    read_frame_up_to(reader, MAX_MESSAGE)
}

/// [`read_frame`], for messages up to `max` bytes rather than [`MAX_MESSAGE`]: for an
/// endpoint no browser reaches.
pub fn read_frame_up_to(reader: &mut impl Read, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_ne_bytes(length) as usize;
    if length > max {
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
    write_frame_up_to(writer, message, MAX_MESSAGE)
}

/// [`write_frame`], for messages up to `max` bytes.
pub fn write_frame_up_to(writer: &mut impl Write, message: &[u8], max: usize) -> io::Result<()> {
    let length = u32::try_from(message.len())
        .ok()
        .filter(|&length| length as usize <= max)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
    writer.write_all(&length.to_ne_bytes())?;
    writer.write_all(message)?;
    writer.flush()
}

/// What the app hears from the native hosts connected to it. Each host is numbered.
pub enum ServerEvent {
    /// A host connected; send it replies through `Replies`.
    Opened(u64, Replies),
    Message(u64, Vec<u8>),
    Closed(u64),
}

/// Where to send a connected host its replies. They are written on a thread of their own.
pub struct Replies(mpsc::Sender<Vec<u8>>);

impl Replies {
    /// Queues a reply. One for a host that has gone is dropped: its connection closes soon.
    pub fn send(&self, message: impl Into<Vec<u8>>) {
        let _ = self.0.send(message.into());
    }
}

/// The app's end: accepts native hosts at a socket until dropped.
pub struct Server {
    path: PathBuf,
    stopped: Arc<AtomicBool>,
    accepting: Option<thread::JoinHandle<()>>,
}

impl Server {
    /// Listens at `path`, and hands every host's events to `events`, from other threads,
    /// until `events` returns false or the server is dropped. Fails while another keefast
    /// listens there.
    pub fn start(
        path: PathBuf,
        events: impl Fn(ServerEvent) -> bool + Send + Sync + 'static,
    ) -> io::Result<Self> {
        Self::start_up_to(path, MAX_MESSAGE, events)
    }

    /// [`Self::start`], for messages either way of up to `max` bytes.
    pub fn start_up_to(
        path: PathBuf,
        max: usize,
        events: impl Fn(ServerEvent) -> bool + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let listener = listen(&path)?;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let accepting = thread::spawn(move || accept(listener, &stop, max, Arc::new(events)));
        Ok(Self {
            path,
            stopped,
            accepting: Some(accepting),
        })
    }
}

impl Drop for Server {
    /// Stops accepting hosts, and removes the socket. Hosts already connected stay connected.
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        // Wakes the accepting thread so it sees it should stop, and waits for it to let go of
        // the socket.
        if connect(&self.path).is_ok()
            && let Some(accepting) = self.accepting.take()
        {
            let _ = accepting.join();
        }
    }
}

fn accept(
    listener: Listener,
    stopped: &AtomicBool,
    max: usize,
    events: Arc<dyn Fn(ServerEvent) -> bool + Send + Sync>,
) {
    for (id, stream) in (0..).zip(listener.incoming()) {
        if stopped.load(Ordering::Acquire) {
            return;
        }
        let Ok(stream) = stream else {
            continue;
        };
        let (mut from_host, mut to_host) = stream.split();
        let (replies, outgoing) = mpsc::channel::<Vec<u8>>();
        if !events(ServerEvent::Opened(id, Replies(replies))) {
            return;
        }
        thread::spawn(move || {
            for reply in outgoing {
                if write_frame_up_to(&mut to_host, &reply, max).is_err() {
                    break;
                }
            }
        });
        let events = Arc::clone(&events);
        thread::spawn(move || {
            while let Ok(Some(message)) = read_frame_up_to(&mut from_host, max) {
                if !events(ServerEvent::Message(id, message)) {
                    return;
                }
            }
            events(ServerEvent::Closed(id));
        });
    }
}

/// Telling whether the app at the other end of a connection runs as this process's user.
#[cfg(unix)]
mod owner {
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    use interprocess::local_socket::Stream;
    use interprocess::local_socket::traits::StreamCommon;

    /// Whether the app runs as this user, as the kernel recorded when it started listening.
    pub fn is_same_user(stream: &Stream) -> io::Result<bool> {
        Ok(is_user(stream.peer_creds()?.euid(), this_user()))
    }

    /// Whether `peer`, the user the other end runs as if the kernel says, is `user`.
    fn is_user(peer: Option<u32>, user: u32) -> bool {
        peer == Some(user)
    }

    /// Fails unless `dir` is this user's, and other users can't get into it.
    pub fn ensure_private(dir: &Path) -> io::Result<()> {
        let metadata = std::fs::metadata(dir)?;
        if metadata.uid() != this_user() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} is open to other users, so keefast's socket can't go there",
                    dir.display()
                ),
            ));
        }
        Ok(())
    }

    fn this_user() -> u32 {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Listening as another user takes another account, so the decision is tested alone.
        #[test]
        fn an_app_of_another_user_is_refused() {
            let user = this_user();
            assert!(is_user(Some(user), user));
            assert!(!is_user(Some(user.wrapping_add(1)), user));
            assert!(!is_user(Some(0), 1000), "not even root's");
            assert!(!is_user(None, user), "nor one the kernel can't vouch for");
        }
    }
}

/// Telling whether the app serving a pipe runs as this process's user, and making a pipe only
/// this user can connect to.
#[cfg(windows)]
mod owner {
    use std::ffi::c_void;
    use std::io;
    use std::ptr;

    use interprocess::local_socket::Stream;
    use interprocess::local_socket::traits::StreamCommon;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::{U16CStr, U16CString};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// Whether the process serving the pipe runs as this user.
    pub fn is_same_user(stream: &Stream) -> io::Result<bool> {
        let server = stream
            .peer_creds()?
            .pid()
            .ok_or_else(|| io::Error::other("the pipe's server has no process ID"))?;
        // SAFETY: OpenProcess has no preconditions; it returns null on failure.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, server) };
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let process = Handle(process);
        let theirs = TokenUserInfo::of(process.0)?;
        // SAFETY: the current process's pseudo handle needs no closing.
        let ours = TokenUserInfo::of(unsafe { GetCurrentProcess() })?;
        // SAFETY: both SIDs point into buffers alive for the call.
        Ok(unsafe { EqualSid(theirs.sid(), ours.sid()) } != 0)
    }

    /// A security descriptor whose access list lets only this user at the pipe.
    pub fn only_this_user() -> io::Result<SecurityDescriptor> {
        // SAFETY: the current process's pseudo handle needs no closing.
        let user = TokenUserInfo::of(unsafe { GetCurrentProcess() })?;
        let mut text: *mut u16 = ptr::null_mut();
        // SAFETY: the SID is valid; the string Windows allocates for it is freed below.
        if unsafe { ConvertSidToStringSidW(user.sid(), &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: Windows returned a nul-terminated string.
        let sid = unsafe { U16CStr::from_ptr_str(text) }.to_string_lossy();
        // SAFETY: ConvertSidToStringSidW allocated the string with LocalAlloc.
        unsafe { LocalFree(text.cast::<c_void>()) };
        // A protected access list, inheriting nothing, that grants this user alone everything.
        let sddl = U16CString::from_str(format!("D:P(A;;GA;;;{sid})"))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        SecurityDescriptor::deserialize(&sddl)
    }

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was opened here, and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// A process token's TOKEN_USER, in a buffer aligned for it.
    struct TokenUserInfo(Vec<u64>);

    impl TokenUserInfo {
        fn of(process: HANDLE) -> io::Result<Self> {
            let mut token: HANDLE = ptr::null_mut();
            // SAFETY: `token` is written on success.
            if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Handle(token);
            let mut size = 0u32;
            // SAFETY: with no buffer, this only asks for the size needed.
            unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut size) };
            let mut buffer = vec![0u64; (size as usize).div_ceil(size_of::<u64>())];
            // SAFETY: the buffer holds at least `size` bytes.
            let read = unsafe {
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    size,
                    &mut size,
                )
            };
            if read == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(buffer))
        }

        fn sid(&self) -> PSID {
            // SAFETY: the buffer holds a TOKEN_USER, aligned for it.
            unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        }
    }
}
