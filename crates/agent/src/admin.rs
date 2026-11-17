//! The Headless host's admin socket: how `keefast-agent unlock`, `lock` and `status` reach
//! `keefast-agent serve` (ADR 0012, ADR 0004 update 2026-11-16).
//!
//! It is a unix socket that only the service's user can open, or a group the operator chooses,
//! so the Master key typed into `unlock` over ssh never travels through the network, a web page
//! or the MCP endpoint. Each connection carries one request and one reply, each a line of JSON.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use keefast_workspace::UnlockSecrets;

use crate::host::{HeadlessHost, HostStatus, UnlockError};

/// The longest request the admin socket reads: a password and a Key file fit many times over.
#[cfg(unix)]
const MAX_REQUEST: u64 = 1 << 20;

/// What an operator asks of a Headless host.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum AdminRequest {
    Unlock(UnlockRequest),
    Lock,
    Status,
}

impl AdminRequest {
    /// Unlocking with the Master key: a password, empty if it has none, and the contents of its
    /// Key file, if it has one. With `remember`, the password is kept for unattended unlock.
    pub fn unlock(password: &str, key_file: Option<&[u8]>, remember: bool) -> Self {
        Self::Unlock(UnlockRequest {
            password: Zeroizing::new(password.to_owned()),
            key_file: key_file.map(|contents| Zeroizing::new(BASE64.encode(contents))),
            remember,
        })
    }
}

/// The Master key, as `keefast-agent unlock` read it from the operator.
#[derive(Serialize, Deserialize)]
pub struct UnlockRequest {
    password: Zeroizing<String>,
    /// The Key file's contents in base64: read by the operator's `unlock`, since the service's
    /// user may not be able to read the operator's files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_file: Option<Zeroizing<String>>,
    #[serde(default)]
    remember: bool,
}

impl std::fmt::Debug for UnlockRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockRequest")
            .field("key_file", &self.key_file.is_some())
            .field("remember", &self.remember)
            .finish_non_exhaustive()
    }
}

/// A Headless host's answer.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum AdminReply {
    /// Done: how the host stands now.
    Status(HostStatus),
    Error {
        kind: AdminErrorKind,
        message: String,
    },
}

/// What went wrong, for `keefast-agent` to tell a wrong Master key from a damaged Vault file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminErrorKind {
    WrongKey,
    Damaged,
    KeyFile,
    Storage,
    SecretStore,
    Save,
    BadRequest,
}

impl AdminReply {
    fn error(kind: AdminErrorKind, message: impl ToString) -> Self {
        Self::Error {
            kind,
            message: message.to_string(),
        }
    }
}

/// Carries out an operator's request.
pub fn handle(host: &HeadlessHost, request: AdminRequest) -> AdminReply {
    match request {
        AdminRequest::Unlock(request) => {
            let password = SecretString::from(request.password.as_str());
            let secrets = match &request.key_file {
                None => UnlockSecrets::new(password.clone(), None),
                Some(encoded) => match BASE64.decode(encoded.as_bytes()) {
                    Ok(contents) => UnlockSecrets::with_key_file_contents(
                        password.clone(),
                        Zeroizing::new(contents),
                    ),
                    Err(_) => {
                        return AdminReply::error(
                            AdminErrorKind::BadRequest,
                            "the Key file was garbled",
                        );
                    }
                },
            };
            if let Err(error) = host.unlock(secrets) {
                let kind = match error {
                    UnlockError::WrongKey => AdminErrorKind::WrongKey,
                    UnlockError::Damaged(_) => AdminErrorKind::Damaged,
                    UnlockError::KeyFile(_) => AdminErrorKind::KeyFile,
                    UnlockError::Storage(_) => AdminErrorKind::Storage,
                };
                return AdminReply::error(kind, error);
            }
            if request.remember
                && let Err(error) = host.remember_password(&password)
            {
                return AdminReply::error(
                    AdminErrorKind::SecretStore,
                    format!(
                        "the Vault is unlocked, but its password couldn't be remembered: {error}"
                    ),
                );
            }
            AdminReply::Status(host.status())
        }
        AdminRequest::Lock => match host.lock() {
            Ok(()) => AdminReply::Status(host.status()),
            Err(error) => AdminReply::error(AdminErrorKind::Save, error),
        },
        AdminRequest::Status => AdminReply::Status(host.status()),
    }
}

#[cfg(unix)]
pub use socket::{AdminGroup, AdminSocket, send};

#[cfg(unix)]
mod socket {
    use std::ffi::CString;
    use std::fs;
    use std::io::{self, BufRead, BufReader, Read, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::thread;

    use super::{AdminErrorKind, AdminReply, AdminRequest, MAX_REQUEST, handle};
    use crate::host::HeadlessHost;

    /// The group of operators the admin socket is opened to, besides the service's user.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AdminGroup(libc::gid_t);

    impl AdminGroup {
        pub fn with_id(id: libc::gid_t) -> Self {
            Self(id)
        }

        /// The group called `name` on this system.
        pub fn named(name: &str) -> io::Result<Self> {
            let c_name = CString::new(name)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad group name"))?;
            let mut group: libc::group = unsafe { std::mem::zeroed() };
            let mut buffer = vec![0 as libc::c_char; 16 * 1024];
            let mut found: *mut libc::group = std::ptr::null_mut();
            // SAFETY: every pointer is valid for the call, and the buffer's length is given.
            let status = unsafe {
                libc::getgrnam_r(
                    c_name.as_ptr(),
                    &mut group,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    &mut found,
                )
            };
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status));
            }
            if found.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("there is no group {name}"),
                ));
            }
            Ok(Self(group.gr_gid))
        }
    }

    /// The admin socket, listening. Its file is removed when it is dropped.
    pub struct AdminSocket {
        listener: UnixListener,
        path: PathBuf,
    }

    impl AdminSocket {
        /// Listens at `path`, which only this user can open, or with `group` also that group.
        /// A missing directory is made, open to as few. A socket left by a host that is gone is
        /// replaced; one a running host answers on is refused.
        pub fn bind(path: &Path, group: Option<AdminGroup>) -> io::Result<Self> {
            if let Some(dir) = path.parent().filter(|dir| !dir.exists()) {
                let mode = if group.is_some() { 0o750 } else { 0o700 };
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(mode)
                    .create(dir)?;
                if let Some(group) = group {
                    chown_group(dir, group)?;
                }
            }
            if path.exists() {
                if UnixStream::connect(path).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        format!("a Headless host already listens at {}", path.display()),
                    ));
                }
                fs::remove_file(path)?;
            }
            // So the socket is never open to others, not even before its mode is set.
            // SAFETY: umask has no preconditions.
            let umask = unsafe { libc::umask(0o177) };
            let bound = UnixListener::bind(path);
            // SAFETY: as above.
            unsafe { libc::umask(umask) };
            let listener = bound?;
            let socket = Self {
                listener,
                path: path.to_owned(),
            };
            match group {
                Some(group) => {
                    chown_group(path, group)?;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
                }
                None => fs::set_permissions(path, fs::Permissions::from_mode(0o600))?,
            }
            Ok(socket)
        }

        /// Answers requests for `host` on a thread of its own, each connection on its own
        /// thread, so a slow unlock doesn't hold up `status`.
        pub fn serve(self, host: Arc<HeadlessHost>) {
            thread::spawn(move || {
                for stream in self.listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let host = Arc::clone(&host);
                    thread::spawn(move || {
                        let _ = answer(&host, stream);
                    });
                }
            });
        }
    }

    impl Drop for AdminSocket {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn answer(host: &HeadlessHost, stream: UnixStream) -> io::Result<()> {
        let mut line = String::new();
        BufReader::new((&stream).take(MAX_REQUEST)).read_line(&mut line)?;
        let reply = match serde_json::from_str::<AdminRequest>(&line) {
            Ok(request) => handle(host, request),
            Err(_) => AdminReply::Error {
                kind: AdminErrorKind::BadRequest,
                message: "keefast-agent doesn't understand the request: is it the same version?"
                    .to_owned(),
            },
        };
        let mut json = serde_json::to_string(&reply).expect("a reply is plain JSON");
        json.push('\n');
        (&stream).write_all(json.as_bytes())
    }

    /// Sends `request` to the Headless host listening at `path`, and returns its reply.
    pub fn send(path: &Path, request: &AdminRequest) -> io::Result<AdminReply> {
        let mut stream = UnixStream::connect(path)?;
        let mut json = serde_json::to_string(request).expect("a request is plain JSON");
        json.push('\n');
        stream.write_all(json.as_bytes())?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        serde_json::from_str(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn chown_group(path: &Path, group: AdminGroup) -> io::Result<()> {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad path"))?;
        // SAFETY: the path is a valid C string; -1 leaves the owner as it is.
        let status = unsafe { libc::chown(c_path.as_ptr(), libc::uid_t::MAX, group.0) };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
