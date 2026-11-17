//! Agent hosts: the Headless hosts this desktop keefast is an Approver for (ADR 0012
//! "Approvals"), under Settings → Agent hosts.
//!
//! Each host is kept by its name and address in `agent-hosts.json` in keefast's config
//! directory, and its Approver token in the OS secret store, as Dropbox's is (ADR 0005). For
//! each, an [`ApproverClient`] holds the host's prompt stream open on a thread of its own. The
//! prompts it hears wait here, oldest first, and the oldest is put to the user as the approval
//! prompt; the window comes to the front for each new one. A prompt leaves when it is answered
//! here, or when the host says to dismiss it: another Approver answered first, or nobody did in
//! time. A dropped stream takes its host's prompts with it, since they can't be answered.
//!
//! A host also asks its Approvers to approve MCP clients pairing by OAuth (ADR 0012 update
//! 2026-11-17 "OAuth"): those consents wait here too, after the prompts, since a use blocks an
//! Agent for at most a minute and a consent waits ten. Approving one needs the code its consent
//! page shows, which the user types; a wrong code leaves it waiting.

use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

use keefast_agent::approver::{APPROVER_TOKEN_PREFIX, ApproverAnswer};
use keefast_agent::approver_client::{
    ActivityError, AgentHost, AnswerPromptError, ApproverClient, ApproverClientEvent,
    agent_activity, answer_consent, answer_prompt,
};
use keefast_agent::{AgentPrompt, AuditRecord, ConsentAnswer, ConsentPrompt};
use keefast_workspace::SecretStore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::core::AppCore;
use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::in_app_agents::{LOCAL_HOST, LOCAL_HOST_NAME};

/// The hosts, their connections, and the prompts waiting for the user.
pub struct AgentHosts {
    /// Where the hosts are kept; `None` keeps them in memory, for tests.
    file: Option<PathBuf>,
    secrets: Arc<dyn SecretStore>,
    hosts: Vec<Host>,
    prompts: Vec<WaitingPrompt>,
    consents: Vec<WaitingConsent>,
    /// The prompt last put to the user, so each new one brings keefast to the front once.
    shown: Option<String>,
    /// Whether hosts are connected to; tests hand prompts in themselves.
    connects: bool,
}

struct Host {
    saved: SavedHost,
    connection: Connection,
    /// Dropping it closes the stream.
    _client: Option<ApproverClient>,
    token: Zeroizing<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedHost {
    id: String,
    name: String,
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SavedHosts {
    #[serde(default)]
    hosts: Vec<SavedHost>,
}

/// How a host's prompt stream stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected,
    /// It dropped or couldn't be opened, and why; it is opened again by itself.
    Disconnected(String),
}

/// A prompt from a host, waiting for the user.
#[derive(Debug, Clone)]
pub struct WaitingPrompt {
    pub host: String,
    pub host_name: String,
    pub id: String,
    pub prompt: AgentPrompt,
    /// When the host denies it if nobody answers.
    pub expires: SystemTime,
}

/// An OAuth client's consent from a host, waiting for the user to approve its code.
#[derive(Debug, Clone)]
pub struct WaitingConsent {
    pub host: String,
    pub host_name: String,
    pub id: String,
    pub consent: ConsentPrompt,
    /// When the host forgets it if nobody approves it.
    pub expires: SystemTime,
}

/// A host as Settings shows it: never its token.
#[derive(Debug, Clone)]
pub struct HostView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub connection: Connection,
}

impl AgentHosts {
    /// No hosts, kept nowhere, and none connected to: until [`start`] loads the saved ones.
    pub fn empty() -> Self {
        Self {
            file: None,
            secrets: Arc::new(MemorySecrets::default()),
            connects: false,
            hosts: Vec::new(),
            prompts: Vec::new(),
            consents: Vec::new(),
            shown: None,
        }
    }

    /// `agent-hosts.json` in keefast's config directory, if the OS has one.
    pub fn file_in_os_config_dir() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("keefast").join("agent-hosts.json"))
    }

    pub fn views(&self) -> Vec<HostView> {
        self.hosts
            .iter()
            .map(|host| HostView {
                id: host.saved.id.clone(),
                name: host.saved.name.clone(),
                url: host.saved.url.clone(),
                connection: host.connection.clone(),
            })
            .collect()
    }

    /// The prompt to put to the user now: the oldest still waiting.
    pub fn front(&self) -> Option<&WaitingPrompt> {
        self.prompts.first()
    }

    /// How many prompts wait, the front one with them.
    pub fn waiting(&self) -> usize {
        self.prompts.len()
    }

    /// The consent to put to the user once no prompt waits: the oldest still waiting.
    pub fn front_consent(&self) -> Option<&WaitingConsent> {
        let now = SystemTime::now();
        self.consents.iter().find(|consent| consent.expires > now)
    }

    /// How many consents wait, the front one with them.
    pub fn consents_waiting(&self) -> usize {
        let now = SystemTime::now();
        self.consents
            .iter()
            .filter(|consent| consent.expires > now)
            .count()
    }

    fn save(&self) -> ApiResult<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let saved = SavedHosts {
            hosts: self.hosts.iter().map(|host| host.saved.clone()).collect(),
        };
        let json = serde_json::to_vec_pretty(&saved).expect("hosts are plain JSON");
        let failed = |error: std::io::Error| {
            ApiError::new(ErrorKind::Io, "keefast couldn't save its Agent hosts.")
                .with_detail(error.to_string())
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(failed)?;
        }
        let staged = file.with_extension("json.new");
        std::fs::write(&staged, json).map_err(failed)?;
        std::fs::rename(&staged, file).map_err(failed)
    }

    /// Takes in what a host's client heard; whether anything the UI shows changed.
    fn heard(&mut self, host: &str, event: ApproverClientEvent) -> bool {
        // This computer's own Agents ask too, through the in-app host, which is no saved host.
        let at = self.hosts.iter().position(|known| known.saved.id == host);
        let host_name = match at {
            Some(at) => self.hosts[at].saved.name.clone(),
            None if host == LOCAL_HOST => LOCAL_HOST_NAME.to_owned(),
            // A host removed while its client still ran.
            None => return false,
        };
        match event {
            ApproverClientEvent::Connected => {
                let Some(at) = at else { return false };
                self.hosts[at].connection = Connection::Connected;
            }
            ApproverClientEvent::Disconnected(why) => {
                let Some(at) = at else { return false };
                self.hosts[at].connection = Connection::Disconnected(why);
                self.prompts.retain(|prompt| prompt.host != host);
                self.consents.retain(|consent| consent.host != host);
            }
            ApproverClientEvent::Prompt {
                id,
                seconds_left,
                prompt,
            } => {
                // A stream opened again sends the prompts still waiting again.
                if self.prompts.iter().any(|known| known.id == id) {
                    return false;
                }
                self.prompts.push(WaitingPrompt {
                    host: host.to_owned(),
                    host_name: host_name.clone(),
                    id,
                    prompt,
                    expires: SystemTime::now() + Duration::from_secs(seconds_left.into()),
                });
            }
            ApproverClientEvent::Consent {
                id,
                seconds_left,
                consent,
            } => {
                if self.consents.iter().any(|known| known.id == id) {
                    return false;
                }
                self.consents.push(WaitingConsent {
                    host: host.to_owned(),
                    host_name,
                    id,
                    consent,
                    expires: SystemTime::now() + Duration::from_secs(seconds_left.into()),
                });
            }
            ApproverClientEvent::Dismissed { id } => {
                self.prompts.retain(|prompt| prompt.id != id);
                self.consents.retain(|consent| consent.id != id);
            }
        }
        true
    }

    /// Whether the front prompt or consent is one the user hasn't been shown, noting that they
    /// have.
    fn news(&mut self) -> bool {
        let front = self
            .front()
            .map(|prompt| prompt.id.clone())
            .or_else(|| self.front_consent().map(|consent| consent.id.clone()));
        let news = front.is_some() && front != self.shown;
        self.shown = front;
        news
    }
}

/// A secret store in memory: until [`start`] hands over the OS's, nothing is kept.
#[derive(Default)]
struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<SecretString>, keefast_workspace::SecretStoreError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .map(SecretString::from))
    }

    fn set(
        &self,
        name: &str,
        secret: &SecretString,
    ) -> Result<(), keefast_workspace::SecretStoreError> {
        let secret = secret.expose_secret().to_owned();
        self.0.lock().unwrap().insert(name.to_owned(), secret);
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), keefast_workspace::SecretStoreError> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

/// The token's name in the OS secret store.
fn secret_name(host: &str) -> String {
    format!("agent-host-{host}")
}

/// Loads the saved hosts, with their tokens from the OS secret store, and connects to each.
/// A host whose token is gone shows why, unconnected.
pub fn start(core: &Arc<AppCore>, file: PathBuf, secrets: Arc<dyn SecretStore>) {
    let saved: SavedHosts = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut hosts = AgentHosts {
        file: Some(file),
        secrets: Arc::clone(&secrets),
        connects: true,
        ..AgentHosts::empty()
    };
    for saved in saved.hosts {
        let token = match secrets.get(&secret_name(&saved.id)) {
            Ok(Some(token)) => Zeroizing::new(token.expose_secret().to_owned()),
            Ok(None) => {
                hosts.hosts.push(Host {
                    saved,
                    connection: Connection::Disconnected(
                        "its token is gone from the OS secret store: remove the host and add it \
                         again"
                            .to_owned(),
                    ),
                    _client: None,
                    token: Zeroizing::new(String::new()),
                });
                continue;
            }
            Err(error) => {
                hosts.hosts.push(Host {
                    saved,
                    connection: Connection::Disconnected(format!(
                        "its token couldn't be read from the OS secret store: {error}"
                    )),
                    _client: None,
                    token: Zeroizing::new(String::new()),
                });
                continue;
            }
        };
        let client = connect(core, &saved, &token);
        hosts.hosts.push(Host {
            saved,
            connection: Connection::Connecting,
            _client: client,
            token,
        });
    }
    *core.agent_hosts() = hosts;
}

fn connect(
    core: &Arc<AppCore>,
    saved: &SavedHost,
    token: &Zeroizing<String>,
) -> Option<ApproverClient> {
    let weak: Weak<AppCore> = Arc::downgrade(core);
    let id = saved.id.clone();
    Some(ApproverClient::start(
        AgentHost {
            url: saved.url.clone(),
            token: token.clone(),
        },
        move |event| {
            if let Some(core) = weak.upgrade() {
                heard(&core, &id, event);
            }
        },
    ))
}

/// Takes in what the host `host`'s client heard, and tells the UI.
pub fn heard(core: &AppCore, host: &str, event: ApproverClientEvent) {
    let (changed, news) = {
        let mut hosts = core.agent_hosts();
        let changed = hosts.heard(host, event);
        (changed, hosts.news())
    };
    if changed {
        core.agent_hosts_changed(news);
    }
}

/// Adds the host at `url` with the Approver token `token`, and connects to it.
pub fn add(core: &Arc<AppCore>, name: &str, url: &str, token: &str) -> ApiResult<HostView> {
    let url = check_url(url)?;
    let token = token.trim();
    if token.starts_with(keefast_agent::TOKEN_PREFIX) {
        return Err(ApiError::invalid(
            "That is an Agent pairing's token. Pair this desktop on the host with \
             `keefast-agent pairing approver <name>`, and paste the token it prints.",
        ));
    }
    if !token.starts_with(APPROVER_TOKEN_PREFIX) {
        return Err(ApiError::invalid(format!(
            "An Approver's token starts with {APPROVER_TOKEN_PREFIX}: get one on the host with \
             `keefast-agent pairing approver <name>`."
        )));
    }
    let name = match name.trim() {
        "" => host_name(&url),
        name => name.to_owned(),
    };
    let mut hosts = core.agent_hosts();
    if hosts.hosts.iter().any(|host| host.saved.url == url) {
        return Err(ApiError::new(
            ErrorKind::Conflict,
            "keefast is an Approver for that Agent host already.",
        ));
    }
    let saved = SavedHost {
        id: new_id(),
        name,
        url,
    };
    hosts
        .secrets
        .set(&secret_name(&saved.id), &SecretString::from(token))
        .map_err(|error| {
            ApiError::new(
                ErrorKind::Unavailable,
                "keefast couldn't keep the token in the OS secret store.",
            )
            .with_detail(error.to_string())
        })?;
    let token = Zeroizing::new(token.to_owned());
    let client = if hosts.connects {
        connect(core, &saved, &token)
    } else {
        None
    };
    hosts.hosts.push(Host {
        _client: client,
        saved: saved.clone(),
        connection: Connection::Connecting,
        token,
    });
    if let Err(error) = hosts.save() {
        hosts.hosts.pop();
        let _ = hosts.secrets.delete(&secret_name(&saved.id));
        return Err(error);
    }
    let view = hosts
        .views()
        .into_iter()
        .find(|view| view.id == saved.id)
        .expect("just added");
    drop(hosts);
    core.agent_hosts_changed(false);
    Ok(view)
}

/// Stops being an Approver for the host `id`: its stream closes, its prompts go, and its
/// token leaves the OS secret store. The host keeps the Approver paired until its operator
/// runs `keefast-agent pairing revoke-approver`.
pub fn remove(core: &AppCore, id: &str) -> ApiResult<()> {
    let removed = {
        let mut hosts = core.agent_hosts();
        let Some(at) = hosts.hosts.iter().position(|host| host.saved.id == id) else {
            return Err(ApiError::new(
                ErrorKind::NotFound,
                "That Agent host is gone already.",
            ));
        };
        let removed = hosts.hosts.remove(at);
        hosts.prompts.retain(|prompt| prompt.host != id);
        hosts.consents.retain(|consent| consent.host != id);
        let _ = hosts.secrets.delete(&secret_name(id));
        hosts.save()?;
        hosts.news();
        removed
    };
    // Closed off the lock: its thread may be telling us something.
    drop(removed);
    core.agent_hosts_changed(false);
    Ok(())
}

/// Answers the prompt `id` from the host `host`. It blocks while the answer travels. The
/// prompt goes either way: if another Approver answered first, or it timed out, the user is
/// told so.
pub fn answer(core: &AppCore, host: &str, id: &str, allow: bool, remember: bool) -> ApiResult<()> {
    if host == LOCAL_HOST {
        return answer_locally(core, id, allow, remember);
    }
    let agent_host = {
        let hosts = core.agent_hosts();
        let prompt = hosts
            .prompts
            .iter()
            .find(|prompt| prompt.id == id && prompt.host == host)
            .ok_or_else(gone)?;
        let remember = remember && prompt.prompt.can_remember;
        let host = hosts
            .hosts
            .iter()
            .find(|known| known.saved.id == host)
            .ok_or_else(gone)?;
        (
            AgentHost {
                url: host.saved.url.clone(),
                token: host.token.clone(),
            },
            remember,
        )
    };
    let (agent_host, remember) = agent_host;
    let answered = answer_prompt(&agent_host, id, ApproverAnswer { allow, remember });
    let news = {
        let mut hosts = core.agent_hosts();
        hosts.prompts.retain(|prompt| prompt.id != id);
        hosts.news()
    };
    core.agent_hosts_changed(news);
    answered.map_err(|error| match error {
        AnswerPromptError::Gone => gone(),
        AnswerPromptError::Unpaired => ApiError::new(
            ErrorKind::Unavailable,
            "The Agent host no longer knows this desktop as an Approver: pair it again.",
        ),
        AnswerPromptError::Unreachable(why) => ApiError::new(
            ErrorKind::Unavailable,
            "The Agent host couldn't be reached, so it will deny the request.",
        )
        .with_detail(why),
        AnswerPromptError::Refused(why) => ApiError::invalid(why),
    })
}

/// Answers the in-app host's prompt `id`: the Agent waiting on it goes on at once.
fn answer_locally(core: &AppCore, id: &str, allow: bool, remember: bool) -> ApiResult<()> {
    let remember = {
        let hosts = core.agent_hosts();
        let prompt = hosts
            .prompts
            .iter()
            .find(|prompt| prompt.id == id && prompt.host == LOCAL_HOST)
            .ok_or_else(gone)?;
        remember && prompt.prompt.can_remember
    };
    // Answering tells this desktop, among the in-app host's Approvers, to dismiss the prompt.
    let answered = core
        .in_app_agents()
        .approvers()
        .answer(id, ApproverAnswer { allow, remember });
    let news = {
        let mut hosts = core.agent_hosts();
        hosts.prompts.retain(|prompt| prompt.id != id);
        hosts.news()
    };
    core.agent_hosts_changed(news);
    answered.map_err(|_| gone())
}

/// Answers the OAuth consent `id` from the host `host`. It blocks while the answer travels.
/// Approving needs the `code` its consent page shows, and makes the Agent pairing `answer`
/// names. A wrong code, or a pairing the host can't make, leaves it waiting, to try again.
pub fn answer_agent_consent(
    core: &AppCore,
    host: &str,
    id: &str,
    answer: &ConsentAnswer,
) -> ApiResult<()> {
    let agent_host = {
        let hosts = core.agent_hosts();
        if !hosts
            .consents
            .iter()
            .any(|consent| consent.id == id && consent.host == host)
        {
            return Err(gone_consent());
        }
        let host = hosts
            .hosts
            .iter()
            .find(|known| known.saved.id == host)
            .ok_or_else(gone_consent)?;
        AgentHost {
            url: host.saved.url.clone(),
            token: host.token.clone(),
        }
    };
    let answered = answer_consent(&agent_host, id, answer);
    let done = !matches!(
        answered,
        Err(AnswerPromptError::Refused(_) | AnswerPromptError::Unreachable(_))
    );
    if done {
        let news = {
            let mut hosts = core.agent_hosts();
            hosts.consents.retain(|consent| consent.id != id);
            hosts.news()
        };
        core.agent_hosts_changed(news);
    }
    answered.map_err(|error| match error {
        AnswerPromptError::Gone => gone_consent(),
        AnswerPromptError::Refused(why) => ApiError::invalid(sentence(&why)),
        AnswerPromptError::Unpaired => ApiError::new(
            ErrorKind::Unavailable,
            "The Agent host no longer knows this desktop as an Approver: pair it again.",
        ),
        AnswerPromptError::Unreachable(why) => ApiError::new(
            ErrorKind::Unavailable,
            "The Agent host couldn't be reached. Try again.",
        )
        .with_detail(why),
    })
}

/// The latest `lines` records of the host `id`'s audit log, oldest first, for its Activity view.
/// It blocks while they travel.
pub fn activity(core: &AppCore, id: &str, lines: usize) -> ApiResult<Vec<AuditRecord>> {
    let agent_host = {
        let hosts = core.agent_hosts();
        let host = hosts
            .hosts
            .iter()
            .find(|known| known.saved.id == id)
            .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "That Agent host is gone."))?;
        AgentHost {
            url: host.saved.url.clone(),
            token: host.token.clone(),
        }
    };
    agent_activity(&agent_host, lines).map_err(|error| match error {
        ActivityError::Unpaired => ApiError::new(
            ErrorKind::Unavailable,
            "The Agent host no longer knows this desktop as an Approver: pair it again.",
        ),
        ActivityError::Unreachable(why) => ApiError::new(
            ErrorKind::Unavailable,
            "The Agent host couldn't be reached, so its Activity can't be shown.",
        )
        .with_detail(why),
    })
}

/// The host's reason, which reads as a command line's does, as a sentence for the dialog.
fn sentence(why: &str) -> String {
    let mut chars = why.chars();
    let mut sentence: String = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    if !sentence.ends_with('.') {
        sentence.push('.');
    }
    sentence
}

fn gone_consent() -> ApiError {
    ApiError::new(
        ErrorKind::Conflict,
        "That request was answered already, or it expired: connect again from the MCP client.",
    )
}

fn gone() -> ApiError {
    ApiError::new(
        ErrorKind::Conflict,
        "Another Approver answered first, or the request timed out.",
    )
}

/// The address as kept: trimmed, without a trailing `/`. Anywhere but this machine needs
/// HTTPS, as the host itself does: the token would otherwise cross the network in the clear.
fn check_url(url: &str) -> ApiResult<String> {
    let url = url.trim().trim_end_matches('/').to_owned();
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or_default();
        let loopback = host == "localhost" || host.starts_with("127.") || rest.starts_with("[::1]");
        if !loopback {
            return Err(ApiError::invalid(
                "An Agent host on another machine needs an https:// address.",
            ));
        }
    } else if !lower.starts_with("https://") || lower.len() <= "https://".len() {
        return Err(ApiError::invalid(
            "Give the Agent host's address, such as https://keefast.example.net:7807.",
        ));
    }
    Ok(url)
}

/// A name for a host from its address: its host name.
fn host_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest).to_owned()
}

fn new_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS has randomness");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::core::tests::core;
    use crate::events::AppEvent;

    fn prompt(command: &str) -> AgentPrompt {
        AgentPrompt {
            pairing: "claude".to_owned(),
            entry_uuid: "00".repeat(16),
            entry_title: "GitHub".to_owned(),
            reference: "claude/GitHub#password".to_owned(),
            program: command.to_owned(),
            command: vec![command.to_owned(), "auth".to_owned()],
            host: None,
            reveal: false,
            can_remember: true,
        }
    }

    fn prompted(id: &str, command: &str) -> ApproverClientEvent {
        ApproverClientEvent::Prompt {
            id: id.to_owned(),
            seconds_left: 60,
            prompt: prompt(command),
        }
    }

    /// The id of a host added to `core` at an address nothing listens on.
    fn added(core: &Arc<AppCore>) -> String {
        add(
            core,
            "build server",
            "http://127.0.0.1:9",
            "kfapprover_test-token",
        )
        .unwrap()
        .id
    }

    #[test]
    fn a_prompt_from_a_host_waits_for_the_user_and_brings_keefast_to_the_front_once() {
        let (core, recorder) = core();
        let host = added(&core);
        let fronts = *recorder.fronts.lock().unwrap();

        heard(&core, &host, ApproverClientEvent::Connected);
        heard(&core, &host, prompted("one", "gh"));
        heard(&core, &host, prompted("two", "npm"));

        let hosts = core.agent_hosts();
        let front = hosts.front().unwrap();
        assert_eq!(
            (front.id.as_str(), front.host_name.as_str()),
            ("one", "build server")
        );
        assert_eq!(hosts.waiting(), 2);
        assert_eq!(*recorder.fronts.lock().unwrap(), fronts + 1);
        assert!(
            recorder
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|event| matches!(event, AppEvent::WorkspaceChanged(_)))
        );
    }

    #[test]
    fn a_dismissed_prompt_goes_and_the_next_one_comes_to_the_front() {
        let (core, recorder) = core();
        let host = added(&core);
        heard(&core, &host, prompted("one", "gh"));
        heard(&core, &host, prompted("two", "npm"));
        let fronts = *recorder.fronts.lock().unwrap();

        heard(
            &core,
            &host,
            ApproverClientEvent::Dismissed {
                id: "one".to_owned(),
            },
        );

        assert_eq!(core.agent_hosts().front().unwrap().id, "two");
        assert_eq!(*recorder.fronts.lock().unwrap(), fronts + 1);
    }

    #[test]
    fn a_prompt_sent_again_after_reconnecting_shows_once() {
        let (core, _) = core();
        let host = added(&core);

        heard(&core, &host, prompted("one", "gh"));
        heard(&core, &host, prompted("one", "gh"));

        assert_eq!(core.agent_hosts().waiting(), 1);
    }

    #[test]
    fn a_dropped_stream_takes_its_hosts_prompts_and_says_why() {
        let (core, _) = core();
        let host = added(&core);
        heard(&core, &host, prompted("one", "gh"));

        heard(
            &core,
            &host,
            ApproverClientEvent::Disconnected("the connection dropped".to_owned()),
        );

        let hosts = core.agent_hosts();
        assert!(hosts.front().is_none());
        assert_eq!(
            hosts.views()[0].connection,
            Connection::Disconnected("the connection dropped".to_owned())
        );
    }

    #[test]
    fn answering_a_prompt_the_host_cant_be_reached_for_says_so_and_drops_it() {
        let (core, _) = core();
        let host = added(&core);
        heard(&core, &host, prompted("one", "gh"));

        let error = answer(&core, &host, "one", true, false).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Unavailable);
        assert!(core.agent_hosts().front().is_none());
    }

    #[test]
    fn the_activity_of_a_host_that_cant_be_reached_or_is_gone_says_so() {
        let (core, _) = core();
        let host = added(&core);

        let unreachable = activity(&core, &host, 10).unwrap_err();
        let gone = activity(&core, "nobody", 10).unwrap_err();

        assert!(
            unreachable.message.contains("couldn't be reached"),
            "{unreachable:?}"
        );
        assert!(gone.message.contains("gone"), "{gone:?}");
    }

    #[test]
    fn a_host_needs_an_approvers_token_and_https_unless_it_is_this_machine() {
        let (core, _) = core();

        let agent_token = add(&core, "", "https://host.example", "kfa_agent").unwrap_err();
        let plain = add(&core, "", "http://host.example", "kfapprover_x").unwrap_err();
        let loopback = add(&core, "", "http://localhost:7807/", "kfapprover_x").unwrap();

        assert!(
            agent_token.message.contains("pairing approver"),
            "{}",
            agent_token.message
        );
        assert!(plain.message.contains("https://"), "{}", plain.message);
        assert_eq!(
            (loopback.name.as_str(), loopback.url.as_str()),
            ("localhost:7807", "http://localhost:7807")
        );
    }

    fn consent_offered(id: &str) -> ApproverClientEvent {
        ApproverClientEvent::Consent {
            id: id.to_owned(),
            seconds_left: 600,
            consent: ConsentPrompt {
                client: "Executor".to_owned(),
                redirect: "executor.sh".to_owned(),
                subgroups: vec!["shared".to_owned()],
                paired_as: None,
            },
        }
    }

    #[test]
    fn an_oauth_consent_waits_behind_the_prompts_and_goes_when_dismissed() {
        let (core, recorder) = core();
        let host = added(&core);
        let fronts = *recorder.fronts.lock().unwrap();

        heard(&core, &host, consent_offered("c1"));
        heard(&core, &host, prompted("one", "gh"));
        let with_a_prompt = core.agent_hosts().front().map(|front| front.id.clone());
        heard(
            &core,
            &host,
            ApproverClientEvent::Dismissed {
                id: "one".to_owned(),
            },
        );
        let consent = core.agent_hosts().front_consent().cloned().unwrap();
        heard(
            &core,
            &host,
            ApproverClientEvent::Dismissed {
                id: "c1".to_owned(),
            },
        );

        assert_eq!(with_a_prompt.as_deref(), Some("one"));
        assert_eq!(consent.consent.client, "Executor");
        assert_eq!(consent.host_name, "build server");
        assert!(core.agent_hosts().front_consent().is_none());
        // The consent, then the prompt, then the consent again once the prompt went.
        assert_eq!(*recorder.fronts.lock().unwrap(), fronts + 3);
    }

    #[test]
    fn approving_a_consent_on_a_host_that_cant_be_reached_keeps_it_to_try_again() {
        let (core, _) = core();
        let host = added(&core);
        heard(&core, &host, consent_offered("c1"));

        let error = answer_agent_consent(
            &core,
            &host,
            "c1",
            &ConsentAnswer {
                approve: true,
                code: "ABCD-EFGH".to_owned(),
                name: "executor".to_owned(),
                reads: vec![],
                can_create: false,
            },
        )
        .unwrap_err();
        let gone = answer_agent_consent(
            &core,
            &host,
            "nope",
            &ConsentAnswer {
                approve: false,
                code: String::new(),
                name: String::new(),
                reads: vec![],
                can_create: false,
            },
        )
        .unwrap_err();

        assert_eq!(error.kind, ErrorKind::Unavailable);
        assert!(core.agent_hosts().front_consent().is_some());
        assert_eq!(gone.kind, ErrorKind::Conflict);
    }

    #[test]
    fn removing_a_host_drops_its_prompts_and_its_token() {
        let (core, _) = core();
        let host = added(&core);
        heard(&core, &host, prompted("one", "gh"));

        remove(&core, &host).unwrap();

        let hosts = core.agent_hosts();
        assert!(hosts.views().is_empty());
        assert!(hosts.front().is_none());
        assert!(hosts.secrets.get(&secret_name(&host)).unwrap().is_none());
    }
}
