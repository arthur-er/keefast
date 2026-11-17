//! Approvers: who is asked when a use of a secret needs the user's say (ADR 0012 "Approvals").
//!
//! An [`AgentSession`](crate::AgentSession) asks its [`Approver`] when no Agent access decision
//! and no Default agent policy answers a use, and for every use by a shell or interpreter. The
//! ask blocks the calling thread until someone answers or [`APPROVAL_TIMEOUT`] passes, so the
//! session lets go of the Workspace first: a Headless host keeps serving while its user decides.
//!
//! On a Headless host the Approver is an [`ApproverHub`]: the desktop keefasts paired with the
//! host as Approvers, each holding a stream of its prompts open. The first answer from any of
//! them wins, and the others are told to dismiss the prompt. With none connected, a use that
//! would ask is refused at once, so an Agent fails fast instead of hanging.
//!
//! This module has no async runtime of its own, as the Workspace has none: the hub tells each
//! connected Approver through a callback, which the HTTP endpoint turns into an event stream.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Where a connected Approver's prompts stream from, as server-sent events: `prompt` and
/// `dismissed`, each with an [`ApproverEvent`] as JSON.
pub const APPROVER_EVENTS_PATH: &str = "/approver/events";
/// Where an Approver POSTs its [`ApproverAnswer`] to a prompt, at `/approver/prompts/<id>`.
pub const APPROVER_PROMPTS_PATH: &str = "/approver/prompts";

/// Where an Approver POSTs its [`ConsentAnswer`] to an OAuth client's consent, at
/// `/approver/consents/<id>`.
pub const APPROVER_CONSENTS_PATH: &str = "/approver/consents";

/// Where an Approver reads the host's audit log, newest last, as `{"records": [...]}`:
/// `/approver/activity?lines=<n>`, the last [`ACTIVITY_LINES`] records unless it says.
pub const APPROVER_ACTIVITY_PATH: &str = "/approver/activity";
/// How many records the Activity view reads unless it asks for more.
pub const ACTIVITY_LINES: usize = 500;
/// The most records one read of the Activity returns.
pub const MAX_ACTIVITY_LINES: usize = 10_000;

/// How long a prompt waits for an answer before the use is denied.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);

/// What an Approver is asked: may this pairing use this Entry's secret for this command?
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPrompt {
    /// The Agent pairing's name.
    pub pairing: String,
    pub entry_uuid: String,
    pub entry_title: String,
    /// The Secret reference the Agent named the Entry by.
    pub reference: String,
    /// The program, by its executable's name: what a remembered answer is remembered for.
    pub program: String,
    /// The program and its arguments, whole, as the Agent will run them.
    pub command: Vec<String>,
    /// Whether the answer may be remembered: never for shells and interpreters.
    pub can_remember: bool,
    /// For an `http_request`, the host it goes to, which is also its `program`, for Approvers
    /// that don't know `host`; `command` is then the method and the URL, with its placeholders
    /// rather than the secrets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// For a `reveal_secret`, true: allowing it hands the value to the Agent, and so to the
    /// model behind it. Its `program` is then `reveal_secret`, and it can't be remembered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reveal: bool,
}

/// What an Approver is asked when an MCP client pairs by OAuth: approve the code its consent
/// page shows, naming the new Agent pairing and choosing what it reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentPrompt {
    /// The name the client registered with: its own claim, not verified.
    pub client: String,
    /// Where the browser goes back to once approved, by its host.
    pub redirect: String,
    /// The Agent group's subgroups, by name, which the pairing may also read; empty while the
    /// Vault is locked.
    pub subgroups: Vec<String>,
    /// The Agent pairing the client has already, which approving connects it to again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_as: Option<String>,
}

/// An Approver's answer to a [`ConsentPrompt`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentAnswer {
    pub approve: bool,
    /// The code the consent page shows, as the user typed it: needed to approve.
    #[serde(default)]
    pub code: String,
    /// The new Agent pairing's name, which names its subgroup.
    #[serde(default)]
    pub name: String,
    /// The other subgroups of the Agent group it reads, by name.
    #[serde(default)]
    pub reads: Vec<String>,
    #[serde(default)]
    pub can_create: bool,
}

/// An Approver's answer, or why there was none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Allow {
        remember: bool,
    },
    Deny {
        remember: bool,
    },
    /// Nobody answered within [`APPROVAL_TIMEOUT`].
    Unanswered,
    /// Nobody could be asked.
    NoApprover,
}

/// Asks the user about a use. It may block until they answer.
pub trait Approver: Send + Sync {
    fn ask(&self, prompt: &AgentPrompt) -> Approval;
}

/// Nobody to ask: every use that would ask is denied.
pub struct NoApprover;

impl Approver for NoApprover {
    fn ask(&self, _prompt: &AgentPrompt) -> Approval {
        Approval::NoApprover
    }
}

/// What a connected Approver answers to a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApproverAnswer {
    pub allow: bool,
    #[serde(default)]
    pub remember: bool,
}

impl From<ApproverAnswer> for Approval {
    fn from(answer: ApproverAnswer) -> Self {
        let remember = answer.remember;
        if answer.allow {
            Self::Allow { remember }
        } else {
            Self::Deny { remember }
        }
    }
}

/// What a connected Approver is told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ApproverEvent {
    /// A prompt to show, until it is answered, dismissed, or `seconds_left` pass.
    Prompt {
        id: String,
        seconds_left: u32,
        prompt: AgentPrompt,
    },
    /// An OAuth client's consent to approve with its code, until it is answered, dismissed,
    /// or `seconds_left` pass.
    Consent {
        id: String,
        seconds_left: u32,
        consent: ConsentPrompt,
    },
    /// The prompt was answered (here or by another Approver) or timed out: stop showing it.
    Dismissed { id: String },
}

/// Why an Approver's answer wasn't taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    #[error("the prompt was answered already, or it timed out")]
    Gone,
}

/// The Approvers connected to a Headless host, and the prompts waiting for them.
pub struct ApproverHub {
    timeout: Duration,
    state: Mutex<HubState>,
}

/// How the hub tells one connected Approver of an event.
type Tell = Arc<dyn Fn(&ApproverEvent) + Send + Sync>;

/// A connected Approver: its paired name, if it has one, and how to tell it.
struct Connected {
    name: Option<String>,
    tell: Tell,
}

#[derive(Default)]
struct HubState {
    next: u64,
    approvers: HashMap<u64, Connected>,
    waiting: HashMap<String, Waiting>,
    consents: HashMap<String, OfferedConsent>,
}

struct OfferedConsent {
    consent: ConsentPrompt,
    until: std::time::Instant,
}

impl OfferedConsent {
    fn event(&self, id: &str) -> ApproverEvent {
        ApproverEvent::Consent {
            id: id.to_owned(),
            seconds_left: whole_seconds(
                self.until
                    .saturating_duration_since(std::time::Instant::now()),
            ),
            consent: self.consent.clone(),
        }
    }
}

struct Waiting {
    prompt: AgentPrompt,
    asked: std::time::Instant,
    answer: mpsc::Sender<ApproverAnswer>,
}

/// A connected Approver; dropping it disconnects it.
pub struct ApproverConnection {
    hub: Arc<ApproverHub>,
    id: u64,
}

impl Drop for ApproverConnection {
    fn drop(&mut self) {
        self.hub.state().approvers.remove(&self.id);
    }
}

impl ApproverHub {
    /// A hub whose prompts time out after [`APPROVAL_TIMEOUT`].
    pub fn new() -> Arc<Self> {
        Self::with_timeout(APPROVAL_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Arc<Self> {
        Arc::new(Self {
            timeout,
            state: Mutex::new(HubState::default()),
        })
    }

    /// Connects an Approver, which `tell` passes events to; it is told at once of the prompts
    /// already waiting. It stays connected until the connection is dropped.
    pub fn connect(
        self: &Arc<Self>,
        tell: impl Fn(&ApproverEvent) + Send + Sync + 'static,
    ) -> ApproverConnection {
        self.connect_as(None, tell)
    }

    /// [`Self::connect`]s the Approver paired as `name`, which [`Self::disconnect`] can cut
    /// off when it is revoked.
    pub fn connect_named(
        self: &Arc<Self>,
        name: &str,
        tell: impl Fn(&ApproverEvent) + Send + Sync + 'static,
    ) -> ApproverConnection {
        self.connect_as(Some(name.to_owned()), tell)
    }

    /// Disconnects every connection of the Approver paired as `name`: the hub lets go of how
    /// it tells them, which ends their event streams, and asks them nothing more.
    pub fn disconnect(&self, name: &str) {
        self.state()
            .approvers
            .retain(|_, connected| connected.name.as_deref() != Some(name));
    }

    fn connect_as(
        self: &Arc<Self>,
        name: Option<String>,
        tell: impl Fn(&ApproverEvent) + Send + Sync + 'static,
    ) -> ApproverConnection {
        let mut state = self.state();
        for (id, waiting) in &state.waiting {
            tell(&self.prompt_event(id, waiting));
        }
        let now = std::time::Instant::now();
        state.consents.retain(|_, offered| offered.until > now);
        for (id, offered) in &state.consents {
            tell(&offered.event(id));
        }
        state.next += 1;
        let id = state.next;
        state.approvers.insert(
            id,
            Connected {
                name,
                tell: Arc::new(tell),
            },
        );
        ApproverConnection {
            hub: Arc::clone(self),
            id,
        }
    }

    /// Whether any Approver is connected.
    pub fn is_connected(&self) -> bool {
        !self.state().approvers.is_empty()
    }

    /// Answers the prompt `id`. Only the first answer counts; every Approver is then told to
    /// dismiss the prompt.
    pub fn answer(&self, id: &str, answer: ApproverAnswer) -> Result<(), AnswerError> {
        let mut state = self.state();
        let waiting = state.waiting.remove(id).ok_or(AnswerError::Gone)?;
        // The asking thread may have just timed out; then the answer is too late.
        waiting.answer.send(answer).map_err(|_| AnswerError::Gone)?;
        broadcast(&state, &ApproverEvent::Dismissed { id: id.to_owned() });
        Ok(())
    }

    /// Shows every Approver, and each that connects within `lasts`, an OAuth client's consent
    /// to approve as `id`. Nothing waits on it here: the answer goes to the host's OAuth
    /// server, which [`Self::withdraw_consent`]s it.
    pub fn offer_consent(&self, id: &str, consent: ConsentPrompt, lasts: Duration) {
        let offered = OfferedConsent {
            consent,
            until: std::time::Instant::now() + lasts,
        };
        let mut state = self.state();
        broadcast(&state, &offered.event(id));
        state.consents.insert(id.to_owned(), offered);
    }

    /// Tells every Approver to dismiss the consent `id`: approved, denied, or expired.
    pub fn withdraw_consent(&self, id: &str) {
        let mut state = self.state();
        if state.consents.remove(id).is_some() {
            broadcast(&state, &ApproverEvent::Dismissed { id: id.to_owned() });
        }
    }

    fn prompt_event(&self, id: &str, waiting: &Waiting) -> ApproverEvent {
        let left = self.timeout.saturating_sub(waiting.asked.elapsed());
        ApproverEvent::Prompt {
            id: id.to_owned(),
            seconds_left: whole_seconds(left),
            prompt: waiting.prompt.clone(),
        }
    }

    fn state(&self) -> MutexGuard<'_, HubState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Approver for ApproverHub {
    fn ask(&self, prompt: &AgentPrompt) -> Approval {
        let (send, answered) = mpsc::channel();
        let id = {
            let mut state = self.state();
            if state.approvers.is_empty() {
                return Approval::NoApprover;
            }
            let id = random_id();
            let waiting = Waiting {
                prompt: prompt.clone(),
                asked: std::time::Instant::now(),
                answer: send,
            };
            let event = self.prompt_event(&id, &waiting);
            state.waiting.insert(id.clone(), waiting);
            broadcast(&state, &event);
            id
        };
        match answered.recv_timeout(self.timeout) {
            Ok(answer) => answer.into(),
            Err(_) => {
                let mut state = self.state();
                // An answer that came in as the wait ended still counts.
                if let Ok(answer) = answered.try_recv() {
                    return answer.into();
                }
                state.waiting.remove(&id);
                broadcast(&state, &ApproverEvent::Dismissed { id });
                Approval::Unanswered
            }
        }
    }
}

/// Rounded up: a prompt just asked has the whole minute.
fn whole_seconds(left: Duration) -> u32 {
    u32::try_from(left.as_secs() + u64::from(left.subsec_nanos() > 0)).unwrap_or(u32::MAX)
}

fn broadcast(state: &HubState, event: &ApproverEvent) {
    for connected in state.approvers.values() {
        (connected.tell)(event);
    }
}

/// A prompt's id: random, so one Approver can't guess another prompt's.
fn random_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS has randomness");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What an Approver's token starts with, so that it is never mistaken for an Agent pairing's.
pub const APPROVER_TOKEN_PREFIX: &str = "kfapprover_";

/// An Approver allowed to connect, as `keefast-agent pairing approvers` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApproverPairing {
    pub name: String,
    /// When it was paired, in seconds since the Unix epoch.
    pub created: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredApprovers {
    #[serde(default)]
    approvers: Vec<StoredApprover>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredApprover {
    name: String,
    created: i64,
    token_sha256: String,
}

/// Why an Approver couldn't be paired or unpaired.
#[derive(Debug, thiserror::Error)]
pub enum ApproverPairingError {
    #[error("an Approver called {0} is paired already: pick another name, or unpair it first")]
    Taken(String),
    #[error("no Approver called {0} is paired")]
    Unknown(String),
    #[error("an Approver needs a name")]
    NoName,
    #[error("the Approvers file {0} can't be read or written: {1}")]
    Io(String, std::io::Error),
    #[error("the Approvers file {0} is damaged: {1}")]
    Damaged(String, serde_json::Error),
}

/// The Approvers a Headless host lets connect: kept in its own state directory rather than the
/// Vault, as they belong to this host, and they connect while the Vault is locked too. Only a
/// hash of each token is kept.
pub struct ApproverCredentials {
    /// The file they are kept in, or none to keep them in memory.
    path: Option<std::path::PathBuf>,
    stored: Mutex<StoredApprovers>,
}

impl ApproverCredentials {
    /// The Approvers kept in `path`, read now; a missing file is no Approver yet.
    pub fn open(path: impl Into<std::path::PathBuf>) -> Result<Self, ApproverPairingError> {
        let path = path.into();
        let shown = path.display().to_string();
        let stored = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| ApproverPairingError::Damaged(shown, error))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                StoredApprovers::default()
            }
            Err(error) => return Err(ApproverPairingError::Io(shown, error)),
        };
        Ok(Self {
            path: Some(path),
            stored: Mutex::new(stored),
        })
    }

    /// Approvers kept only while this lives, for tests.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            stored: Mutex::new(StoredApprovers::default()),
        }
    }

    /// Pairs an Approver called `name`, at `now` in Unix seconds, and returns its token: the
    /// only time anyone sees it.
    pub fn pair(
        &self,
        name: &str,
        now: i64,
    ) -> Result<(ApproverPairing, zeroize::Zeroizing<String>), ApproverPairingError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApproverPairingError::NoName);
        }
        let mut stored = self.stored();
        if stored
            .approvers
            .iter()
            .any(|approver| approver.name == name)
        {
            return Err(ApproverPairingError::Taken(name.to_owned()));
        }
        let token = new_approver_token();
        let mut changed = stored.clone();
        changed.approvers.push(StoredApprover {
            name: name.to_owned(),
            created: now,
            token_sha256: token_hash(&token),
        });
        self.write(&changed)?;
        *stored = changed;
        Ok((
            ApproverPairing {
                name: name.to_owned(),
                created: now,
            },
            token,
        ))
    }

    /// Unpairs the Approver called `name`: its token stops working at once, though a stream
    /// it holds open stays until it reconnects.
    pub fn unpair(&self, name: &str) -> Result<(), ApproverPairingError> {
        let mut stored = self.stored();
        let mut changed = stored.clone();
        changed.approvers.retain(|approver| approver.name != name);
        if changed.approvers.len() == stored.approvers.len() {
            return Err(ApproverPairingError::Unknown(name.to_owned()));
        }
        self.write(&changed)?;
        *stored = changed;
        Ok(())
    }

    pub fn list(&self) -> Vec<ApproverPairing> {
        self.stored()
            .approvers
            .iter()
            .map(|approver| ApproverPairing {
                name: approver.name.clone(),
                created: approver.created,
            })
            .collect()
    }

    /// The Approver whose token is `token`, by name.
    pub fn approver_with_token(&self, token: &str) -> Option<String> {
        use subtle::ConstantTimeEq;
        let hash = token_hash(token);
        self.stored()
            .approvers
            .iter()
            .find(|approver| bool::from(approver.token_sha256.as_bytes().ct_eq(hash.as_bytes())))
            .map(|approver| approver.name.clone())
    }

    fn stored(&self) -> MutexGuard<'_, StoredApprovers> {
        self.stored
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Writes the file whole, readable only by this user, through a temporary file renamed
    /// over it, so a crash never leaves half of it.
    fn write(&self, stored: &StoredApprovers) -> Result<(), ApproverPairingError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let shown = path.display().to_string();
        let io = |error| ApproverPairingError::Io(shown.clone(), error);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let json = serde_json::to_vec_pretty(stored).expect("Approvers are plain JSON");
        let staged = path.with_extension("json.new");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&staged).map_err(io)?;
        std::io::Write::write_all(&mut file, &json).map_err(io)?;
        file.sync_all().map_err(io)?;
        std::fs::rename(&staged, path).map_err(io)
    }
}

fn token_hash(token: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A new random Approver token: 256 bits, in URL-safe base64.
fn new_approver_token() -> zeroize::Zeroizing<String> {
    use base64::Engine;
    let mut bytes = zeroize::Zeroizing::new([0u8; 32]);
    getrandom::fill(bytes.as_mut()).expect("the OS has randomness");
    zeroize::Zeroizing::new(format!(
        "{APPROVER_TOKEN_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes.as_ref())
    ))
}
