//! The Headless host's admin socket: how `keefast-agent unlock`, `lock` and `status` reach
//! `keefast-agent serve` (ADR 0012, ADR 0004 update 2026-11-16).
//!
//! It is a unix socket that only the service's user can open, or a group the operator chooses,
//! so the Master key typed into `unlock` over ssh never travels through the network, a web page
//! or the MCP endpoint. Each connection carries one request and one reply, each a line of JSON.
//!
//! Agent pairings are made and revoked here too (`keefast-agent pairing`), so an operator with
//! only a shell on the host can pair an Agent: the token is printed once, on the operator's
//! terminal, and the Vault keeps only its hash. `keefast-agent log` reads the audit log here.
//! Approvers are paired here too (`keefast-agent pairing approver`): the first one can't be
//! approved by an Approver, as there is none yet.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use keefast_core::{Group, GroupId, Vault};
use keefast_workspace::{EditVaultError, UnlockSecrets, VaultId, Workspace};

use crate::approver::ApproverPairing;
use crate::audit::AuditRecord;
use crate::host::{
    ConsentApprovalError, HeadlessHost, HostStatus, PairingChoice, UnlockError, subgroups_named,
};
use crate::{AgentPairing, AgentSettings, AgentSettingsError, AgentSettingsExt};

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
    /// Creates an Agent pairing, with its own subgroup of the Agent group and a new token.
    PairingCreate(PairingCreate),
    PairingList,
    PairingRevoke {
        name: String,
    },
    /// The audit log's last `lines` records.
    Log {
        lines: usize,
    },
    /// Pairs a desktop keefast as an Approver, with a new token. It works while the Vault is
    /// locked: Approvers are kept in the host's state, not the Vault.
    ApproverPair {
        name: String,
    },
    ApproverList,
    ApproverRevoke {
        name: String,
    },
    /// Approves the OAuth client whose consent page shows `code`, creating its Agent pairing
    /// as `pairing` says; an empty name names it as the client calls itself.
    PairingApprove {
        code: String,
        #[serde(flatten)]
        pairing: PairingChoice,
    },
}

/// An Agent pairing to create, as `keefast-agent pairing create` names it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingCreate {
    pub name: String,
    /// The names of the other subgroups of the Agent group it reads.
    #[serde(default)]
    pub reads: Vec<String>,
    #[serde(default)]
    pub can_create: bool,
    /// The Group, by its path from the root, to make the Agent group if the Vault has none
    /// yet. It is made if it isn't there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_group: Option<String>,
}

/// An Agent pairing, as `keefast-agent pairing list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingSummary {
    pub name: String,
    /// The names of the other subgroups of the Agent group it reads.
    pub reads: Vec<String>,
    pub can_create: bool,
}

/// A bearer token, which `Debug` doesn't show.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Token(pub Zeroizing<String>);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
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
    /// The Vault's Agent group, by its path, and its Agent pairings.
    Pairings {
        agent_group: Option<String>,
        pairings: Vec<PairingSummary>,
    },
    /// A pairing made, with its token: the only time anyone sees it.
    PairingCreated {
        pairing: PairingSummary,
        token: Token,
    },
    /// Audit log records, oldest first.
    Log { records: Vec<AuditRecord> },
    /// The Approvers paired with this host.
    Approvers { approvers: Vec<ApproverPairing> },
    /// An OAuth client approved: the Agent pairing it pairs as, and whether it had it already.
    ConsentApproved {
        client: String,
        pairing: String,
        again: bool,
    },
    /// An Approver paired, with its token: the only time anyone sees it.
    ApproverPaired {
        approver: ApproverPairing,
        token: Token,
    },
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
    /// The Vault is locked, and Agent pairings live in it.
    Locked,
    /// The Agent pairing can't be made or found as asked.
    Pairing,
    /// The audit log can't be read.
    Log,
    /// The Approver can't be paired or found as asked.
    Approver,
    /// No OAuth client waits with that code.
    Consent,
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
        // The log holds no secret, and is kept outside the Vault: it reads while locked too.
        AdminRequest::Log { lines } => match host.audit_records(lines) {
            Ok(records) => AdminReply::Log { records },
            Err(error) => AdminReply::error(
                AdminErrorKind::Log,
                format!("the audit log can't be read: {error}"),
            ),
        },
        AdminRequest::ApproverPair { name } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| {
                    i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
                });
            match host.approver_credentials().pair(&name, now) {
                Ok((approver, token)) => AdminReply::ApproverPaired {
                    approver,
                    token: Token(token),
                },
                Err(error) => AdminReply::error(AdminErrorKind::Approver, error),
            }
        }
        AdminRequest::ApproverList => AdminReply::Approvers {
            approvers: host.approver_credentials().list(),
        },
        AdminRequest::ApproverRevoke { name } => match host.revoke_approver(&name) {
            Ok(()) => AdminReply::Approvers {
                approvers: host.approver_credentials().list(),
            },
            Err(error) => AdminReply::error(AdminErrorKind::Approver, error),
        },
        AdminRequest::PairingApprove { code, pairing } => {
            match host.approve_consent_code(&code, pairing) {
                Ok(approved) => AdminReply::ConsentApproved {
                    client: approved.client,
                    pairing: approved.pairing,
                    again: approved.again,
                },
                Err(error) => {
                    let kind = match error {
                        ConsentApprovalError::Consent(_) => AdminErrorKind::Consent,
                        ConsentApprovalError::Locked => AdminErrorKind::Locked,
                        ConsentApprovalError::Pairing(_) | ConsentApprovalError::NoSubgroup(_) => {
                            AdminErrorKind::Pairing
                        }
                        ConsentApprovalError::Save(_) => AdminErrorKind::Save,
                    };
                    AdminReply::error(kind, error)
                }
            }
        }
        AdminRequest::PairingList => host
            .with_vault(|workspace, vault| pairings(workspace, vault))
            .unwrap_or_else(locked),
        AdminRequest::PairingCreate(create) => edit(host, |workspace, vault| {
            create_pairing(workspace, vault, create)
        }),
        AdminRequest::PairingRevoke { name } => edit(host, |workspace, vault| {
            let settings = workspace.agent_settings(vault).ok_or_else(locked_error)?;
            let pairing = settings
                .pairings()
                .iter()
                .find(|pairing| pairing.name() == name)
                .ok_or_else(|| {
                    AdminReply::error(
                        AdminErrorKind::Pairing,
                        format!("there is no Agent pairing called {name}"),
                    )
                })?;
            workspace
                .revoke_agent_pairing(vault, pairing.id())
                .map_err(|error| AdminReply::error(AdminErrorKind::Pairing, error))?;
            Ok(pairings(workspace, vault))
        }),
    }
}

fn locked_error() -> AdminReply {
    AdminReply::error(
        AdminErrorKind::Locked,
        "the Vault is locked, and Agent pairings are kept in it: unlock it first",
    )
}

fn locked() -> AdminReply {
    locked_error()
}

/// Runs `f` on the unlocked Vault and saves it; the reply is `f`'s, unless the save failed.
fn edit(
    host: &HeadlessHost,
    f: impl FnOnce(&mut Workspace, VaultId) -> Result<AdminReply, AdminReply>,
) -> AdminReply {
    match host.edit(f) {
        None => locked(),
        Some((Err(reply), _)) => reply,
        Some((Ok(_), Err(error))) => AdminReply::error(AdminErrorKind::Save, error),
        Some((Ok(reply), Ok(()))) => reply,
    }
}

fn pairings(workspace: &Workspace, vault: VaultId) -> AdminReply {
    let Some(open) = workspace.readable_vault(vault) else {
        return locked();
    };
    let vault = open.vault();
    let settings = AgentSettings::of(vault);
    AdminReply::Pairings {
        agent_group: settings
            .agent_group()
            .and_then(|group| group_path(vault, group)),
        pairings: settings
            .pairings()
            .iter()
            .map(|pairing| summary(vault, pairing))
            .collect(),
    }
}

fn summary(vault: &Vault, pairing: &AgentPairing) -> PairingSummary {
    PairingSummary {
        name: pairing.name().to_owned(),
        reads: pairing
            .readable()
            .iter()
            .filter_map(|group| vault.group(*group))
            .map(|group| group.name().to_owned())
            .collect(),
        can_create: pairing.can_create(),
    }
}

/// `group`'s path from the root Group, which it leaves out.
fn group_path(vault: &Vault, group: GroupId) -> Option<String> {
    fn find(at: &Group, group: GroupId, names: &mut Vec<String>) -> bool {
        at.groups().iter().any(|child| {
            names.push(child.name().to_owned());
            if child.id() == group || find(child, group, names) {
                return true;
            }
            names.pop();
            false
        })
    }
    let mut names = Vec::new();
    find(vault.root(), group, &mut names).then(|| names.join("/"))
}

fn create_pairing(
    workspace: &mut Workspace,
    vault: VaultId,
    create: PairingCreate,
) -> Result<AdminReply, AdminReply> {
    let failed = |error: AgentSettingsError| AdminReply::error(AdminErrorKind::Pairing, error);
    let agent_group = agent_group(workspace, vault, create.agent_group.as_deref())?;
    let open = workspace.readable_vault(vault).ok_or_else(locked_error)?;
    let reads = subgroups_named(open.vault(), agent_group, &create.reads).map_err(|name| {
        AdminReply::error(
            AdminErrorKind::Pairing,
            format!("the Agent group has no subgroup called {name}"),
        )
    })?;
    let created = workspace
        .create_agent_pairing(vault, &create.name, reads, create.can_create)
        .map_err(failed)?;
    let vault = workspace
        .readable_vault(vault)
        .ok_or_else(locked_error)?
        .vault();
    Ok(AdminReply::PairingCreated {
        pairing: summary(vault, &created.pairing),
        token: Token(created.token),
    })
}

/// The Vault's Agent group; when it has none yet, the Group at `path` (`--agent-group`),
/// made where it is missing and set as the Agent group.
fn agent_group(
    workspace: &mut Workspace,
    vault: VaultId,
    path: Option<&str>,
) -> Result<GroupId, AdminReply> {
    let failed = |error: AgentSettingsError| AdminReply::error(AdminErrorKind::Pairing, error);
    let settings = workspace.agent_settings(vault).ok_or_else(locked_error)?;
    match (settings.agent_group(), path) {
        (Some(group), _) => Ok(group),
        (None, None) => Err(AdminReply::error(
            AdminErrorKind::Pairing,
            "the Vault has no Agent group yet: name one with --agent-group, or choose one \
             in keefast's Vault settings, under Agents",
        )),
        (None, Some(path)) => {
            let group = group_at_path(workspace, vault, path).map_err(failed)?;
            workspace.set_agent_group(vault, group).map_err(failed)?;
            Ok(group)
        }
    }
}

/// The Group at `path` from the root Group, made where it is missing.
fn group_at_path(
    workspace: &mut Workspace,
    vault: VaultId,
    path: &str,
) -> Result<GroupId, AgentSettingsError> {
    let mut at = workspace
        .readable_vault(vault)
        .ok_or(EditVaultError::VaultNotOpen)?
        .vault()
        .root()
        .id();
    for name in path
        .split('/')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let found = workspace
            .readable_vault(vault)
            .and_then(|open| open.vault().group(at))
            .and_then(|group| group.groups().iter().find(|child| child.name() == name))
            .map(|group| group.id());
        at = match found {
            Some(group) => group,
            None => workspace.create_group(vault, at, name)?,
        };
    }
    Ok(at)
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
            // Bound in a directory only this user can enter, its mode set, then moved into
            // place: so the socket is never open to others, not even for a moment. (A umask
            // would do too, but it is the whole process's, and other threads make files.)
            let staging = staging_dir(path)?;
            let staged = staging.join("admin.sock");
            let bound = (|| -> io::Result<UnixListener> {
                let listener = UnixListener::bind(&staged)?;
                match group {
                    Some(group) => {
                        chown_group(&staged, group)?;
                        fs::set_permissions(&staged, fs::Permissions::from_mode(0o660))?;
                    }
                    None => fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?,
                }
                fs::rename(&staged, path)?;
                Ok(listener)
            })();
            let _ = fs::remove_file(&staged);
            let _ = fs::remove_dir(&staging);
            Ok(Self {
                listener: bound?,
                path: path.to_owned(),
            })
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

    /// A new directory beside `path` that only this user can enter.
    fn staging_dir(path: &Path) -> io::Result<PathBuf> {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let dir = path.with_file_name(format!(".keefast-agent-{name}"));
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        Ok(dir)
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
