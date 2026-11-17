//! The Approver's side of a Headless host's prompt stream: what a desktop keefast runs for each
//! Agent host it is paired with (ADR 0012 "Approvals").
//!
//! An [`ApproverClient`] holds the host's stream of server-sent events open on a thread of its
//! own, and hands each prompt and dismissal to its caller. When the stream drops, it says so and
//! connects again, waiting longer each time up to half a minute; the host sends the prompts
//! still waiting to every Approver that connects, so nothing is lost meanwhile. It uses plain
//! blocking HTTP (`ureq`), as the desktop app's other background work does.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::approver::{
    APPROVER_CONSENTS_PATH, APPROVER_EVENTS_PATH, APPROVER_PROMPTS_PATH, AgentPrompt,
    ApproverAnswer, ApproverEvent, ConsentAnswer, ConsentPrompt,
};

/// A Headless host an Approver is paired with.
#[derive(Clone)]
pub struct AgentHost {
    /// Its address: the one Agents use, with or without `/mcp`.
    pub url: String,
    /// The Approver's token, from `keefast-agent pairing approver`.
    pub token: Zeroizing<String>,
}

impl AgentHost {
    /// The host's address, without `/mcp` or a trailing `/`.
    fn base(&self) -> &str {
        let base = self.url.trim_end_matches('/');
        base.strip_suffix("/mcp").unwrap_or(base)
    }

    fn authorization(&self) -> String {
        format!("Bearer {}", self.token.as_str())
    }
}

/// What an [`ApproverClient`] tells its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApproverClientEvent {
    /// The stream is open: prompts reach this Approver.
    Connected,
    /// The stream couldn't be opened or dropped, and why. The prompts it showed are gone; the
    /// client connects again by itself.
    Disconnected(String),
    Prompt {
        id: String,
        seconds_left: u32,
        prompt: AgentPrompt,
    },
    /// An MCP client asks to pair by OAuth: approve the code its consent page shows.
    Consent {
        id: String,
        seconds_left: u32,
        consent: ConsentPrompt,
    },
    Dismissed {
        id: String,
    },
}

/// Why an answer didn't reach the host, or wasn't taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerPromptError {
    #[error("another Approver answered first, or the prompt timed out")]
    Gone,
    #[error("the Agent host refused this Approver's token: pair it again")]
    Unpaired,
    #[error("the Agent host can't be reached: {0}")]
    Unreachable(String),
    /// Refused as asked, such as a wrong code or a pairing name taken: the host's reason.
    #[error("{0}")]
    Refused(String),
}

/// The longest a stream stays open before it is opened afresh: a connection that died without
/// closing (a laptop put to sleep) is noticed at the latest then. The host sends a comment every
/// 15 s, so a live stream is never idle that long.
const STREAM_LIFETIME: Duration = Duration::from_secs(5 * 60);
const FIRST_RETRY: Duration = Duration::from_secs(2);
const LAST_RETRY: Duration = Duration::from_secs(30);

/// A connection to one Agent host's prompts, until dropped.
pub struct ApproverClient {
    host: AgentHost,
    stop: Arc<AtomicBool>,
}

impl ApproverClient {
    /// Connects to `host` on a thread of its own, telling `on_event` what happens, until the
    /// client is dropped.
    pub fn start(host: AgentHost, on_event: impl Fn(ApproverClientEvent) + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_host = host.clone();
        let thread_stop = Arc::clone(&stop);
        thread::spawn(move || stream(&thread_host, &thread_stop, &on_event));
        Self { host, stop }
    }

    /// Answers the prompt `id`. It blocks while the answer travels.
    pub fn answer(&self, id: &str, answer: ApproverAnswer) -> Result<(), AnswerPromptError> {
        answer_prompt(&self.host, id, answer)
    }
}

impl Drop for ApproverClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Answers the prompt `id` on `host`.
pub fn answer_prompt(
    host: &AgentHost,
    id: &str,
    answer: ApproverAnswer,
) -> Result<(), AnswerPromptError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );
    let url = format!("{}{APPROVER_PROMPTS_PATH}/{id}", host.base());
    let response = agent
        .post(&url)
        .header("Authorization", &host.authorization())
        .header("Content-Type", "application/json")
        .send(serde_json::to_string(&answer).expect("an answer is plain JSON"))
        .map_err(|error| AnswerPromptError::Unreachable(error.to_string()))?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(AnswerPromptError::Unpaired),
        409 => Err(AnswerPromptError::Gone),
        status => Err(AnswerPromptError::Unreachable(format!(
            "it answered {status}"
        ))),
    }
}

/// Answers the OAuth consent `id` on `host`: approving needs the code its page shows.
pub fn answer_consent(
    host: &AgentHost,
    id: &str,
    answer: &ConsentAnswer,
) -> Result<(), AnswerPromptError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );
    let url = format!("{}{APPROVER_CONSENTS_PATH}/{id}", host.base());
    let mut response = agent
        .post(&url)
        .header("Authorization", &host.authorization())
        .header("Content-Type", "application/json")
        .send(serde_json::to_string(answer).expect("an answer is plain JSON"))
        .map_err(|error| AnswerPromptError::Unreachable(error.to_string()))?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(AnswerPromptError::Unpaired),
        409 => Err(AnswerPromptError::Gone),
        status => {
            let message = response
                .body_mut()
                .read_to_string()
                .ok()
                .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
                .and_then(|body| body["message"].as_str().map(str::to_owned));
            match message {
                Some(message) if (400..500).contains(&status) => {
                    Err(AnswerPromptError::Refused(message))
                }
                _ => Err(AnswerPromptError::Unreachable(format!(
                    "it answered {status}"
                ))),
            }
        }
    }
}

/// Holds the stream open, opening it again whenever it drops, until `stop`.
fn stream(host: &AgentHost, stop: &AtomicBool, on_event: &dyn Fn(ApproverClientEvent)) {
    let mut retry = FIRST_RETRY;
    while !stop.load(Ordering::SeqCst) {
        let opened = Instant::now();
        let outcome = read_stream(host, stop, on_event, &mut retry);
        if stop.load(Ordering::SeqCst) {
            return;
        }
        // A stream that lived its whole time is opened again at once, quietly: the host sends
        // the prompts still waiting again, and the caller already shows them.
        if opened.elapsed() >= STREAM_LIFETIME {
            continue;
        }
        on_event(ApproverClientEvent::Disconnected(outcome));
        // Woken often, so a removed host's thread ends soon.
        let mut waited = Duration::ZERO;
        while waited < retry && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(200));
            waited += Duration::from_millis(200);
        }
        retry = (retry * 2).min(LAST_RETRY);
    }
}

/// Reads one stream until it ends, and says why it did.
fn read_stream(
    host: &AgentHost,
    stop: &AtomicBool,
    on_event: &dyn Fn(ApproverClientEvent),
    retry: &mut Duration,
) -> String {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(STREAM_LIFETIME))
            .build(),
    );
    let url = format!("{}{APPROVER_EVENTS_PATH}", host.base());
    let mut response = match agent
        .get(&url)
        .header("Authorization", &host.authorization())
        .header("Accept", "text/event-stream")
        .call()
    {
        Ok(response) => response,
        Err(error) => return format!("can't reach {}: {error}", host.base()),
    };
    match response.status().as_u16() {
        200 => {}
        401 => {
            return "the Agent host refused this Approver's token: pair it again with \
                    `keefast-agent pairing approver` on the host"
                .to_owned();
        }
        404 => return format!("{} isn't a keefast Agent host", host.base()),
        status => return format!("the Agent host answered {status}"),
    }
    *retry = FIRST_RETRY;
    on_event(ApproverClientEvent::Connected);
    let body = response.body_mut().as_reader();
    let mut data = String::new();
    for line in BufReader::new(body).lines() {
        if stop.load(Ordering::SeqCst) {
            return String::new();
        }
        let line = match line {
            Ok(line) => line,
            Err(error) => return format!("the connection dropped: {error}"),
        };
        if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.trim_start());
        } else if line.is_empty() && !data.is_empty() {
            match serde_json::from_str::<ApproverEvent>(&data) {
                Ok(ApproverEvent::Prompt {
                    id,
                    seconds_left,
                    prompt,
                }) => on_event(ApproverClientEvent::Prompt {
                    id,
                    seconds_left,
                    prompt,
                }),
                Ok(ApproverEvent::Consent {
                    id,
                    seconds_left,
                    consent,
                }) => on_event(ApproverClientEvent::Consent {
                    id,
                    seconds_left,
                    consent,
                }),
                Ok(ApproverEvent::Dismissed { id }) => {
                    on_event(ApproverClientEvent::Dismissed { id });
                }
                // An event a later host sends: not for this keefast.
                Err(_) => {}
            }
            data.clear();
        }
    }
    "the Agent host closed the connection".to_owned()
}
